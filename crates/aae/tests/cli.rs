//! The CLI against a real `aae daemon` (spec §12): a thin client with JSON output.

mod common;

use common::Daemon;
use serde_json::Value;
use std::process::{Command, Output};

impl Daemon {
    /// `aae <args>` on this daemon's socket.
    fn cli(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aae"));
        command
            .args(args)
            .env("AAE_SOCKET", self.socket())
            .envs(env.iter().copied());
        command.output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.cli(&[args, &["--json"]].concat(), &[]);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

#[tokio::test]
async fn runs_and_gates_by_hand_from_the_cli() {
    let d = Daemon::start(|_| {}).await;
    d.json(&["gate", "close", "project:shop", "--reason", "pause"]);
    assert_eq!(d.json(&["gate", "list"])[0]["key"], "project:shop");
    let out = d.cli(
        &["run", "start", "shop", "web", "check", "--key", "k1", "--input", "n=2"],
        &[],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let run = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert_eq!(
        d.json(&["run", "start", "shop", "web", "check", "--key", "k1"])["run"],
        run.as_str()
    );
    let view = d.json(&["run", "show", &run]);
    assert_eq!((&view["state"], &view["inputs"]["n"]), (&"queued".into(), &2.into()));
    d.json(&["gate", "open", "project:shop"]);
    d.until_state(&run, "passed").await;
    assert_eq!(d.json(&["run", "list", "--state", "passed"])[0]["id"], run.as_str());
    assert_eq!(d.json(&["status"])["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn the_finisher_reports_the_answer_and_needs_the_session_environment() {
    let d = Daemon::start(|_| {}).await;
    let out = d.cli(&["finish", "pass", "--summary", "done"], &[]);
    assert!(!out.status.success());
    let env = [("AAE_RUN", "nope"), ("AAE_ATTEMPT_TOKEN", "bad")];
    let out = d.cli(&["finish", "pass", "--summary", "done", "--data", "{\"n\":1}"], &env);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "invalid");
    assert_eq!(out.status.code(), Some(1));
}

#[tokio::test]
async fn without_a_daemon_a_client_command_fails_and_names_the_daemon() {
    let d = Daemon::start(|_| {}).await;
    let out = d.cli(&["status"], &[("AAE_SOCKET", "/nonexistent/aae.sock")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("aae daemon"));
}
