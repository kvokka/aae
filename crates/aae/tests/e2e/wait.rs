//! `aae run wait` and `aae run start` from a session against a real `aae daemon` (docs/api/frontends.md): the outcomes
//! on the event stream, the exit codes, a restart of the daemon in between; the session's attempt token checked.

use crate::common::{Daemon, WAIT};
use serde_json::Value;
use std::{
    fs,
    process::{Command, Output, Stdio},
};

/// `nap` sleeps 2 s; `broken` fails at once.
fn formulas(dir: &std::path::Path) {
    let formulas = dir.join("config/aae/shop/formulas");
    let nap = "name = \"nap\"\n[[step]]\nkind = \"exec\"\ncommand = [\"sleep\", \"2\"]\ntimeout = \"60s\"\n";
    fs::write(formulas.join("nap.toml"), nap).unwrap();
    let broken = "name = \"broken\"\n[[step]]\nkind = \"exec\"\ncommand = [\"false\"]\ntimeout = \"10s\"\n";
    fs::write(formulas.join("broken.toml"), broken).unwrap();
}

fn aae(d: &Daemon, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aae"));
    command
        .args(args)
        .env("AAE_SOCKET", d.socket())
        .env_remove("AAE_RUN")
        .env_remove("AAE_ATTEMPT_TOKEN");
    command
}

fn waited(out: &Output) -> (Option<i32>, Value) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value =
        serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{stdout} {}", String::from_utf8_lossy(&out.stderr)));
    (out.status.code(), value)
}

#[tokio::test]
async fn wait_prints_the_outcomes_and_exits_zero_only_when_all_passed() {
    let d = Daemon::start(formulas).await;
    let passed = d.create("check", None).await;
    let failed = d.create("broken", None).await;
    let out = aae(&d, &["run", "wait", &passed, "--json"]).output().unwrap();
    let (code, all) = waited(&out);
    assert_eq!(
        (code, &all[0]["id"], &all[0]["state"]),
        (Some(0), &passed.as_str().into(), &"passed".into())
    );
    let out = aae(&d, &["run", "wait", &passed, &failed, "--json"]).output().unwrap();
    let (code, all) = waited(&out);
    assert_eq!(code, Some(1));
    assert_eq!(
        (&all[1]["state"], &all[1]["class"]),
        (&"failed".into(), &"agent".into()),
        "{all}"
    );
    let out = aae(&d, &["run", "wait", &passed]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(&format!("id: {passed}")) && text.contains("state: passed"),
        "{text}"
    );
    let out = aae(&d, &["run", "wait", "run_999"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[tokio::test]
async fn wait_runs_out_of_time_with_its_own_code_and_survives_a_restart() {
    let mut d = Daemon::start(formulas).await;
    let slow = d.create("slow", None).await;
    let out = aae(&d, &["run", "wait", &slow, "--timeout", "1s", "--json"])
        .output()
        .unwrap();
    let (code, all) = waited(&out);
    assert_eq!((code, &all[0]["state"]), (Some(124), &"running".into()));
    d.post(&format!("/v1/runs/{slow}/cancel"), serde_json::json!({}), None)
        .await
        .unwrap();
    let nap = d.create("nap", None).await;
    let timeout = format!("{}s", WAIT.as_secs());
    let wait = aae(&d, &["run", "wait", &nap, "--timeout", &timeout, "--json"]);
    let wait = { wait }.stdout(Stdio::piped()).spawn().unwrap();
    d.until_state(&nap, "running").await;
    d.terminate();
    d.child = crate::common::spawn(d.dir.path());
    let (code, all) = waited(&wait.wait_with_output().unwrap());
    assert!(matches!(code, Some(0 | 1)), "{code:?} {all}");
    assert!(
        ["passed", "failed", "cancelled"].contains(&all[0]["state"].as_str().unwrap()),
        "{all}"
    );
}

#[tokio::test]
async fn a_start_in_a_session_carries_its_attempt_and_a_stale_one_is_refused() {
    let d = Daemon::start(|_| {}).await;
    let parent = d.create("check", None).await;
    d.until_state(&parent, "passed").await;
    let start = ["run", "start", "shop", "web", "check"];
    let out = aae(&d, &start)
        .env("AAE_RUN", &parent)
        .env("AAE_ATTEMPT_TOKEN", "old")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.code() == Some(1) && text.contains("stale attempt"), "{text}");
    let out = aae(&d, &start).output().unwrap();
    let child = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let view = d.get(&format!("/v1/runs/{child}")).await;
    assert_eq!((&view["depth"], &view["parent_run"]), (&0.into(), &Value::Null));
    let out = aae(&d, &["run", "list", "--parent", &parent, "--json"])
        .output()
        .unwrap();
    assert_eq!(waited(&out).1, serde_json::json!([]));
}
