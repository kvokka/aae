//! The short model names against the real codex's own list (`model/list` of `codex app-server`, no model call and no
//! account): every target of the table is a model codex knows and does not mark as replaced by a newer one.

use loop_manager_codex::ADAPTER;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

#[tokio::test]
async fn every_short_name_expands_to_a_current_codex_model() {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new("codex")
        .arg("app-server")
        .env("CODEX_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let requests = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "clientInfo": { "name": "loop-manager-test", "version": "0" }, "capabilities": { "experimentalApi": true } } }),
        json!({ "jsonrpc": "2.0", "method": "initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "model/list", "params": { "includeHidden": true } }),
    ];
    for request in requests {
        stdin.write_all(format!("{request}\n").as_bytes()).await.unwrap();
    }
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let reply = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            let message: Value = serde_json::from_str(&line).unwrap_or_default();
            if message["id"] == 2 {
                return message;
            }
        }
        panic!("codex app-server closed its output before the model list");
    })
    .await
    .expect("no model list in 30 s");
    let models = reply["result"]["data"].as_array().unwrap_or_else(|| panic!("{reply}"));
    for (short, full) in ADAPTER.models {
        let model = models.iter().find(|m| m["id"] == *full);
        let model = model.unwrap_or_else(|| panic!("`{short}` expands to `{full}`, which codex does not list"));
        assert!(
            model["upgrade"].is_null(),
            "`{short}` expands to `{full}`, which codex replaced: {model}"
        );
    }
}
