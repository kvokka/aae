//! A fake OpenAI Responses API (spec §7.8, docs/design/contracts.md 9): the real `codex` with a model provider
//! whose `base_url` is [`Provider::base_url`] gets answers from a plan instead of a model.

use axum::{
    Router,
    body::{Body, Bytes},
    http::{Method, StatusCode, Uri, header::HeaderName},
    response::Response,
};
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;

/// The answer to one model request.
#[derive(Clone, Debug)]
pub enum Reply {
    /// A text answer that completes the response.
    Text(String),
    /// A call of codex's `exec_command` tool running the command (an approval request, a long or a background
    /// command).
    Command(String),
    /// A turn of one command: `Command` while the request carries no command output, then `Text` "Done." —
    /// the same plan serves any number of threads at once.
    Task(String),
    /// A call without arguments of the request's function tool whose name ends with this (an MCP tool); a text
    /// answer if the request has no such tool.
    Mcp(String),
    /// One turn: the reply at the number of tool outputs in the request's input, the last one repeating — the same
    /// plan serves any number of threads at once.
    Turn(Vec<Reply>),
    /// 429 `rate_limit_exceeded` with `retry-after: 1`.
    RateLimited,
    /// The plan's usage limit: 429 `usage_limit_reached` with `resets_at` that many seconds ahead and the `x-codex-*`
    /// headers, as the lab saw it.
    UsageLimit(u64),
    /// A 429 worded unlike any known limit, without `retry-after` or `x-codex-*` headers.
    StrangeQuota,
    /// 429 `insufficient_quota`: the API account is out of credit.
    InsufficientQuota,
    /// 503 `server_is_overloaded`.
    Overloaded,
    /// 401: the token has expired.
    Unauthorized,
    /// A stream that fails with `context_length_exceeded`.
    ContextOverflow,
    /// 404 `model_not_found` for the requested model.
    NoModel,
    /// Headers and `response.created`, then silence.
    Hang,
    /// Silence before the headers.
    Silent,
}

/// A model request the fake received.
#[derive(Clone, Debug)]
pub struct Request {
    /// Its number among model requests, from 1.
    pub n: usize,
    pub body: Value,
}

/// The fake on a free port of 127.0.0.1, serving until dropped. Model requests (`POST …/responses`) take the
/// plan's replies in order, the last one repeating; every other path gets 404, as the lab fake answered.
pub struct Provider {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Request>>>,
    server: JoinHandle<()>,
}

impl Provider {
    /// # Panics
    /// If the plan is empty or no local port can be bound.
    pub async fn start(plan: Vec<Reply>) -> Self {
        assert!(!plan.is_empty(), "a plan needs a reply");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let addr = listener.local_addr().expect("a bound address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let plan = Arc::new(plan);
        let app = Router::new().fallback(move |method: Method, uri: Uri, body: Bytes| {
            let (seen, plan) = (seen.clone(), plan.clone());
            async move {
                if method != Method::POST || uri.path() != "/v1/responses" {
                    return error(StatusCode::NOT_FOUND, json!({ "message": "not found" }), &[]);
                }
                let body: Value = serde_json::from_slice(&body).unwrap_or_default();
                let n = {
                    let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
                    let n = seen.len() + 1;
                    seen.push(Request { n, body: body.clone() });
                    n
                };
                let reply = plan[n.min(plan.len()) - 1].clone();
                answer(n, reply, &body).await
            }
        });
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { addr, requests, server }
    }

    /// The `base_url` of a codex model provider with `wire_api = "responses"`.
    pub fn base_url(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    /// The model requests so far, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn answer(n: usize, reply: Reply, body: &Value) -> Response {
    let model = body["model"].as_str().unwrap_or_default();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    match reply {
        Reply::Text(text) => {
            let item = json!({ "type": "message", "id": format!("msg_fake_{n}"), "role": "assistant",
                "status": "completed", "content": [{ "type": "output_text", "text": text, "annotations": [] }] });
            let delta = json!({ "type": "response.output_text.delta", "item_id": format!("msg_fake_{n}"),
                "output_index": 0, "content_index": 0, "delta": text });
            stream(n, model, item, Some(delta))
        }
        Reply::Task(command) => {
            let ran = body["input"].as_array().and_then(|i| i.last()).map(|i| &i["type"]);
            let reply = match ran {
                Some(kind) if kind == "function_call_output" => Reply::Text("Done.".into()),
                _ => Reply::Command(command),
            };
            Box::pin(answer(n, reply, body)).await
        }
        Reply::Command(command) => {
            let arguments = json!({ "cmd": command }).to_string();
            let item = json!({ "type": "function_call", "id": format!("fc_fake_{n}"), "status": "completed",
                "call_id": format!("call_fake_{n}"), "name": "exec_command", "arguments": arguments });
            stream(n, model, item, None)
        }
        Reply::Mcp(suffix) => {
            // codex puts the tools of MCP server `s` in a namespace `mcp__s`: a call names both.
            let tools = body["tools"].as_array().into_iter().flatten();
            let named = |t: &Value| t["name"].as_str().is_some_and(|name| name.ends_with(&suffix));
            let mut found = tools.flat_map(|t| match t["tools"].as_array() {
                Some(inner) => inner.iter().map(|i| (i, t["name"].clone())).collect(),
                None => vec![(t, Value::Null)],
            });
            let Some((tool, namespace)) = found.find(|(t, _)| named(t)) else {
                return Box::pin(answer(n, Reply::Text(format!("No tool *{suffix}.")), body)).await;
            };
            let item = json!({ "type": "function_call", "id": format!("fc_fake_{n}"), "status": "completed",
                "call_id": format!("call_fake_{n}"), "name": tool["name"], "namespace": namespace,
                "arguments": "{}" });
            stream(n, model, item, None)
        }
        Reply::Turn(replies) => {
            let input = body["input"].as_array().into_iter().flatten();
            let done = input.filter(|i| i["type"] == "function_call_output").count();
            let reply = replies[done.min(replies.len() - 1)].clone();
            Box::pin(answer(n, reply, body)).await
        }
        Reply::RateLimited => error(
            StatusCode::TOO_MANY_REQUESTS,
            json!({ "type": "rate_limit_exceeded", "code": "rate_limit_exceeded",
                "message": "Rate limit reached. Please try again in 1s." }),
            &[("retry-after", "1")],
        ),
        Reply::UsageLimit(after) => {
            let reset = now + after;
            let (primary, week) = (reset.to_string(), (reset + 400_000).to_string());
            error(
                StatusCode::TOO_MANY_REQUESTS,
                json!({ "type": "usage_limit_reached", "message": "The usage limit has been reached",
                    "plan_type": "pro", "resets_at": reset, "limit_window_minutes": 300 }),
                &[
                    ("x-codex-primary-used-percent", "100"),
                    ("x-codex-primary-window-minutes", "300"),
                    ("x-codex-primary-reset-at", &primary),
                    ("x-codex-secondary-used-percent", "42"),
                    ("x-codex-secondary-window-minutes", "10080"),
                    ("x-codex-secondary-reset-at", &week),
                ],
            )
        }
        Reply::StrangeQuota => error(
            StatusCode::TOO_MANY_REQUESTS,
            json!({ "message": "Usage paused for this workspace until the next billing period." }),
            &[],
        ),
        Reply::InsufficientQuota => error(
            StatusCode::TOO_MANY_REQUESTS,
            json!({ "type": "insufficient_quota", "code": "insufficient_quota",
                "message": "You exceeded your current quota, please check your plan and billing details." }),
            &[],
        ),
        Reply::Overloaded => error(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({ "code": "server_is_overloaded", "message": "Our servers are currently overloaded." }),
            &[],
        ),
        Reply::Unauthorized => error(
            StatusCode::UNAUTHORIZED,
            json!({ "code": "token_expired", "message": "Your authentication token has expired." }),
            &[],
        ),
        Reply::ContextOverflow => {
            let failed = json!({ "type": "response.failed", "sequence_number": 1, "response": {
                "id": format!("resp_fake_{n}"), "object": "response", "status": "failed", "output": [],
                "error": { "code": "context_length_exceeded",
                    "message": "Your input exceeds the context window of this model." } } });
            respond(StatusCode::OK, "text/event-stream", &[], Body::from(sse(&failed)))
        }
        Reply::NoModel => error(
            StatusCode::NOT_FOUND,
            json!({ "type": "invalid_request_error", "code": "model_not_found",
                "message": format!("The model `{model}` does not exist or you do not have access to it.") }),
            &[],
        ),
        Reply::Hang => {
            let created = response(n, model, "in_progress", json!([]));
            let created = json!({ "type": "response.created", "sequence_number": 0, "response": created });
            let events = tokio_stream::iter([Ok::<_, Infallible>(sse(&created))]).chain(tokio_stream::pending());
            respond(StatusCode::OK, "text/event-stream", &[], Body::from_stream(events))
        }
        Reply::Silent => std::future::pending().await,
    }
}

/// A completed response of one output item; a text item is streamed by its delta first.
fn stream(n: usize, model: &str, item: Value, delta: Option<Value>) -> Response {
    let mut events = vec![
        json!({ "type": "response.created", "response": response(n, model, "in_progress", json!([])) }),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": item }),
    ];
    events.extend(delta);
    events.push(json!({ "type": "response.output_item.done", "output_index": 0, "item": item }));
    let mut completed = response(n, model, "completed", json!([item]));
    completed["usage"] = json!({ "input_tokens": 10, "input_tokens_details": { "cached_tokens": 0 },
        "output_tokens": 3, "output_tokens_details": { "reasoning_tokens": 0 }, "total_tokens": 13 });
    events.push(json!({ "type": "response.completed", "response": completed }));
    let body: Vec<Bytes> = events
        .into_iter()
        .enumerate()
        .map(|(i, mut event)| {
            event["sequence_number"] = json!(i);
            sse(&event)
        })
        .collect();
    respond(StatusCode::OK, "text/event-stream", &[], Body::from(body.concat()))
}

fn response(n: usize, model: &str, status: &str, output: Value) -> Value {
    json!({ "id": format!("resp_fake_{n}"), "object": "response", "status": status, "model": model, "output": output })
}

fn sse(data: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {}\ndata: {data}\n\n",
        data["type"].as_str().unwrap_or_default()
    ))
}

fn error(status: StatusCode, error: Value, headers: &[(&str, &str)]) -> Response {
    let body = json!({ "error": error });
    respond(status, "application/json", headers, Body::from(body.to_string()))
}

fn respond(status: StatusCode, content_type: &str, headers: &[(&str, &str)], body: Body) -> Response {
    let mut response = Response::builder().status(status).header("content-type", content_type);
    for (name, value) in headers {
        response = response.header(HeaderName::from_bytes(name.as_bytes()).expect("a header name"), *value);
    }
    response.body(body).expect("a valid response")
}
