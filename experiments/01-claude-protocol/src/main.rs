//! Experiment 1a (spec §16): the `claude -p` stream-json protocol on a live model (haiku).
//! Usage: `cargo run -p claude-protocol -- basic|resume|permission|interrupt`; logs go to `logs/`.

use claude_protocol::{Claude, is, kind, tagged, uuid, workdir};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

const S: fn(u64) -> Duration = Duration::from_secs;
const HAIKU: [&str; 2] = ["--model", "haiku"];

fn log(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("logs")
        .join(format!("{name}.ndjson"))
}

fn print(t: f64, v: &Value) {
    println!("{t:7.3} {}", kind(v));
}

fn result_line(v: &Value) -> String {
    format!(
        "is_error={} terminal_reason={} stop_reason={} result={:?}",
        v["is_error"], v["terminal_reason"], v["stop_reason"], v["result"]
    )
}

/// Silence before the first prompt, echo of the prompt, init per turn, exit on EOF.
fn basic() {
    let work = workdir("basic");
    let sid = uuid();
    let mut c = Claude::spawn(
        &work,
        &log("basic"),
        &[&HAIKU[..], &["--session-id", &sid, "--replay-user-messages"]].concat(),
        &[],
    );
    let early = c.next(S(3));
    println!(
        "before first prompt (3 s): {}",
        early.map_or("nothing".into(), |(_, v)| kind(&v))
    );
    for (n, text) in ["Reply with exactly: OK", "Reply with exactly: TWO"].iter().enumerate() {
        let id = uuid();
        let sent = c.t();
        c.user(text, &id);
        let (t, r) = c
            .until(
                S(60),
                |_, t, v| {
                    print(t, v);
                    if v["isReplay"] == true {
                        println!(
                            "        echo uuid matches ours: {}; +{:.3} s",
                            v["uuid"] == id.as_str(),
                            t - sent
                        );
                    }
                    if is(v, "system/init") {
                        println!(
                            "        init model={} session_id matches: {}",
                            v["model"],
                            v["session_id"] == sid.as_str()
                        );
                    }
                },
                |v| is(v, "result/success") || v["type"] == "result",
            )
            .expect("result");
        println!("turn {}: {:.3} s, {}", n + 1, t - sent, result_line(&r));
    }
    let eof = c.t();
    c.close_stdin();
    while c.next(S(10)).is_some() {}
    let status = c.child.wait().expect("wait");
    println!("exit after stdin EOF: {:.3} s, {status}; session {sid}", c.t() - eof);
}

/// Does `--resume` with stream-json input keep the context, and which model does it use with and without `--model`?
fn resume() {
    let work = workdir("resume");
    let sid = uuid();
    let mut a = Claude::spawn(
        &work,
        &log("resume-a"),
        &[&HAIKU[..], &["--session-id", &sid]].concat(),
        &[],
    );
    let first = uuid();
    a.user("Remember the word PINEAPPLE. Reply with exactly: OK", &first);
    a.until(S(60), |_, _, _| {}, |v| v["type"] == "result")
        .expect("result a");
    a.close_stdin();
    let _ = a.child.wait();

    let mut b = Claude::spawn(
        &work,
        &log("resume-b"),
        &[&HAIKU[..], &["--resume", &sid]].concat(),
        &[],
    );
    b.user(
        "Which word did I ask you to remember? Reply with that word only.",
        &uuid(),
    );
    let (_, r) = b
        .until(
            S(60),
            |_, _, v| {
                if is(v, "system/init") {
                    println!(
                        "resume --model haiku: init model={} session_id same: {}",
                        v["model"],
                        v["session_id"] == sid.as_str()
                    );
                }
            },
            |v| v["type"] == "result",
        )
        .expect("result b");
    println!("resume --model haiku: {}", result_line(&r));
    // A message uuid already in the transcript: deduplicated or a new turn?
    b.user("Reply with exactly: DUP", &first);
    let dup = b.until(
        S(30),
        |_, t, v| {
            if v["type"] == "command_lifecycle" || v["isReplay"] == true {
                println!("{t:7.3} dup: {} {}", v["type"], v["state"]);
            }
        },
        |v| v["type"] == "result",
    );
    println!(
        "resent uuid: {}",
        dup.map_or("no result in 30 s".into(), |(_, r)| result_line(&r))
    );
    b.close_stdin();
    let _ = b.child.wait();

    // Without --model: an unreachable API keeps any non-haiku model from running; init still names the model.
    let dead = [
        ("ANTHROPIC_BASE_URL", "http://127.0.0.1:9"),
        ("CLAUDE_CODE_MAX_RETRIES", "0"),
    ];
    for (name, args) in [("fresh", vec![]), ("resume", vec!["--resume", sid.as_str()])] {
        let mut c = Claude::spawn(&work, &log(&format!("resume-nomodel-{name}")), &args, &dead);
        c.user("Reply with exactly: OK", &uuid());
        let init = c.until(S(30), |_, _, _| {}, |v| is(v, "system/init"));
        println!(
            "{name} without --model: init model={}",
            init.map_or(json!(null), |(_, v)| v["model"].clone())
        );
        c.kill();
    }
}

/// Permission request over stdio: deny the first, allow the second.
fn permission() {
    let work = workdir("permission");
    let args = [
        &HAIKU[..],
        &["--permission-prompt-tool", "stdio", "--permission-mode", "default"],
    ]
    .concat();
    let mut c = Claude::spawn(
        &work,
        &log("permission"),
        &args,
        &[("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS", "1")],
    );
    c.user("Use the Bash tool to run exactly: touch probe-a.txt\nIf that is denied, use the Bash tool to run exactly: touch probe-b.txt\nThen reply DONE.", &uuid());
    let mut asked = 0;
    let (_, r) = c
        .until(S(120), |c, t, v| {
            print(t, v);
            if is(v, "system/session_state_changed") {
                println!("        state={}", v["state"]);
            }
            if is(v, "control_request/can_use_tool") {
                asked += 1;
                let req = &v["request"];
                let answer = if asked == 1 {
                    json!({"behavior":"deny","message":"Denied by the experiment."})
                } else {
                    json!({"behavior":"allow","updatedInput":req["input"]})
                };
                println!("        asks {} {}, answer {}", req["tool_name"], req["input"]["command"], answer["behavior"]);
                c.send(json!({"type":"control_response","response":{"subtype":"success","request_id":v["request_id"],"response":answer}}));
            }
        }, |v| v["type"] == "result")
        .expect("result");
    println!(
        "{}; files: a={} b={}",
        result_line(&r),
        work.join("probe-a.txt").exists(),
        work.join("probe-b.txt").exists()
    );
    c.close_stdin();
    let _ = c.child.wait();
}

/// Native `interrupt` control request during a foreground Bash command.
fn interrupt() {
    let work = workdir("interrupt");
    let mut c = Claude::spawn(
        &work,
        &log("interrupt"),
        &[&HAIKU[..], &["--allowedTools", "Bash"]].concat(),
        &[("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS", "1")],
    );
    c.user(
        "Use the Bash tool to run exactly: python3 -c 'import time; time.sleep(40)'\nThen reply DONE.",
        &uuid(),
    );
    c.until(
        S(60),
        |_, t, v| print(t, v),
        |v| v["message"]["content"][0]["type"] == "tool_use",
    )
    .expect("tool_use");
    std::thread::sleep(S(3));
    let sent = c.t();
    c.send(json!({"type":"control_request","request_id":"interrupt-1","request":{"subtype":"interrupt"}}));
    let (t, r) = c
        .until(
            S(60),
            |_, t, v| {
                print(t, v);
                if v["type"] == "control_response" {
                    println!("        control_response after {:.3} s: {}", t - sent, v["response"]);
                }
            },
            |v| v["type"] == "result",
        )
        .expect("result");
    println!("result {:.3} s after interrupt: {}", t - sent, result_line(&r));
    std::thread::sleep(S(1));
    println!(
        "tool processes alive after interrupt: {}",
        tagged(&c.tag).iter().filter(|p| **p != c.child.id()).count()
    );
    c.user("Reply with exactly: AFTER", &uuid());
    let (_, r) = c
        .until(S(60), |_, _, _| {}, |v| v["type"] == "result")
        .expect("result after");
    println!("next turn: {}", result_line(&r));
    c.close_stdin();
    let _ = c.child.wait();
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("basic") => basic(),
        Some("resume") => resume(),
        Some("permission") => permission(),
        Some("interrupt") => interrupt(),
        _ => eprintln!("usage: claude-protocol basic|resume|permission|interrupt"),
    }
}
