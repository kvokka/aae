//! A test HTTP MCP service (spec §15 criterion 14): `POST /mcp` in both protocol epochs, one tool [`TOOL`] that
//! answers with the identity headers of the call. Every request must carry `Authorization: Bearer <token>` (401
//! otherwise) and appends one JSON line to the log: the server's pid, the JSON-RPC method, the tool called, the
//! `X-Aae-*` headers and whether the token was right.

use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::Write as _,
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::net::TcpListener;

/// The one tool of the service.
pub const TOOL: &str = "whoami";
/// The stateless epoch, chosen by the request header.
const MODERN: &str = "2026-07-28";
/// The `initialize` epoch versions answered as asked; any other gets the first.
const SUPPORTED: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const IDENTITY: &[&str] = &["x-aae-project", "x-aae-rig", "x-aae-agent", "x-aae-run"];

/// Serves as aae starts a service: on `AAE_SERVICE_PORT` of 127.0.0.1 with `AAE_SERVICE_TOKEN`, until its parent
/// dies.
///
/// # Errors
/// If a variable is missing, the port cannot be bound or the server fails.
pub async fn serve_service(log: &Path) -> std::io::Result<()> {
    let var = |name| std::env::var(name).map_err(|e| std::io::Error::other(format!("{name}: {e}")));
    let port: u16 = var("AAE_SERVICE_PORT")?.parse().map_err(std::io::Error::other)?;
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    // A daemon killed with SIGKILL leaves its services to the sweep of its next start; a test leaves none behind.
    let parent = std::os::unix::process::parent_id();
    let orphaned = async {
        while std::os::unix::process::parent_id() == parent {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    };
    let token = var("AAE_SERVICE_TOKEN")?;
    tokio::select! {
        served = serve(listener, &token, log) => served,
        () = orphaned => Ok(()),
    }
}

/// Serves on `listener` until the task is dropped, appending to `log` (created if absent).
///
/// # Errors
/// If the log cannot be opened or the server fails.
pub async fn serve(listener: TcpListener, token: &str, log: &Path) -> std::io::Result<()> {
    let log = Arc::new(Mutex::new(File::options().create(true).append(true).open(log)?));
    let bearer = Arc::new(format!("Bearer {token}"));
    let app = Router::new().fallback(move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
        let (log, bearer) = (log.clone(), bearer.clone());
        async move {
            if uri.path() != "/mcp" {
                return respond(StatusCode::NOT_FOUND, None);
            }
            let msg: Value = serde_json::from_slice(&body).unwrap_or_default();
            let authorized = headers.get("authorization").is_some_and(|v| v == bearer.as_str());
            let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
            let mut line = json!({ "pid": std::process::id(), "http": method.as_str(), "method": msg["method"],
                "tool": msg["params"]["name"], "authorized": authorized });
            for name in IDENTITY {
                line[name.trim_start_matches("x-aae-")] = json!(header(name));
            }
            let mut file = log.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(file, "{line}");
            drop(file);
            if !authorized {
                return respond(StatusCode::UNAUTHORIZED, None);
            }
            if method != Method::POST {
                return respond(StatusCode::METHOD_NOT_ALLOWED, None);
            }
            let modern = header("mcp-protocol-version").as_deref() == Some(MODERN);
            match answer(&msg, &line, modern) {
                Some(body) => respond(StatusCode::OK, Some(body)),
                None => respond(StatusCode::ACCEPTED, None),
            }
        }
    });
    axum::serve(listener, app).await
}

/// The JSON-RPC answer to `msg`; `None` for a notification or a response.
fn answer(msg: &Value, line: &Value, modern: bool) -> Option<Value> {
    let (id, method) = (msg.get("id")?, msg["method"].as_str()?);
    let info = json!({ "name": "aae-mcp-probe", "version": "1" });
    let mut result = match method {
        "server/discover" if modern => json!({ "supportedVersions": [MODERN], "capabilities": { "tools": {} } }),
        "initialize" if !modern => {
            let asked = msg["params"]["protocolVersion"].as_str().unwrap_or_default();
            let version = SUPPORTED.iter().find(|v| **v == asked).unwrap_or(&SUPPORTED[0]);
            json!({ "protocolVersion": version, "capabilities": { "tools": {} }, "serverInfo": info })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": [{ "name": TOOL, "description": "Who is calling: the identity headers.",
            "inputSchema": { "type": "object", "properties": {} } }] }),
        "tools/call" if msg["params"]["name"] == TOOL => {
            let who = json!({ "project": line["project"], "rig": line["rig"], "agent": line["agent"],
                "run": line["run"] });
            json!({ "content": [{ "type": "text", "text": who.to_string() }], "isError": false })
        }
        _ => {
            let error = json!({ "code": -32601, "message": format!("no method or tool for `{method}`") });
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": error }));
        }
    };
    if modern {
        result["resultType"] = json!("complete");
        result["_meta"] = json!({ "io.modelcontextprotocol/serverInfo": info });
    }
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn respond(status: StatusCode, body: Option<Value>) -> Response {
    let builder = Response::builder().status(status);
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a valid response")
}
