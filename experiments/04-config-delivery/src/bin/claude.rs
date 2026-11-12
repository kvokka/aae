//! Experiments 4–5 for claude (spec §16): hooks, skills, MCP and credentials delivered to `claude -p` without
//! writing into the rig. Scenarios: `flags` (out-of-rig files, inline JSON, resume without the flags), `sources`
//! (rig config under each `--setting-sources`), `auth` (own `CLAUDE_CONFIG_DIR`). Summary on stdout, logs in
//! `logs/claude/`. Live model: haiku.

use claude_protocol::{Claude, is, workdir};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LOGS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/logs/claude");

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn skill(dir: &Path, name: &str) {
    write(
        &dir.join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: Marker skill {name}.\n---\n\nSay {name}.\n"),
    );
}

/// Hooks that append their stdin to `<out>/<tag>-<event>.jsonl`.
fn hooks(out: &Path, tag: &str) -> Value {
    let h = |ev: &str| {
        let cmd = format!(
            "cat >> {}/{tag}-{ev}.jsonl; echo >> {}/{tag}-{ev}.jsonl",
            out.display(),
            out.display()
        );
        json!([{"matcher": "", "hooks": [{"type": "command", "command": cmd}]}])
    };
    json!({"UserPromptSubmit": h("ups"), "Stop": h("stop")})
}

/// How many times each hook of a tag fired.
fn fired(out: &Path, tag: &str) -> String {
    let n = |ev: &str| {
        let f = std::fs::read_to_string(out.join(format!("{tag}-{ev}.jsonl"))).unwrap_or_default();
        f.matches("hook_event_name").count()
    };
    format!("{tag}: UserPromptSubmit {}, Stop {}", n("ups"), n("stop"))
}

/// An MCP server config entry backed by the stdio server of the sibling `codex` binary.
fn mcp(tag: &str, root: &Path) -> Value {
    let exe = std::env::current_exe().unwrap().with_file_name("codex");
    json!({"type": "stdio", "command": exe, "args": ["mcp", tag, root]})
}

/// One turn: returns `system/init`, the final result and the ms from spawn to the result.
fn session(work: &Path, log: &str, args: &[&str], env: &[(&str, &str)], prompt: &str) -> (Value, Value, u128) {
    let log = if log.is_empty() {
        PathBuf::from("/dev/null")
    } else {
        Path::new(LOGS).join(log)
    };
    let mut c = Claude::spawn(work, &log, args, env);
    c.user(prompt, &claude_protocol::uuid());
    let mut init = Value::Null;
    let result = c.until(
        Duration::from_secs(120),
        |_, _, v| {
            if is(v, "system/init") {
                init = v.clone();
            }
        },
        |v| v["type"] == "result",
    );
    let ms = (c.t() * 1000.0) as u128;
    c.close_stdin();
    c.kill();
    (init, result.map(|r| r.1).unwrap_or_default(), ms)
}

fn names(init: &Value, key: &str) -> Vec<String> {
    let all = init[key].as_array().cloned().unwrap_or_default();
    all.iter()
        .map(|v| {
            v.as_str()
                .map_or_else(|| format!("{}={}", v["name"], v["status"]), str::to_owned)
        })
        .collect()
}

/// Of `marks`, those present in the list.
fn present(list: &[String], marks: &[&str]) -> Vec<String> {
    marks
        .iter()
        .filter(|m| list.iter().any(|s| s.contains(*m)))
        .map(|m| (*m).to_owned())
        .collect()
}

fn flags(root: &Path) {
    let (aux, rig) = (root.join("aux"), root.join("rig-flags"));
    std::fs::create_dir_all(&rig).unwrap();
    write(
        &aux.join("settings.json"),
        &json!({"hooks": hooks(&aux, "file")}).to_string(),
    );
    let plugin = aux.join("plugin");
    write(
        &plugin.join(".claude-plugin/plugin.json"),
        r#"{"name":"aae-p","version":"0.0.1","description":"x"}"#,
    );
    skill(&plugin.join("skills"), "p-skill");
    write(
        &plugin.join("hooks/hooks.json"),
        &json!({"hooks": hooks(&aux, "plugin")}).to_string(),
    );
    write(
        &aux.join("mcp.json"),
        &json!({"mcpServers": {"fileMcp": mcp("FILE", root)}}).to_string(),
    );
    let inline = json!({"mcpServers": {"inlineMcp": mcp("INLINE", root)}}).to_string();
    let (s, p, m) = (
        aux.join("settings.json"),
        plugin.display().to_string(),
        aux.join("mcp.json"),
    );
    let args = [
        "--model",
        "haiku",
        "--settings",
        s.to_str().unwrap(),
        "--plugin-dir",
        &p,
    ];
    let mcp_args = ["--mcp-config", m.to_str().unwrap(), "--mcp-config", &inline];
    let all: Vec<&str> = args.iter().chain(&mcp_args).copied().collect();
    let (init, res, _) = session(&rig, "flags.ndjson", &all, &[], "Reply OK");
    let skills = present(&names(&init, "skills"), &["p-skill"]);
    println!(
        "[flags] files + plugin + 2 MCP: result {}; skills {skills:?}",
        res["subtype"]
    );
    println!(
        "[flags] mcp_servers {:?}; hooks {}; {}",
        names(&init, "mcp_servers"),
        fired(&aux, "file"),
        fired(&aux, "plugin")
    );
    let sid = init["session_id"].as_str().unwrap_or("").to_owned();
    let (init, res, _) = session(
        &rig,
        "flags-resume.ndjson",
        &["--model", "haiku", "--resume", &sid],
        &[],
        "Reply OK",
    );
    let skills = present(&names(&init, "skills"), &["p-skill"]);
    println!(
        "[flags] resume without the flags: result {}; same session {}; skills {skills:?}",
        res["subtype"],
        init["session_id"] == sid.as_str()
    );
    println!(
        "[flags] resume: mcp_servers {:?}; hooks {}; {}",
        names(&init, "mcp_servers"),
        fired(&aux, "file"),
        fired(&aux, "plugin")
    );
    let inline = json!({"hooks": hooks(&aux, "inline")}).to_string();
    let (_, res, _) = session(
        &rig,
        "flags-inline.ndjson",
        &["--model", "haiku", "--settings", &inline],
        &[],
        "Reply OK",
    );
    println!(
        "[flags] inline --settings JSON: result {}; hooks {}",
        res["subtype"],
        fired(&aux, "inline")
    );
}

fn sources(root: &Path) {
    let prompt =
        "If your instructions contain a word that starts with RIGMARK, reply with that word only; else reply NONE.";
    let cases: [(&str, &[&str]); 4] = [
        ("empty", &["--setting-sources", "", "--strict-mcp-config"]),
        ("project", &["--setting-sources", "project"]),
        ("project-local", &["--setting-sources", "project,local"]),
        ("host", &["--setting-sources", "user,project,local"]),
    ];
    for (name, extra) in cases {
        let rig = root.join(format!("rig-{name}"));
        let out = root.join(format!("out-{name}"));
        std::fs::create_dir_all(&out).unwrap();
        let settings = json!({"hooks": hooks(&out, "rig"), "enableAllProjectMcpServers": true});
        write(&rig.join(".claude/settings.json"), &settings.to_string());
        skill(&rig.join(".claude/skills"), "rig-skill");
        write(&rig.join("CLAUDE.md"), "Project marker: RIGMARK-CLAUDE-MD.\n");
        write(
            &rig.join(".mcp.json"),
            &json!({"mcpServers": {"rigMcp": mcp("RIG", root)}}).to_string(),
        );
        let args: Vec<&str> = ["--model", "haiku"].iter().chain(extra).copied().collect();
        // The host run reads the operator's own config: its log is not kept, only counts are printed.
        let log = if name == "host" {
            String::new()
        } else {
            format!("sources-{name}.ndjson")
        };
        let env = [("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "0")];
        let (init, res, ms) = session(&rig, &log, &args, &env, prompt);
        let (sk, mc) = (names(&init, "skills"), names(&init, "mcp_servers"));
        let rig_mcp: Vec<&String> = mc.iter().filter(|m| m.contains("rigMcp")).collect();
        println!(
            "[sources] {name}: reply {:?}; rig-skill {}; rig MCP {rig_mcp:?}; hooks {}; other skills {}, other MCP {}, plugins {}; result in {ms} ms",
            res["result"].as_str().unwrap_or("").trim(),
            !present(&sk, &["rig-skill"]).is_empty(),
            fired(&out, "rig"),
            sk.len() - present(&sk, &["rig-skill"]).len(),
            mc.len() - rig_mcp.len(),
            init["plugins"].as_array().map_or(0, Vec::len)
        );
    }
}

fn auth(root: &Path) {
    let rig = root.join("rig-auth");
    let cfg = rig.join("config-dir");
    std::fs::create_dir_all(&cfg).unwrap();
    let home = std::env::var("HOME").unwrap();
    let creds: Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{home}/.claude/.credentials.json")).unwrap()).unwrap();
    let token = creds["claudeAiOauth"]["accessToken"].as_str().unwrap().to_owned();
    let dir = cfg.to_str().unwrap();
    for (name, env) in [
        ("auth-none", vec![("CLAUDE_CONFIG_DIR", dir)]),
        (
            "auth-token",
            vec![("CLAUDE_CONFIG_DIR", dir), ("CLAUDE_CODE_OAUTH_TOKEN", token.as_str())],
        ),
    ] {
        let (init, res, ms) = session(&rig, &format!("{name}.ndjson"), &["--model", "haiku"], &env, "Reply OK");
        let text = res["result"].as_str().unwrap_or("").replace(&token, "<token>");
        println!(
            "[auth] {name}: apiKeySource {}; result {} is_error {} {text:?}; {ms} ms",
            init["apiKeySource"], res["subtype"], res["is_error"]
        );
    }
}

/// Replaces the temp root in every log with `<root>`.
fn sanitize(root: &Path) {
    for e in std::fs::read_dir(LOGS).unwrap().flatten() {
        let s = std::fs::read_to_string(e.path()).unwrap();
        std::fs::write(e.path(), s.replace(root.to_str().unwrap(), "<root>")).unwrap();
    }
}

fn main() {
    let root = workdir("cfg");
    std::fs::create_dir_all(LOGS).unwrap();
    match std::env::args().nth(1).as_deref() {
        Some("flags") => flags(&root),
        Some("sources") => sources(&root),
        Some("auth") => auth(&root),
        _ => eprintln!("usage: claude flags | sources | auth"),
    }
    sanitize(&root);
}
