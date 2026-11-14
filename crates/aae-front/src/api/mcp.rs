//! The MCP frontend (spec §12): five tools over the API's own handlers on `POST /mcp`, in both protocol epochs
//! (spec §11.4). A request with `MCP-Protocol-Version: 2026-07-28` is served statelessly (`server/discover`); any
//! other takes the `initialize` epoch, with no session id, so a daemon restart strands no client.

use super::{
    ApiError, Body, State, request_hash,
    runs::{self, CancelRun, CreateRun, ListQuery},
};
use aae_core::config::{Config, InputType};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};

/// The stateless epoch: chosen by the request header, never negotiated.
pub const MODERN: &str = "2026-07-28";
/// The `initialize` epoch versions answered as asked; any other gets the first.
pub const SUPPORTED: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// The tools: `(name, description, input schema)`.
fn tools() -> Value {
    let id = json!({ "type": "string", "description": "The run id." });
    let text = json!({ "type": "string" });
    let key = json!({ "type": "string", "description": "A repeat with the same key and arguments returns the first \
        result and changes nothing; with other arguments it is an error." });
    json!([
        { "name": "list_formulas", "description": "The formulas a run can start, by project, with their inputs.",
          "inputSchema": { "type": "object", "properties": { "project": text } } },
        { "name": "start_run", "description": "Starts a run of a formula on a rig; returns its id.",
          "inputSchema": { "type": "object", "required": ["project", "rig", "formula"], "properties": {
              "project": text, "rig": text,
              "formula": { "type": "string", "description": "`name`, or `<pack>.<name>`, as list_formulas gives it." },
              "inputs": { "type": "object" },
              "continue_from": { "type": "string", "description": "A terminal run whose last session to continue." },
              "idempotency_key": key } } },
        { "name": "run_state", "description": "The state of a run: its view, where it waits, its attempts.",
          "inputSchema": { "type": "object", "required": ["id"], "properties": { "id": id } } },
        { "name": "list_runs", "description": "Runs, newest first.",
          "inputSchema": { "type": "object", "properties": { "project": text, "state": text,
              "limit": { "type": "integer", "minimum": 1 } } } },
        { "name": "cancel_run", "description": "Cancels a run.",
          "inputSchema": { "type": "object", "required": ["id"], "properties": { "id": id, "reason": text,
              "idempotency_key": key } } },
    ])
}

/// `GET /mcp`: no server-initiated stream (allowed in both epochs).
pub(super) async fn get() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

/// `POST /mcp`: one JSON-RPC message; a notification or a response gets 202 and no body.
pub(super) async fn post(state: State, headers: HeaderMap, body: Bytes) -> Response {
    let modern = headers.get("mcp-protocol-version").is_some_and(|v| v == MODERN);
    let Ok(Value::Object(msg)) = serde_json::from_slice::<Value>(&body) else {
        return reply(
            StatusCode::BAD_REQUEST,
            rpc_error(&Value::Null, -32700, "not one JSON-RPC message"),
        );
    };
    let (Some(id), Some(method)) = (msg.get("id"), msg.get("method").and_then(Value::as_str)) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let params = msg.get("params").cloned().unwrap_or_default();
    let result = match method {
        "server/discover" if modern => json!({ "supportedVersions": [MODERN], "capabilities": { "tools": {} } }),
        "initialize" if !modern => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = SUPPORTED.iter().find(|v| **v == asked).unwrap_or(&SUPPORTED[0]);
            json!({ "protocolVersion": version, "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": server_info() })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => match call(state, &params).await {
            Some(result) => result,
            None => {
                let message = format!("no tool `{}`", params["name"]);
                return reply(StatusCode::OK, rpc_error(id, -32602, &message));
            }
        },
        _ => return reply(StatusCode::OK, rpc_error(id, -32601, &format!("no method `{method}`"))),
    };
    let result = match result {
        Value::Object(mut result) if modern => {
            result.insert("resultType".into(), json!("complete"));
            result.insert(
                "_meta".into(),
                json!({ "io.modelcontextprotocol/serverInfo": server_info() }),
            );
            Value::Object(result)
        }
        result => result,
    };
    reply(StatusCode::OK, json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn server_info() -> Value {
    json!({ "name": "aae", "version": env!("CARGO_PKG_VERSION") })
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn reply(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// A tool result; `None` for an unknown tool. Bad arguments and API errors are results with `isError`.
async fn call(state: State, params: &Value) -> Option<Value> {
    let mut args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let parse = |e: serde_json::Error| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", e.to_string());
    let headers = keyed(&mut args, params["name"].as_str()?);
    let id = || {
        args["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", "`id` is required"))
    };
    let reply = match params["name"].as_str()? {
        "list_formulas" => Ok(Json(formulas(&state.config(), args["project"].as_str()))),
        "start_run" => match (headers, serde_json::from_value::<CreateRun>(args.clone())) {
            (Ok(headers), Ok(body)) => runs::create(state, headers, Body(body)).await,
            (Err(e), _) => Err(e),
            (_, Err(e)) => Err(parse(e)),
        },
        "run_state" => match id() {
            Ok(id) => runs::get(state, Path(id)).await,
            Err(e) => Err(e),
        },
        "list_runs" => match serde_json::from_value::<ListQuery>(args.clone()) {
            Ok(query) => runs::list(state, Query(query)).await,
            Err(e) => Err(parse(e)),
        },
        "cancel_run" => match (id(), headers, serde_json::from_value::<CancelRun>(args.clone())) {
            (Ok(id), Ok(headers), Ok(body)) => runs::cancel(state, headers, Path(id), Body(body)).await,
            (Err(e), ..) | (_, Err(e), _) => Err(e),
            (.., Err(e)) => Err(parse(e)),
        },
        _ => return None,
    };
    Some(match reply {
        Ok(Json(value)) => json!({ "content": [{ "type": "text", "text": value.to_string() }], "isError": false }),
        Err(e) => {
            let text = format!("{}: {}", e.code, e.message);
            json!({ "content": [{ "type": "text", "text": text }], "isError": true })
        }
    })
}

/// The headers of a call to `tool` whose `args` carry an `idempotency_key` (taken out of them): what
/// [`super::fingerprint`] makes of an HTTP request.
fn keyed(args: &mut Value, tool: &str) -> Result<HeaderMap, ApiError> {
    let mut headers = HeaderMap::new();
    if let Some(key) = args.as_object_mut().and_then(|args| args.remove("idempotency_key")) {
        let key = key.as_str().and_then(|key| HeaderValue::from_str(key).ok());
        let bad = || {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "`idempotency_key` is not a header value",
            )
        };
        headers.insert("idempotency-key", key.ok_or_else(bad)?);
        headers.insert("aae-request", request_hash(format!("{tool} {args}").into_bytes()));
    }
    Ok(headers)
}

/// Every formula of every project (or of `project`), named as a run start takes it.
pub(super) fn formulas(config: &Config, project: Option<&str>) -> Value {
    let mut out = Vec::new();
    for p in config.projects.iter().filter(|p| project.is_none_or(|n| n == p.name)) {
        for pack in &p.packs {
            for (name, formula) in &pack.formulas {
                let inputs: Map<String, Value> = formula
                    .inputs
                    .iter()
                    .map(|(n, i)| (n.clone(), json!({ "type": input_type(i.kind), "required": i.required })))
                    .collect();
                // The project's own pack has no version: its formulas go by their bare name.
                let formula_ref = match pack.version {
                    Some(_) => format!("{}.{name}", pack.name),
                    None => name.clone(),
                };
                out.push(
                    json!({ "project": p.name, "formula": formula_ref, "description": formula.description,
                    "inputs": inputs }),
                );
            }
        }
    }
    out.into()
}

fn input_type(kind: InputType) -> &'static str {
    match kind {
        InputType::Bead => "bead",
        InputType::String => "string",
        InputType::Integer => "integer",
        InputType::Boolean => "boolean",
    }
}
