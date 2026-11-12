//! Experiment 6 (spec §16): a finisher CLI called from inside a claude and a codex session, before and after
//! resume. Usage: `cargo build -p finisher && cargo run -p finisher -- codex|claude`.

use codex_appserver::{Client, completed, setup, spawn, thread_id, turn};
use serde_json::json;
use std::io::Read;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

const FINISH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/debug/finish");

/// The daemon side: every connection's content.
fn receiver(sock: &str) -> Receiver<String> {
    let listener = std::os::unix::net::UnixListener::bind(sock).expect("bind");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for mut s in listener.incoming().map_while(Result::ok) {
            let mut text = String::new();
            let _ = s.read_to_string(&mut text);
            let _ = tx.send(text);
        }
    });
    rx
}

fn got(rx: &Receiver<String>) -> Option<String> {
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

/// Round 0 starts a thread with a per-thread token; rounds 1 and 2 resume it in a new app-server process, without
/// and with the thread config.
fn codex() {
    let env = setup("finisher");
    let sock = format!("{}/sock/daemon.sock", env.root);
    let rx = receiver(&sock);
    let mut vars = env.vars.clone();
    vars.extend([("AAE_SOCKET", sock.clone()), ("AAE_PROC_TOKEN", "proc".into())]);
    let mut id = String::new();
    for round in 0..3 {
        let mut child = spawn("stdio://", &vars);
        let mut c = Client::stdio(&mut child);
        c.initialize();
        let mut params =
            json!({"config": {"shell_environment_policy": {"set": {"AAE_ATTEMPT_TOKEN": format!("tok-{round}")}}}});
        if round == 0 {
            params["cwd"] = json!(format!("{}/ws-a", env.root));
            id = thread_id(&c.request("thread/start", params));
        } else {
            if round == 1 {
                params = json!({});
            }
            params["threadId"] = json!(id);
            c.request("thread/resume", params);
        }
        turn(
            &mut c,
            &id,
            &format!("RUN:{FINISH} round{round}"),
            &format!("key-{round}"),
        );
        let status = completed(&mut c, &id, 30);
        println!("[codex] round {round}: turn {status:?}; daemon got {:?}", got(&rx));
        let _ = child.kill().and_then(|()| child.wait());
    }
}

/// Round 0 is a fresh session, round 1 resumes it in a new process with another token.
fn claude() {
    let work = claude_protocol::workdir("finisher");
    let sock = work.join("daemon.sock").display().to_string();
    let rx = receiver(&sock);
    let sid = claude_protocol::uuid();
    for (round, how) in [(0, "--session-id"), (1, "--resume")] {
        let token = format!("tok-{round}");
        let log = format!("{}/logs/claude-{round}.ndjson", env!("CARGO_MANIFEST_DIR"));
        let args = ["--model", "haiku", "--permission-mode", "bypassPermissions", how, &sid];
        let env = [("AAE_SOCKET", sock.as_str()), ("AAE_ATTEMPT_TOKEN", token.as_str())];
        let mut c = claude_protocol::Claude::spawn(&work, log.as_ref(), &args, &env);
        let text = format!("Run this exact command with the Bash tool, then reply OK: {FINISH} round{round}");
        c.user(&text, &claude_protocol::uuid());
        let result = c.until(Duration::from_secs(120), |_, _, _| {}, |v| v["type"] == "result");
        let ok = result.is_some_and(|(_, v)| v["is_error"] == false);
        println!("[claude] round {round}: result ok {ok}; daemon got {:?}", got(&rx));
        c.close_stdin();
        let _ = c.child.wait();
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("codex") => codex(),
        Some("claude") => claude(),
        _ => eprintln!("usage: finisher codex|claude"),
    }
}
