//! Minimal hand-written client for `claude -p` stream-json, shared by the claude experiments.
//! Every line in and out is written to a log as `{"t": seconds, "dir": "in"|"out", "msg": ...}`
//! with the home and work directories replaced by placeholders.

use serde_json::{Value, json};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

pub struct Claude {
    pub child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<(f64, Value)>,
    t0: Instant,
    log: File,
    work: PathBuf,
    /// Value of `AAE_EXP_TAG` in the process environment, inherited by every descendant.
    pub tag: String,
}

pub fn uuid() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/uuid")
        .expect("uuid")
        .trim()
        .to_owned()
}

/// A fresh empty work directory under the system temp dir.
pub fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aae-exp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("workdir");
    d
}

impl Claude {
    /// Starts `claude -p` in stream-json mode with host settings off and a clean environment.
    /// The process gets its own process group.
    pub fn spawn(work: &Path, log: &Path, args: &[&str], env: &[(&str, &str)]) -> Claude {
        let tag = uuid();
        let mut cmd = Command::new("claude");
        cmd.args(["-p", "--input-format", "stream-json", "--output-format", "stream-json"])
            .arg("--verbose")
            .args(args);
        // Host settings off unless the caller picks its own sources.
        if !args.contains(&"--setting-sources") {
            cmd.args(["--setting-sources", "", "--strict-mcp-config"]);
        }
        cmd.current_dir(work)
            .env_clear()
            .envs(
                ["PATH", "HOME", "LANG", "USER", "TERM"]
                    .iter()
                    .filter_map(|k| Some((k, std::env::var(k).ok()?))),
            )
            .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1")
            .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
            .env("AAE_EXP_TAG", &tag)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .process_group(0);
        let mut child = cmd.spawn().expect("spawn claude");
        let stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = channel();
        let t0 = Instant::now();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let v = serde_json::from_str(&line).unwrap_or(json!({"unparsed": line}));
                if tx.send((t0.elapsed().as_secs_f64(), v)).is_err() {
                    break;
                }
            }
        });
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir).expect("log dir");
        }
        Claude {
            stdin: child.stdin.take(),
            child,
            rx,
            t0,
            log: File::create(log).expect("log"),
            work: work.to_owned(),
            tag,
        }
    }

    pub fn t(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    fn record(&mut self, t: f64, dir: &str, msg: &Value) {
        let mut msg = msg.clone();
        if let Some(o) = msg.as_object_mut() {
            for k in ["tools", "skills", "slash_commands", "agents", "plugins"] {
                if let Some(n) = o.get(k).and_then(Value::as_array).map(Vec::len) {
                    o.insert(k.into(), json!(format!("<{n} items>")));
                }
            }
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let line = json!({"t": (t * 1000.0).round() / 1000.0, "dir": dir, "msg": msg})
            .to_string()
            .replace(self.work.to_str().unwrap_or("\0"), "<work>")
            .replace(&home, "<home>");
        writeln!(self.log, "{line}").expect("log write");
    }

    pub fn send(&mut self, msg: Value) {
        let t = self.t();
        self.record(t, "in", &msg);
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{msg}").expect("write stdin");
        stdin.flush().expect("flush");
    }

    /// Sends a user message; `uuid` is the caller's own id for the message.
    pub fn user(&mut self, text: &str, uuid: &str) {
        self.send(
            json!({"type":"user","uuid":uuid,"session_id":"","parent_tool_use_id":null,
            "message":{"role":"user","content":[{"type":"text","text":text}]}}),
        );
    }

    pub fn close_stdin(&mut self) {
        let t = self.t();
        self.record(t, "in", &json!({"eof": true}));
        self.stdin = None;
    }

    /// Next message within `timeout`; `None` on timeout or EOF.
    pub fn next(&mut self, timeout: Duration) -> Option<(f64, Value)> {
        match self.rx.recv_timeout(timeout) {
            Ok((t, v)) => {
                self.record(t, "out", &v);
                Some((t, v))
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Reads until `stop` returns true, calling `seen` on every message. Returns the stopping message.
    pub fn until(
        &mut self,
        timeout: Duration,
        mut seen: impl FnMut(&mut Self, f64, &Value),
        stop: impl Fn(&Value) -> bool,
    ) -> Option<(f64, Value)> {
        let end = Instant::now() + timeout;
        while let Some((t, v)) = self.next(end.saturating_duration_since(Instant::now())) {
            seen(self, t, &v);
            if stop(&v) {
                return Some((t, v));
            }
        }
        None
    }

    /// Kills the whole process group and reaps the leader.
    pub fn kill(&mut self) {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", self.child.id())])
            .status();
        let _ = self.child.wait();
    }
}

/// `type` or `type/subtype` of a stream message.
pub fn kind(v: &Value) -> String {
    let ty = v["type"].as_str().unwrap_or("?");
    match v["subtype"].as_str().or(v["request"]["subtype"].as_str()) {
        Some(s) => format!("{ty}/{s}"),
        None => ty.to_owned(),
    }
}

pub fn is(v: &Value, k: &str) -> bool {
    kind(v) == k
}

/// Pids whose environment carries `AAE_EXP_TAG=<tag>`: the claude process and every descendant that kept its env.
pub fn tagged(tag: &str) -> Vec<u32> {
    let needle = format!("AAE_EXP_TAG={tag}");
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .filter(|pid: &u32| {
            std::fs::read(format!("/proc/{pid}/environ"))
                .is_ok_and(|env| env.split(|b| *b == 0).any(|kv| kv == needle.as_bytes()))
        })
        .collect()
}
