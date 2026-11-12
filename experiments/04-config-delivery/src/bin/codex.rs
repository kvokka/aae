//! Experiments 4–5 for codex (spec §16): what reaches a thread of `codex app-server` without writing into the rig.
//! Scenarios: `mcp` (stdio and HTTP MCP per thread), `rig` (rig config, skills and a separate `CODEX_HOME`),
//! `process` (a host-like `CODEX_HOME`, process-wide hooks, the provider key). Summary on stdout, logs in `logs/codex/`.

use codex_appserver::{Client, Env, completed, setup, thread_id, turn};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KEY: &str = "fake-key-1234";

fn spawn(env: &Env, args: &[&str]) -> Child {
    let mut cmd = Command::new("codex");
    cmd.args(["app-server", "--listen", "stdio://"]).args(args).env_clear();
    cmd.envs(env.vars.iter().map(|(k, v)| (k, v)));
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    cmd.spawn().expect("spawn codex app-server")
}

fn skill(dir: &str, name: &str) {
    std::fs::create_dir_all(format!("{dir}/{name}")).unwrap();
    let md = format!("---\nname: {name}\ndescription: Marker skill {name}.\n---\n\nSay {name}.\n");
    std::fs::write(format!("{dir}/{name}/SKILL.md"), md).unwrap();
}

/// Thread params with its own cwd and model (the fake provider tells threads apart by model).
fn params(env: &Env, ws: &str, model: &str, config: Value) -> Value {
    std::fs::create_dir_all(format!("{}/{ws}", env.root)).unwrap();
    json!({"cwd": format!("{}/{ws}", env.root), "model": model, "approvalPolicy": "never",
           "sandbox": "danger-full-access", "config": config})
}

/// Starts a thread, runs one turn and returns the thread id with the bodies of its provider requests.
fn run(c: &mut Client, env: &Env, p: Value) -> (String, String) {
    let model = p["model"].as_str().unwrap().to_owned();
    let id = thread_id(&c.request("thread/start", p));
    turn(c, &id, "Reply OK", &format!("key-{model}"));
    completed(c, &id, 60);
    let seen = env.seen.lock().unwrap();
    let body = seen
        .iter()
        .filter(|s| s.model == model)
        .map(|s| s.body.as_str())
        .collect();
    (id, body)
}

fn has(body: &str, marks: &[&str]) -> String {
    let found: Vec<_> = marks.iter().filter(|m| body.contains(*m)).collect();
    format!("{found:?}")
}

fn skills(c: &mut Client, cwd: String) -> Vec<String> {
    let r = c.request("skills/list", json!({"cwds": [cwd], "forceReload": true}));
    let all = r["result"]["data"].as_array().cloned().unwrap_or_default();
    all.iter()
        .flat_map(|e| e["skills"].as_array().cloned().unwrap_or_default())
        .filter(|s| !s["path"].as_str().unwrap_or("").contains("/.system/"))
        .map(|s| {
            format!(
                "{}({})",
                s["name"].as_str().unwrap_or(""),
                s["scope"].as_str().unwrap_or("")
            )
        })
        .collect()
}

fn hooks_fired(c: &Client, thread: &str) -> Vec<String> {
    c.seen("hook/completed")
        .into_iter()
        .filter(|m| m["params"]["threadId"] == thread)
        .map(|m| {
            format!(
                "{}/{}",
                m["params"]["run"]["eventName"].as_str().unwrap_or(""),
                m["params"]["run"]["source"].as_str().unwrap_or("")
            )
        })
        .collect()
}

fn save(env: &Env, c: &Client, file: &str) {
    let out: String = c
        .log
        .iter()
        .map(|(t, d, m)| format!("{}\n", json!({"t": t, "dir": d.to_string(), "msg": m})))
        .collect();
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/logs/codex/");
    std::fs::create_dir_all(dir).unwrap();
    let out = out.replace(&env.root, "<work>").replace(KEY, "<key>");
    let exe = std::env::current_exe().unwrap();
    std::fs::write(format!("{dir}{file}"), out.replace(exe.to_str().unwrap(), "<exe>")).unwrap();
}

/// Pids written by the MCP servers of a tag, each with whether it is still alive.
fn mcp_pids(env: &Env, tag: &str) -> Vec<(String, bool)> {
    let f = std::fs::read_to_string(format!("{}/mcp-{tag}.pid", env.root)).unwrap_or_default();
    f.lines()
        .map(|p| (p.to_owned(), std::path::Path::new(&format!("/proc/{p}")).exists()))
        .collect()
}

fn mcp_stdio(tag: &str, exe: &str, root: &str) -> Value {
    json!({"command": exe, "args": ["mcp", tag, root]})
}

/// A stdio MCP server with one tool `tag_<TAG>`; records its pid.
fn mcp_server(tag: &str, root: &str) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(format!("{root}/mcp-{tag}.pid"))
        .unwrap();
    writeln!(f, "{}", std::process::id()).unwrap();
    let mut out = std::io::stdout();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(m) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = m.get("id") else { continue };
        let result = match m["method"].as_str() {
            Some("initialize") => json!({"protocolVersion": m["params"]["protocolVersion"],
                "capabilities": {"tools": {}}, "serverInfo": {"name": format!("mcp-{tag}"), "version": "0.0.1"}}),
            Some("tools/list") => json!({"tools": [{"name": format!("tag_{tag}"), "description": "Marker tool.",
                "inputSchema": {"type": "object", "properties": {}}}]}),
            _ => json!({}),
        };
        writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap();
    }
}

/// Accepts HTTP connections, keeps the request heads and answers 404.
fn http_sink() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let h = heads.clone();
    std::thread::spawn(move || {
        for mut conn in l.incoming().map_while(Result::ok) {
            let mut r = BufReader::new(conn.try_clone().unwrap());
            let (mut head, mut line) = (String::new(), String::new());
            while r.read_line(&mut line).unwrap_or(0) > 2 {
                head.push_str(&line);
                line.clear();
            }
            let n = head
                .to_ascii_lowercase()
                .lines()
                .find_map(|l| l.strip_prefix("content-length:")?.trim().parse().ok());
            let _ = r.read_exact(&mut vec![0; n.unwrap_or(0)]);
            h.lock().unwrap().push(
                head.lines()
                    .filter(|l| !l.to_ascii_lowercase().starts_with("host:"))
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            let _ = write!(
                conn,
                "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            );
        }
    });
    (port, heads)
}

fn mcp() {
    let env = setup("cfg-mcp");
    let exe = std::env::current_exe().unwrap().display().to_string();
    let (port, heads) = http_sink();
    let mut child = spawn(&env, &[]);
    let mut c = Client::stdio(&mut child);
    c.initialize();
    let pa = params(
        &env,
        "ws-a",
        "model-A",
        json!({"mcp_servers": {"a": mcp_stdio("A", &exe, &env.root)}}),
    );
    let (a, body_a) = run(&mut c, &env, pa);
    let pb = params(
        &env,
        "ws-b",
        "model-B",
        json!({"mcp_servers": {"b": mcp_stdio("B", &exe, &env.root)}}),
    );
    let (_, body_b) = run(&mut c, &env, pb);
    let http = json!({"url": format!("http://127.0.0.1:{port}/mcp"), "http_headers": {"X-Aae-Agent": "agent-c"}});
    let (_, _) = run(
        &mut c,
        &env,
        params(&env, "ws-c", "model-C", json!({"mcp_servers": {"h": http}})),
    );
    for (t, body) in [("A", &body_a), ("B", &body_b)] {
        println!(
            "[mcp] thread {t}: tools in provider request {}",
            has(body, &["tag_A", "tag_B"])
        );
    }
    let st = c.request("mcpServerStatus/list", json!({"threadId": a}));
    let names: Vec<&Value> = st["result"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| &s["name"])
        .collect();
    println!("[mcp] mcpServerStatus/list for thread A: {names:?}");
    println!("[mcp] HTTP MCP of thread C, requests seen: {:?}", heads.lock().unwrap());
    println!(
        "[mcp] server pids (pid, alive): A {:?}, B {:?}",
        mcp_pids(&env, "A"),
        mcp_pids(&env, "B")
    );
    c.request("thread/unsubscribe", json!({"threadId": a}));
    std::thread::sleep(Duration::from_secs(2));
    println!(
        "[mcp] after thread/unsubscribe A: A {:?}, B {:?}",
        mcp_pids(&env, "A"),
        mcp_pids(&env, "B")
    );
    let before = mcp_pids(&env, "B");
    save(&env, &c, "mcp.ndjson");
    drop(c);
    let t = Instant::now();
    while child.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_secs(1));
    println!(
        "[mcp] app-server exit {:?} after stdin EOF; servers alive before {before:?}, after: A {:?}, B {:?}",
        child.try_wait().unwrap(),
        mcp_pids(&env, "A"),
        mcp_pids(&env, "B")
    );
}

fn rig() {
    let env = setup("cfg-rig");
    for ws in ["ws-a", "ws-b"] {
        let r = format!("{}/{ws}", env.root);
        std::fs::create_dir_all(format!("{r}/.codex")).unwrap();
        std::fs::write(
            format!("{r}/.codex/config.toml"),
            "developer_instructions = \"RIG-CONFIG-MARK\"\n",
        )
        .unwrap();
        std::fs::write(format!("{r}/AGENTS.md"), "RIG-AGENTS-MARK\n").unwrap();
        skill(&format!("{r}/.agents/skills"), "rig-skill");
    }
    skill(&format!("{}/home/.agents/skills", env.root), "home-agents-skill");
    skill(&format!("{}/codex-home/skills", env.root), "codex-home-skill");
    skill(&format!("{}/extra", env.root), "extra-root-skill");
    let trust = format!("\n[projects.\"{}/ws-b\"]\ntrust_level = \"trusted\"\n", env.root);
    let cfg = format!("{}/codex-home/config.toml", env.root);
    std::fs::write(&cfg, std::fs::read_to_string(&cfg).unwrap() + &trust).unwrap();
    let mut child = spawn(&env, &[]);
    let mut c = Client::stdio(&mut child);
    c.initialize();
    let r = c.request(
        "skills/extraRoots/set",
        json!({"extraRoots": [format!("{}/extra", env.root)]}),
    );
    println!("[rig] skills/extraRoots/set: error {}", r["error"]);
    let marks = [
        "RIG-CONFIG-MARK",
        "RIG-AGENTS-MARK",
        "rig-skill",
        "home-agents-skill",
        "codex-home-skill",
        "extra-root-skill",
    ];
    for (ws, model, trusted) in [("ws-a", "model-A", false), ("ws-b", "model-B", true)] {
        let (_, body) = run(&mut c, &env, params(&env, ws, model, json!({})));
        let listed = skills(&mut c, format!("{}/{ws}", env.root));
        println!(
            "[rig] {ws} (trusted {trusted}): provider request has {}; skills/list {listed:?}",
            has(&body, &marks)
        );
    }
    save(&env, &c, "rig.ndjson");
    let _ = child.kill().and_then(|()| child.wait());
}

fn process() {
    let mut env = setup("cfg-process");
    let exe = std::env::current_exe().unwrap().display().to_string();
    let r = env.root.clone();
    let cfg = format!("{r}/codex-home/config.toml");
    let host = format!(
        "developer_instructions = \"HOST-MARK\"\nbypass_hook_trust = true\n{}env_key = \"AAE_FAKE_KEY\"\n\
         [mcp_servers.host]\ncommand = \"{exe}\"\nargs = [\"mcp\", \"HOST\", \"{r}\"]\n",
        std::fs::read_to_string(&cfg).unwrap()
    );
    std::fs::write(&cfg, host).unwrap();
    let dump = |ev: &str| format!("cat > {r}/hook-{ev}.json");
    let hooks = json!({"hooks": {"Stop": [{"matcher": "", "hooks": [{"type": "command", "command": dump("stop"), "timeout": 10}]}]}});
    std::fs::write(format!("{r}/codex-home/hooks.json"), hooks.to_string()).unwrap();
    skill(&format!("{r}/codex-home/skills"), "codex-home-skill");
    env.vars.push(("AAE_FAKE_KEY", KEY.into()));
    let flag = format!(
        "hooks.UserPromptSubmit=[{{matcher=\"\",hooks=[{{type=\"command\",command=\"{}\",timeout=10}}]}}]",
        dump("ups")
    );
    let mut child = spawn(&env, &["-c", &flag]);
    let mut c = Client::stdio(&mut child);
    c.initialize();
    let (t1, body) = run(&mut c, &env, params(&env, "ws-a", "model-A", json!({})));
    let auth = |model: &str| -> Vec<String> {
        let seen = env.seen.lock().unwrap();
        let heads = seen
            .iter()
            .filter(|s| s.model == model)
            .map(|s| s.head.to_ascii_lowercase());
        heads
            .filter_map(|h| {
                Some(
                    h.lines()
                        .find(|l| l.starts_with("authorization:"))?
                        .replace(&KEY.to_ascii_lowercase(), "<key>"),
                )
            })
            .collect()
    };
    println!(
        "[process] thread 1: provider request has {}; Authorization {:?}",
        has(&body, &["HOST-MARK", "tag_HOST", "codex-home-skill"]),
        auth("model-A")
    );
    println!(
        "[process] thread 1: hook/completed {:?}; MCP HOST pids {:?}",
        hooks_fired(&c, &t1),
        mcp_pids(&env, "HOST")
    );
    let (t3, _) = run(
        &mut c,
        &env,
        params(&env, "ws-c", "model-C", json!({"bypass_hook_trust": true})),
    );
    println!(
        "[process] thread 3 with bypass_hook_trust in thread config: hook/completed {:?}",
        hooks_fired(&c, &t3)
    );
    let overrides = [
        ("model-B", json!({"experimental_bearer_token": "thread-key"})),
        ("model-D", json!({"env_key": "AAE_OTHER_KEY"})),
    ];
    for (model, own) in overrides {
        let p = params(&env, "ws-b", model, json!({"model_providers": {"fake": own.clone()}}));
        let id = thread_id(&c.request("thread/start", p));
        turn(&mut c, &id, "Reply OK", &format!("key-{model}"));
        let done = c.wait(Duration::from_secs(60), |m| {
            m["method"] == "turn/completed" && m["params"]["threadId"] == id.as_str()
        });
        let t = done.map(|m| m["params"]["turn"].clone()).unwrap_or_default();
        println!(
            "[process] thread with provider override {own}: turn {} {}; Authorization {:?}",
            t["status"],
            t["error"]["message"],
            auth(model)
        );
    }
    save(&env, &c, "process.ndjson");
    let _ = child.kill().and_then(|()| child.wait());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("mcp") if args.len() == 4 => mcp_server(&args[2], &args[3]),
        Some("mcp") => mcp(),
        Some("rig") => rig(),
        Some("process") => process(),
        _ => eprintln!("usage: codex mcp | rig | process"),
    }
}
