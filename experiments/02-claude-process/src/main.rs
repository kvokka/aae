//! Experiment 1b (spec §16): claude processes, descendants and background work on a live model (haiku).
//! Usage: `cargo run -p claude-process -- orphans [subreaper] | background`; logs go to `logs/`.

use claude_protocol::{Claude, is, kind, tagged, uuid, workdir};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const S: fn(u64) -> Duration = Duration::from_secs;
const STATE: (&str, &str) = ("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS", "1");

fn log(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("logs")
        .join(format!("{name}.ndjson"))
}

/// `pid ppid pgid sid comm` from `/proc/<pid>/stat`.
fn stat(pid: u32) -> String {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let (Some(open), Some(close)) = (s.find('('), s.rfind(')')) else {
        return format!("{pid} gone");
    };
    let f: Vec<&str> = s[close + 2..].split(' ').collect();
    format!(
        "pid={pid} ppid={} pgid={} sid={} state={} comm={}",
        f[1],
        f[2],
        f[3],
        f[0],
        &s[open + 1..close]
    )
}

/// `kill -9` of claude's process group in the middle of a foreground Bash command; who survives.
fn orphans(subreaper: bool) {
    if subreaper {
        // SAFETY: plain prctl call without pointers.
        let rc = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
        assert_eq!(rc, 0, "PR_SET_CHILD_SUBREAPER");
    }
    let name = if subreaper { "orphans-subreaper" } else { "orphans" };
    let mut c = Claude::spawn(
        &workdir(name),
        &log(name),
        &["--model", "haiku", "--allowedTools", "Bash"],
        &[STATE],
    );
    c.user(
        "Use the Bash tool to run exactly: python3 -c 'import os,time; os.fork(); time.sleep(100)'\nThen reply DONE.",
        &uuid(),
    );
    c.until(
        S(60),
        |_, _, _| {},
        |v| v["message"]["content"][0]["type"] == "tool_use",
    )
    .expect("tool_use");
    let started = c.t();
    // Without the subreaper also wait for two heartbeats of the long tool.
    let beats = if subreaper { 0 } else { 2 };
    let mut seen = 0;
    if beats > 0 {
        c.until(
            S(90),
            |_, t, v| {
                if is(v, "tool_progress") {
                    seen += 1;
                    println!(
                        "tool_progress at +{:.1} s after tool_use, elapsed_time_seconds={}",
                        t - started,
                        v["elapsed_time_seconds"]
                    );
                }
            },
            |v| is(v, "tool_progress") && v["elapsed_time_seconds"].as_f64() >= Some(55.0),
        );
    } else {
        std::thread::sleep(S(5));
    }
    let me = std::process::id();
    println!(
        "experiment pid={me} subreaper={subreaper}; claude {}",
        stat(c.child.id())
    );
    for p in tagged(&c.tag).into_iter().filter(|p| *p != c.child.id()) {
        println!("  before: {}", stat(p));
    }
    c.kill();
    std::thread::sleep(S(1));
    let survivors = tagged(&c.tag);
    for p in &survivors {
        println!("  after kill(-pgid): {}", stat(*p));
    }
    println!("survivors: {}", survivors.len());
    for p in survivors {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &p.to_string()])
            .status();
    }
    std::thread::sleep(S(1));
    // SAFETY: reaping adopted orphans; null status pointer is allowed.
    while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
    println!("left after sweep by env tag: {}", tagged(&c.tag).len());
}

/// Background shell command plus background subagent; both "idle" signals in one run.
fn background() {
    let args = ["--model", "haiku", "--allowedTools", "Bash,Agent,Task"];
    let mut c = Claude::spawn(&workdir("background"), &log("background"), &args, &[STATE]);
    c.user(
        "Do exactly these steps without waiting for anything:\n\
         1) Use the Bash tool with run_in_background set to true to run: python3 -c 'import time; time.sleep(20)'\n\
         2) Use the Agent tool to start a general-purpose subagent with the prompt: \
         \"Use the Bash tool to run: python3 -c 'import time; time.sleep(10)' and then reply sub-ok\"\n\
         3) Reply STARTED.",
        &uuid(),
    );
    let (mut state, mut bg, mut results) = (String::from("?"), 0usize, 0);
    let end = Instant::now() + S(150);
    while let Some((t, v)) = c.next(S(20).min(end.saturating_duration_since(Instant::now()))) {
        let k = kind(&v);
        let note = match k.as_str() {
            "system/session_state_changed" => {
                state = v["state"].as_str().unwrap_or("?").into();
                state.clone()
            }
            "system/background_tasks_changed" => {
                let tasks = v["tasks"].as_array().cloned().unwrap_or_default();
                bg = tasks.len();
                tasks
                    .iter()
                    .map(|t| t["task_type"].as_str().unwrap_or("?"))
                    .collect::<Vec<_>>()
                    .join(",")
            }
            "system/task_started" => format!("{} backgrounded={}", v["task_type"], v["is_backgrounded"]),
            "system/task_notification" => format!("status={}", v["status"]),
            "system/task_updated" => format!("patch.status={}", v["patch"]["status"]),
            "system/init" => String::new(),
            k if k.starts_with("result") => {
                results += 1;
                format!("origin={} is_error={}", v["origin"]["kind"], v["is_error"])
            }
            _ => continue,
        };
        let idle = state == "idle" && bg == 0;
        println!(
            "{t:7.3} {k} {note}{}",
            if idle { "   <= idle without background" } else { "" }
        );
    }
    println!("results: {results}; final state={state}, background tasks={bg}");
    c.close_stdin();
    let _ = c.child.wait();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["orphans"] => orphans(false),
        ["orphans", "subreaper"] => orphans(true),
        ["background"] => background(),
        _ => eprintln!("usage: claude-process orphans [subreaper] | background"),
    }
}
