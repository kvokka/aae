//! An `await` step from the CLI against a real `loop-manager daemon` (docs/design/config.md, docs/api/frontends.md): the run
//! waits until an outcome is given from outside.

use crate::common::{self, Daemon};
use serde_json::{Value, json};
use std::{fs, process::Output};

fn cli(d: &Daemon, args: &[&str]) -> Output {
    common::loop_manager()
        .args(args)
        .env("LOOP_MANAGER_SOCKET", d.socket())
        .env("HOME", d.dir.path())
        .env("XDG_CONFIG_HOME", d.dir.path().join("config"))
        .output()
        .unwrap()
}

fn ok(d: &Daemon, args: &[&str]) -> Value {
    let out = cli(d, &[args, &["--json"]].concat());
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The code of the JSON error a failed `--json` command prints, with its exit code.
fn error(d: &Daemon, args: &[&str]) -> (Option<i32>, Value) {
    let out = cli(d, &[args, &["--json"]].concat());
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    (out.status.code(), error["error"]["code"].clone())
}

fn awaiting(d: &Daemon) -> Vec<Value> {
    let runs = ok(d, &["run", "list", "--awaiting"]);
    runs.as_array().unwrap().iter().map(|r| r["id"].clone()).collect()
}

#[tokio::test]
async fn an_outcome_from_the_cli_moves_a_waiting_run_on() {
    let d = Daemon::start(|dir| {
        let formula = "name = \"gate\"\n[[step]]\nname = \"gate\"\nkind = \"await\"\n[[step]]\nname = \"done\"\n\
                       kind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n";
        fs::write(dir.join("config/loop-manager/shop/formulas/gate.toml"), formula).unwrap();
    })
    .await;
    let run = ok(&d, &["run", "start", "shop", "web", "gate"])["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let slow = ok(&d, &["run", "start", "shop", "web", "slow"])["run"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(d.until_state(&run, "running").await["step"], "gate");
    d.until_state(&slow, "running").await;
    assert_eq!(awaiting(&d), [json!(run)], "only the run waiting on `await`");
    let text = String::from_utf8(cli(&d, &["run", "list", "--awaiting"]).stdout).unwrap();
    assert_eq!(
        text.trim_end(),
        loop_manager_core::toon::encode(&ok(&d, &["run", "list", "--awaiting"]))
    );
    // An outcome for a step the run does not wait on is refused and changes nothing.
    assert_eq!(
        error(&d, &["run", "resolve", &run, "nope", "pass"]),
        (Some(1), json!("rejected"))
    );
    assert_eq!(ok(&d, &["run", "show", &run])["state"], "running");
    assert_eq!(awaiting(&d), [json!(run)]);
    let resolve = [
        "run",
        "resolve",
        &run,
        "gate",
        "pass",
        "--summary",
        "ok",
        "--data",
        r#"{"pr":1}"#,
        "--key",
        "r1",
    ];
    assert_eq!(ok(&d, &resolve), Value::Null);
    assert_eq!(
        ok(&d, &resolve),
        Value::Null,
        "a repeat with the same key is the same reply"
    );
    d.until_state(&run, "passed").await;
    let view = ok(&d, &["run", "show", &run]);
    assert_eq!(
        view["steps"]["gate"],
        json!({ "result": "pass", "summary": "ok", "data": { "pr": 1 } }),
        "{view}"
    );
    assert!(awaiting(&d).is_empty());
    assert_eq!(
        error(&d, &["run", "resolve", "run_999", "gate", "pass"]),
        (Some(1), json!("not_found"))
    );
}

#[tokio::test]
async fn a_fail_from_the_cli_with_data_from_a_file_fails_the_run() {
    let d = Daemon::start(|dir| {
        let formula = "name = \"gate\"\n[[step]]\nname = \"gate\"\nkind = \"await\"\n";
        fs::write(dir.join("config/loop-manager/shop/formulas/gate.toml"), formula).unwrap();
    })
    .await;
    let run = ok(&d, &["run", "start", "shop", "web", "gate"])["run"]
        .as_str()
        .unwrap()
        .to_owned();
    d.until_state(&run, "running").await;
    let data = d.dir.path().join("verdict.json");
    fs::write(&data, r#"{"reason":"no"}"#).unwrap();
    let file = format!("@{}", data.display());
    let resolve = ["run", "resolve", &run, "gate", "fail", "--summary", "rejected"];
    assert_eq!(ok(&d, &[&resolve[..], &["--data", &file]].concat()), Value::Null);
    d.until_state(&run, "failed").await;
    let view = ok(&d, &["run", "show", &run]);
    assert_eq!(view["class"], "agent", "{view}");
    assert_eq!(
        view["outcome"],
        json!({ "result": "fail", "summary": "rejected", "data": { "reason": "no" } }),
        "{view}"
    );
}
