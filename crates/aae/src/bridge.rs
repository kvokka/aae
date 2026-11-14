//! `aae mcp-bridge -- <command…>` (spec §11.4): one stdio MCP server shared over HTTP. The bridge starts the server
//! as its child, initializes it once, and only then binds `127.0.0.1:$AAE_SERVICE_PORT`, so "the port accepts" means
//! "the server is ready". `POST /mcp` speaks both protocol epochs as the API's MCP frontend does, with no session
//! id; every request but the handshake goes to the server under an id of the bridge's own, with the caller's
//! identity headers in `params._meta`. The child dying ends the bridge with an error, so the daemon restarts both.

use aae_front::api::mcp::{MODERN, SUPPORTED};
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    process::{ExitCode, Stdio},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::{ChildStdin, ChildStdout},
    sync::oneshot,
};

/// How long the server may take to answer `initialize`.
const HANDSHAKE: Duration = Duration::from_secs(25);
/// The identity headers and the `_meta` keys they become.
const IDENTITY: &[(&str, &str)] = &[
    ("x-aae-project", "aae/project"),
    ("x-aae-rig", "aae/rig"),
    ("x-aae-agent", "aae/agent"),
    ("x-aae-run", "aae/run"),
];

type Pending = Mutex<HashMap<u64, oneshot::Sender<Value>>>;

/// The bridge's side of the stdio session.
struct Bridge {
    stdin: tokio::sync::Mutex<ChildStdin>,
    pending: Arc<Pending>,
    next: AtomicU64,
    /// `Bearer $AAE_SERVICE_TOKEN`.
    bearer: String,
    /// The server's `initialize` result.
    init: Value,
}

/// Runs the bridge until the server or the listener fails.
pub(crate) fn run(command: Vec<String>) -> ExitCode {
    let var = |name| std::env::var(name).map_err(|e| format!("{name}: {e}"));
    let served = var("AAE_SERVICE_PORT")
        .and_then(|port| port.parse::<u16>().map_err(|e| format!("AAE_SERVICE_PORT: {e}")))
        .and_then(|port| Ok((port, var("AAE_SERVICE_TOKEN")?)))
        .and_then(|(port, token)| {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            runtime
                .map_err(|e| format!("runtime: {e}"))?
                .block_on(serve(&command, port, &token))
        });
    let Err(e) = served;
    eprintln!("aae mcp-bridge: {e}");
    ExitCode::FAILURE
}

async fn serve(command: &[String], port: u16, token: &str) -> Result<std::convert::Infallible, String> {
    let (program, args) = command.split_first().ok_or("no command")?;
    // The environment goes to the child whole: its `AAE_INSTANCE` marker lets a sweep find it as well.
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err("no pipes to the server".into());
    };
    let pending = Arc::new(Pending::default());
    let mut reader = tokio::spawn(read(stdout, pending.clone()));
    let mut bridge = Bridge {
        stdin: tokio::sync::Mutex::new(stdin),
        pending,
        next: AtomicU64::new(0),
        bearer: format!("Bearer {token}"),
        init: Value::Null,
    };
    let params = json!({ "protocolVersion": SUPPORTED[0], "capabilities": {},
        "clientInfo": { "name": "aae-mcp-bridge", "version": env!("CARGO_PKG_VERSION") } });
    let reply = tokio::select! {
        reply = tokio::time::timeout(HANDSHAKE, bridge.call("initialize", params)) => {
            reply.map_err(|_| format!("no answer to `initialize` in {}s", HANDSHAKE.as_secs()))??
        }
        _ = &mut reader => return Err(gone(&mut child).await),
    };
    bridge.init = match reply.get("result") {
        Some(result) => result.clone(),
        None => return Err(format!("`initialize` failed: {}", reply["error"])),
    };
    bridge
        .send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("port {port}: {e}"))?;
    let app = Router::new()
        .route(
            "/mcp",
            post(handle).get(async || (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")])),
        )
        .with_state(Arc::new(bridge));
    tokio::select! {
        served = axum::serve(listener, app) => Err(format!("listener: {}", served.err().map(|e| e.to_string()).unwrap_or_default())),
        _ = reader => Err(gone(&mut child).await),
    }
}

/// Why the server is gone, once its stdout closed.
async fn gone(child: &mut tokio::process::Child) -> String {
    match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
        Ok(Ok(status)) => format!("the server exited: {status}"),
        _ => "the server closed its stdout".into(),
    }
}

/// Hands each response of the server to the request waiting on its id, until the server's stdout closes.
async fn read(stdout: ChildStdout, pending: Arc<Pending>) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            eprintln!("aae mcp-bridge: not a JSON-RPC message from the server: {line}");
            continue;
        };
        // shortcut: requests and notifications of the server to the client are dropped (the bridge declares no client
        // capabilities, so a conforming server sends none); forward them once a shared server needs one.
        if msg.get("method").is_some() {
            continue;
        }
        let waiting = msg["id"].as_u64().and_then(|id| lock(&pending).remove(&id));
        if let Some(tx) = waiting {
            let _ = tx.send(msg);
        }
    }
}

fn lock(pending: &Pending) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<Value>>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Bridge {
    /// One line to the server's stdin.
    async fn send(&self, msg: &Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        let line = format!("{msg}\n");
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("the server's stdin: {e}"))?;
        stdin.flush().await.map_err(|e| format!("the server's stdin: {e}"))
    }

    /// A request to the server under the bridge's next id; the server's whole response.
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        let sent = self
            .send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        if let Err(e) = sent {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        rx.await.map_err(|_| "the server is gone".to_owned())
    }
}

/// `POST /mcp`: one JSON-RPC message; a notification or a response gets 202 and does not reach the server.
async fn handle(State(bridge): State<Arc<Bridge>>, headers: HeaderMap, body: Bytes) -> Response {
    if !headers
        .get(header::AUTHORIZATION)
        .is_some_and(|v| v == bridge.bearer.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let modern = headers.get("mcp-protocol-version").is_some_and(|v| v == MODERN);
    let Ok(Value::Object(mut msg)) = serde_json::from_slice::<Value>(&body) else {
        return reply(
            StatusCode::BAD_REQUEST,
            rpc_error(&Value::Null, -32700, "not one JSON-RPC message"),
        );
    };
    let (Some(id), Some(method)) = (msg.remove("id"), msg.get("method").and_then(Value::as_str)) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let init = &bridge.init;
    let result = match method {
        "server/discover" if modern => {
            json!({ "supportedVersions": [MODERN], "capabilities": init["capabilities"] })
        }
        // The server is initialized once, by the bridge; every client's handshake is answered from that one.
        "initialize" => {
            let asked = msg["params"]["protocolVersion"].as_str().unwrap_or_default();
            let version = SUPPORTED.iter().find(|v| **v == asked).unwrap_or(&SUPPORTED[0]);
            let mut result = json!({ "protocolVersion": version, "capabilities": init["capabilities"],
                "serverInfo": init["serverInfo"] });
            if let Some(instructions) = init.get("instructions") {
                result["instructions"] = instructions.clone();
            }
            result
        }
        _ => {
            let method = method.to_owned();
            let mut params = msg.remove("params").unwrap_or_else(|| json!({}));
            if let Some(params) = params.as_object_mut() {
                let meta = params.entry("_meta").or_insert_with(|| json!({}));
                for (name, key) in IDENTITY {
                    if let (Some(meta), Some(value)) = (meta.as_object_mut(), headers.get(*name)) {
                        meta.insert((*key).into(), json!(value.to_str().unwrap_or_default()));
                    }
                }
            }
            let mut response = match bridge.call(&method, params).await {
                Ok(response) => response,
                Err(e) => return reply(StatusCode::OK, rpc_error(&id, -32603, &e)),
            };
            response["id"] = id;
            if modern && let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) {
                result.entry("resultType").or_insert_with(|| json!("complete"));
            }
            return reply(StatusCode::OK, response);
        }
    };
    reply(StatusCode::OK, json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn reply(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}
