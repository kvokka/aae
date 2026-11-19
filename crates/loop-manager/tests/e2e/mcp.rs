//! The MCP frontend (docs/api/frontends.md, criterion 17): five tools on the daemon's socket, in both protocol epochs
//! (docs/design/config.md).

use crate::common::Daemon;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// One HTTP/1.1 exchange on `/mcp`: the status and the JSON body (null when empty).
async fn http(d: &Daemon, method: &str, modern: bool, body: Option<&Value>) -> (u16, Value) {
    let mut stream = tokio::net::UnixStream::connect(d.socket()).await.unwrap();
    let body = body.map(Value::to_string).unwrap_or_default();
    let version = if modern {
        "mcp-protocol-version: 2026-07-28\r\n"
    } else {
        ""
    };
    let request = format!(
        "{method} /mcp HTTP/1.1\r\nhost: loop-manager\r\nconnection: close\r\ncontent-type: application/json\r\n\
         accept: application/json, text/event-stream\r\n{version}content-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head[9..12].parse().unwrap(),
        serde_json::from_str(body).unwrap_or(Value::Null),
    )
}

async fn rpc(d: &Daemon, modern: bool, method: &str, params: Value) -> Value {
    let message = json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params });
    let (status, reply) = http(d, "POST", modern, Some(&message)).await;
    assert_eq!((status, &reply["id"]), (200, &json!(7)), "{reply}");
    reply
}

/// A tool call in the `initialize` epoch: its text, and whether it is an error.
async fn call(d: &Daemon, tool: &str, arguments: Value) -> (String, bool) {
    let reply = rpc(d, false, "tools/call", json!({ "name": tool, "arguments": arguments })).await;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap().to_owned();
    (text, reply["result"]["isError"].as_bool().unwrap())
}

/// The value of the top-level field `key` of a TOON object: its `key: value` line.
fn field<'a>(toon: &'a str, key: &str) -> &'a str {
    let line = toon.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix(": "));
    line.unwrap_or_else(|| panic!("no field {key} in {toon}"))
}

fn names(reply: &Value) -> Vec<&str> {
    let tools = reply["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    tools.iter().map(|t| t["name"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn both_epochs_serve_the_same_five_tools() {
    let d = Daemon::start(|_| {}).await;
    let five = ["list_formulas", "start_run", "run_state", "list_runs", "cancel_run"];

    let discover = rpc(&d, true, "server/discover", json!({})).await;
    assert_eq!(discover["result"]["supportedVersions"], json!(["2026-07-28"]));
    assert_eq!(discover["result"]["resultType"], "complete");
    assert_eq!(names(&rpc(&d, true, "tools/list", json!({})).await), five);

    // The fallback of claude: `server/discover` without the stateless epoch is an unknown method.
    assert_eq!(
        rpc(&d, false, "server/discover", json!({})).await["error"]["code"],
        -32601
    );
    let init = rpc(
        &d,
        false,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
    )
    .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "loop-manager");
    let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert_eq!(http(&d, "POST", false, Some(&initialized)).await.0, 202);
    assert_eq!(names(&rpc(&d, false, "tools/list", json!({})).await), five);
    assert_eq!(http(&d, "GET", false, None).await.0, 405);
    let unknown = rpc(&d, false, "tools/call", json!({ "name": "nope", "arguments": {} })).await;
    assert_eq!(unknown["error"]["code"], -32602);
}

#[tokio::test]
async fn a_run_started_over_mcp_is_followed_listed_and_cancelled_there() {
    let d = Daemon::start(|loop_manager| {
        let inner = "name = \"inner\"\ninternal = true\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n";
        std::fs::write(loop_manager.join("config/loop-manager/shop/formulas/inner.toml"), inner).unwrap();
    })
    .await;
    let (formulas, _) = call(&d, "list_formulas", json!({ "project": "shop" })).await;
    // The answer an agent reads is TOON, not JSON (docs/api/mcp.md); an internal formula is no part of the catalog a
    // user starts from (docs/api/frontends.md).
    assert!(serde_json::from_str::<Value>(&formulas).is_err(), "{formulas}");
    assert!(
        formulas.contains("formula: check") && !formulas.contains("inner"),
        "{formulas}"
    );
    let n = "      n:\n        required: false\n        type: integer";
    assert!(formulas.contains(n), "{formulas}");

    let (started, error) = call(
        &d,
        "start_run",
        json!({ "project": "shop", "rig": "web", "formula": "slow" }),
    )
    .await;
    assert!(!error, "{started}");
    let run = field(&started, "run").to_owned();
    let state = |want: &'static str| {
        let (d, run) = (&d, run.clone());
        async move {
            for _ in 0..100 {
                let (view, _) = call(d, "run_state", json!({ "id": run })).await;
                if field(&view, "state") == want {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("run never {want}");
        }
    };
    state("running").await;
    let (runs, _) = call(&d, "list_runs", json!({ "project": "shop" })).await;
    assert!(
        runs.starts_with("[1]") && runs.contains(&format!("\n    id: {run}\n")),
        "{runs}"
    );
    let (_, error) = call(&d, "cancel_run", json!({ "id": run, "reason": "enough" })).await;
    assert!(!error);
    state("cancelled").await;
    // The stateless epoch calls the same tools and marks its results.
    let params = json!({ "name": "run_state", "arguments": { "id": run } });
    let modern = rpc(&d, true, "tools/call", params).await["result"].clone();
    assert_eq!(modern["resultType"], "complete");
    assert_eq!(
        modern["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "loop-manager"
    );
    assert_eq!(
        field(modern["content"][0]["text"].as_str().unwrap(), "state"),
        "cancelled"
    );

    let (text, error) = call(
        &d,
        "start_run",
        json!({ "project": "shop", "rig": "web", "formula": "slow",
        "continue_from": "nope" }),
    )
    .await;
    assert!(error && text.contains("it has no session to continue"), "{text}");
    let (text, error) = call(&d, "start_run", json!({ "project": "shop" })).await;
    assert!(error && text.starts_with("bad_request"), "{text}");
    let keyed = |formula| json!({ "project": "shop", "rig": "web", "formula": formula, "idempotency_key": "k-1" });
    let (first, error) = call(&d, "start_run", keyed("check")).await;
    assert!(!error, "{first}");
    assert_eq!(
        call(&d, "start_run", keyed("check")).await,
        (first, false),
        "the first result"
    );
    let (text, error) = call(&d, "start_run", keyed("slow")).await;
    assert!(error && text.starts_with("idempotency_key_reused"), "{text}");
    let (text, error) = call(&d, "run_state", json!({ "id": "nope" })).await;
    assert!(error && text.starts_with("not_found"), "{text}");
}
