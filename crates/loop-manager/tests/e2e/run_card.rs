//! The optional chat plugin opens a card on the daemon socket and resumes from the cursor after a terminal event.

use crate::common::{self, Daemon, WAIT};
use serde_json::{Value, json};
use std::{io::Write, path::Path, process::Stdio};

fn card(d: &Daemon, name: &str, args: Value) -> Value {
    card_in(d, name, args, &d.dir.path().join("run"))
}

fn card_in(d: &Daemon, name: &str, args: Value, runtime: &Path) -> Value {
    let server = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/run-card/server.py");
    let mut child = common::program("timeout")
        .arg(WAIT.as_secs().to_string())
        .arg("python3")
        .arg(server)
        .env_remove("LOOP_MANAGER_SOCKET")
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_CONFIG_HOME", d.dir.path().join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let rpc = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}
    });
    writeln!(child.stdin.take().unwrap(), "{rpc}").unwrap();
    let reply = child.wait_with_output().unwrap();
    assert!(reply.status.success());
    let payload: Value = serde_json::from_slice(&reply.stdout).unwrap();
    assert!(payload["result"]["structuredContent"].is_object(), "{payload}");
    payload["result"]["structuredContent"].clone()
}

#[tokio::test]
async fn the_card_catches_up_after_a_disconnected_client_returns() {
    let d = Daemon::start(|_| {}).await;
    let run = d.create("slow", None).await;
    d.until_state(&run, "running").await;
    let opened = card(&d, "show_run_card", json!({ "run": run, "socket": d.socket() }));
    assert_eq!(opened["socket"], json!(d.socket()));
    assert_eq!(opened["card"]["rounds"], 3);
    assert_eq!(opened["terminal"], false);
    assert_eq!(opened["plan"][0]["kind"], "exec");
    d.post(&format!("/v1/runs/{run}/cancel"), json!({"reason": "done"}), None)
        .await
        .unwrap();
    d.until_state(&run, "cancelled").await;
    let changed = card(&d, "watch_run_card", json!({ "run": run, "cursor": opened["cursor"] }));
    assert_eq!(changed["card"]["state"], "cancelled");
    assert_eq!(changed["terminal"], true);
}

#[tokio::test]
async fn a_card_keeps_the_summaries_of_the_previous_and_final_steps() {
    let d = Daemon::start(|dir| {
        let formula = r#"
name = "handoff"
[[step]]
name = "review"
kind = "await"
[[step]]
name = "publish"
kind = "exec"
command = ["true"]
"#;
        std::fs::write(dir.join("config/loop-manager/shop/formulas/handoff.toml"), formula).unwrap();
    })
    .await;
    let run = d.create("handoff", None).await;
    d.until_state(&run, "running").await;
    let args = json!({"run": run});
    let open = card(&d, "show_run_card", args.clone());
    assert_eq!(open["card"]["summaries"], json!({}));
    d.post(
        &format!("/v1/runs/{run}/resolve"),
        json!({"step": "review", "outcome": {"result": "pass", "summary": "Ready for release"}}),
        None,
    )
    .await
    .unwrap();
    d.until_state(&run, "passed").await;
    let done = card(&d, "show_run_card", args);
    assert_eq!(
        done["card"]["summaries"],
        json!({"publish": "exit 0", "review": "Ready for release"})
    );
}

/// Different daemons may both have a run_1. Follow-up reads must stay on the selected daemon.
#[tokio::test]
async fn the_card_selects_the_default_named_or_sole_registered_socket() {
    let local = Daemon::start(|_| {}).await;
    let other = Daemon::start(|_| {}).await;
    let run = local.create("check", None).await;
    assert_eq!(other.create("slow", None).await, run);
    let registry = local.dir.path().join("config/loop-manager/sockets");
    std::fs::create_dir_all(&registry).unwrap();
    std::os::unix::fs::symlink(other.socket(), registry.join("work")).unwrap();
    let args = json!({"run": run});
    let default = card(&local, "show_run_card", args.clone());
    assert_eq!(default["socket"], json!(local.socket()));
    let named = card(&local, "show_run_card", json!({"run": run, "daemon": "work"}));
    let fallback = card_in(&local, "show_run_card", args, &local.dir.path().join("no-default"));
    for selected in [named, fallback] {
        assert_eq!(selected["socket"], json!(other.socket()));
        assert_eq!(selected["card"]["formula"], "shop.slow");
    }
}
