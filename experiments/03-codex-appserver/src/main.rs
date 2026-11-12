//! Scenarios of experiment 2: `threads stdio|unix`, `reconnect`. Summary on stdout, message logs in `logs/`.

use codex_appserver::generated::protocol as p;
use codex_appserver::{Client, Env, completed, rss_kb, setup, spawn, thread_id, turn};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn hook(cmd: String) -> Value {
    json!([{"matcher": "", "hooks": [{"type": "command", "command": cmd, "timeout": 10}]}])
}

/// Thread params: own cwd, model, instructions, command env and hooks that dump their stdin into a file.
fn thread_params(env: &Env, tag: &str) -> Value {
    let dump = |ev: &str| {
        format!(
            "cat > {r}/hook-{tag}-{ev}.json; echo \"$AAE_TAG\" > {r}/hook-{tag}-{ev}.env",
            r = env.root
        )
    };
    json!({"cwd": format!("{}/ws-{}", env.root, tag.to_lowercase()), "model": format!("model-{tag}"),
           "approvalPolicy": "never", "sandbox": "danger-full-access",
           "developerInstructions": format!("MARK-{tag}"),
           "config": {"bypass_hook_trust": true, "shell_environment_policy": {"set": {"AAE_TAG": tag}},
                      "hooks": {"UserPromptSubmit": hook(dump("ups")), "Stop": hook(dump("stop"))}}})
}

fn save(env: &Env, c: &Client, file: &str) {
    let out: String = c
        .log
        .iter()
        .map(|(t, d, m)| format!("{}\n", json!({"t": t, "dir": d.to_string(), "msg": m})))
        .collect();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/logs/");
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(format!("{path}{file}"), out.replace(&env.root, "<work>")).unwrap();
}

fn hooks_of(c: &Client, thread: &str) -> Vec<String> {
    c.seen("hook/completed")
        .into_iter()
        .filter(|m| m["params"]["threadId"] == thread)
        .map(|m| {
            let n: p::HookCompletedNotification = serde_json::from_value(m["params"].clone()).unwrap();
            format!("{:?}/{:?}/{:?}", n.run.event_name, n.run.source, n.run.status)
        })
        .collect()
}

fn threads(transport: &str) {
    let env = setup(&format!("threads-{transport}"));
    let sock = format!("{}/sock/app.sock", env.root);
    let t0 = Instant::now();
    let mut child = spawn(
        &if transport == "unix" {
            format!("unix://{sock}")
        } else {
            "stdio://".into()
        },
        &env.vars,
    );
    let mut c = if transport == "unix" {
        Client::unix(&sock)
    } else {
        Client::stdio(&mut child)
    };
    let init = c.initialize();
    println!(
        "[{transport}] initialize in {} ms: {}",
        t0.elapsed().as_millis(),
        init["result"]["userAgent"]
    );
    std::thread::sleep(Duration::from_secs(2));
    println!("[{transport}] rss empty: {} kB", rss_kb(child.id()));
    let a = thread_id(&c.request("thread/start", thread_params(&env, "A")));
    let mut pb = thread_params(&env, "B");
    // B: its own set, only one hook; the trust bypass stays.
    pb["config"]["hooks"] = json!({"UserPromptSubmit": hook(format!("cat > {r}/hook-B-ups.json; echo \"$AAE_TAG\" > {r}/hook-B-ups.env", r = env.root))});
    let b = thread_id(&c.request("thread/start", pb));
    let started = Instant::now();
    turn(&mut c, &a, "Reply OK (A)", "key-a1");
    turn(&mut c, &b, "Reply OK (B)", "key-b1");
    let mut done = Vec::new();
    while done.len() < 2 {
        let Some(m) = c.wait(Duration::from_secs(30), |m| m["method"] == "turn/completed") else {
            break;
        };
        let n: p::TurnCompletedNotification = serde_json::from_value(m["params"].clone()).unwrap();
        done.push((
            if n.thread_id == a { "A" } else { "B" },
            n.turn.status,
            started.elapsed().as_millis(),
        ));
    }
    println!("[{transport}] two parallel turns (thread, status, ms): {done:?}");
    for (tag, id) in [("A", &a), ("B", &b)] {
        let mine: Vec<String> = env
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.model == format!("model-{tag}"))
            .map(|s| {
                let other = if tag == "A" { "MARK-B" } else { "MARK-A" };
                format!(
                    "own marker {}, other marker {}",
                    s.body.contains(&format!("MARK-{tag}")),
                    s.body.contains(other)
                )
            })
            .collect();
        let files: Vec<String> = ["ups", "stop"]
            .iter()
            .filter_map(|ev| {
                let f = std::fs::read_to_string(format!("{}/hook-{tag}-{ev}.json", env.root)).ok()?;
                let v: Value = serde_json::from_str(&f).ok()?;
                let tag_env = std::fs::read_to_string(format!("{}/hook-{tag}-{ev}.env", env.root)).unwrap_or_default();
                Some(format!(
                    "{ev}: session_id is thread {}, AAE_TAG={:?}",
                    v["session_id"] == id.as_str(),
                    tag_env.trim()
                ))
            })
            .collect();
        println!(
            "[{transport}] thread {tag}: provider requests {mine:?}; hook files {files:?}; hook/completed {:?}",
            hooks_of(&c, id)
        );
    }
    println!("[{transport}] rss after 2 threads: {} kB", rss_kb(child.id()));

    // Idempotency: the same clientUserMessageId again on an idle thread.
    let before = env.seen.lock().unwrap().len();
    turn(&mut c, &a, "Reply OK (A again)", "key-a1");
    let again = completed(&mut c, &a, 30);
    let read = c.request("thread/read", json!({"threadId": a, "includeTurns": true}));
    let ids: Vec<&Value> = read["result"]["thread"]["turns"]
        .as_array()
        .map(|t| {
            t.iter()
                .flat_map(|t| t["items"].as_array().into_iter().flatten())
                .filter_map(|i| i.get("clientId"))
                .collect()
        })
        .unwrap_or_default();
    println!(
        "[{transport}] repeated clientUserMessageId: turn {again:?}, provider requests +{}, clientIds in history {ids:?}",
        env.seen.lock().unwrap().len() - before
    );

    for tag in ["C", "D"] {
        let mut params = thread_params(&env, tag);
        params["cwd"] = json!(format!("{}/ws-a", env.root));
        if tag == "C" {
            // C: the full set without the trust bypass.
            params["config"].as_object_mut().unwrap().remove("bypass_hook_trust");
        }
        let id = thread_id(&c.request("thread/start", params));
        turn(&mut c, &id, "Reply OK", &format!("key-{tag}"));
        completed(&mut c, &id, 30);
        println!(
            "[{transport}] thread {tag} (bypass_hook_trust {}): hook/completed {:?}",
            tag != "C",
            hooks_of(&c, &id)
        );
    }
    println!("[{transport}] rss after 4 threads: {} kB", rss_kb(child.id()));
    save(&env, &c, &format!("threads-{transport}.ndjson"));
    let _ = child.kill().and_then(|()| child.wait());
}

fn reconnect() {
    let env = setup("reconnect");
    let sock = format!("{}/sock/app.sock", env.root);
    let mut child = spawn(&format!("unix://{sock}"), &env.vars);
    let mut c1 = Client::unix(&sock);
    c1.initialize();
    let a = thread_id(&c1.request("thread/start", thread_params(&env, "A")));
    turn(&mut c1, &a, "SLOW6 then reply OK", "key-r1");
    c1.wait(Duration::from_secs(10), |m| m["method"] == "turn/started");
    save(&env, &c1, "reconnect-1.ndjson");
    drop(c1);
    std::thread::sleep(Duration::from_secs(1));
    println!(
        "[reconnect] first client gone; app-server alive: {}",
        child.try_wait().unwrap().is_none()
    );
    let mut c2 = Client::unix(&sock);
    c2.initialize();
    let loaded = c2.request("thread/loaded/list", json!({}));
    println!(
        "[reconnect] loaded threads: {}",
        loaded["result"]["data"].as_array().map_or(0, Vec::len)
    );
    let r = c2.request("thread/resume", json!({"threadId": a, "excludeTurns": true}));
    let resumed: p::ThreadResumeResponse = serde_json::from_value(r["result"].clone()).expect("ThreadResumeResponse");
    println!("[reconnect] resume: thread status {:?}", resumed.thread.status);
    let t = Instant::now();
    let done = completed(&mut c2, &a, 20);
    println!(
        "[reconnect] cut turn seen by the second client: {done:?} after {} ms",
        t.elapsed().as_millis()
    );
    turn(&mut c2, &a, "Reply OK", "key-r2");
    println!(
        "[reconnect] next turn from the second client: {:?}",
        completed(&mut c2, &a, 20)
    );
    save(&env, &c2, "reconnect-2.ndjson");
    let _ = child.kill().and_then(|()| child.wait());

    // stdio: the client closes stdin in the middle of a turn.
    let env = setup("stdio-eof");
    let mut child = spawn("stdio://", &env.vars);
    let mut c = Client::stdio(&mut child);
    c.initialize();
    let a = thread_id(&c.request("thread/start", thread_params(&env, "A")));
    turn(&mut c, &a, "SLOW6 then reply OK", "key-e1");
    c.wait(Duration::from_secs(10), |m| m["method"] == "turn/started");
    std::thread::sleep(Duration::from_secs(1));
    drop(c);
    let t = Instant::now();
    while child.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let requests = env
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.path.ends_with("/responses"))
        .count();
    println!(
        "[stdio-eof] app-server exited {:?} after {} ms; responses requests {requests}",
        child.try_wait().unwrap(),
        t.elapsed().as_millis()
    );
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("threads") => threads(&std::env::args().nth(2).unwrap_or_else(|| "stdio".into())),
        Some("reconnect") => reconnect(),
        _ => eprintln!("usage: codex-appserver threads stdio|unix | reconnect"),
    }
}
