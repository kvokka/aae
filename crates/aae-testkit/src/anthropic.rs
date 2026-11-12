//! A fake Anthropic Messages API (spec §7.8, docs/design/contracts.md 9): the real `claude` with
//! `ANTHROPIC_BASE_URL` set to [`Provider::base_url`] gets answers from a plan instead of a model.

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
    /// A text answer.
    Text(String),
    /// A `tool_use` of `Bash` running the command (a permission request, a long or a background command).
    Bash(String),
    /// 429 `rate_limit_error` with `retry-after: 1`.
    RateLimited,
    /// The account quota: 429 with `retry-after: 3600` and the `anthropic-ratelimit-unified-*` headers of a
    /// rejection that resets in an hour.
    Quota,
    /// A 429 worded unlike any known limit, without `retry-after` or unified headers.
    StrangeQuota,
    /// 529 `overloaded_error`.
    Overloaded,
    /// 401 `authentication_error`.
    Unauthorized,
    /// 400: the credit balance is too low.
    Billing,
    /// 400: the prompt is too long.
    ContextOverflow,
    /// 404 `not_found_error` for the requested model.
    NoModel,
    /// Headers and `message_start`, then silence.
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

/// The fake on a free port of 127.0.0.1, serving until dropped. Model requests (`POST /v1/messages`) take the
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
                if method != Method::POST || uri.path() != "/v1/messages" {
                    return error(StatusCode::NOT_FOUND, "not_found_error", "fake", &[]);
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

    /// The value for `ANTHROPIC_BASE_URL`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
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
    let stream = body["stream"].as_bool().unwrap_or(false);
    let reset = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + 3600;
    let reset = reset.to_string();
    match reply {
        Reply::Text(text) => {
            let block = json!({ "type": "text", "text": text });
            let delta = json!({ "type": "text_delta", "text": text });
            message(n, model, stream, block, delta, "end_turn")
        }
        Reply::Bash(command) => {
            let input = json!({ "command": command, "description": "run a command" });
            let block = json!({ "type": "tool_use", "id": format!("toolu_fake_{n}"), "name": "Bash", "input": input });
            let delta = json!({ "type": "input_json_delta", "partial_json": input.to_string() });
            message(n, model, stream, block, delta, "tool_use")
        }
        Reply::RateLimited => error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            "This request would exceed your organization's rate limit of 50 requests per minute",
            &[("retry-after", "1")],
        ),
        Reply::Quota => error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            "This request would exceed your account's rate limit. Please try again later.",
            &[
                ("retry-after", "3600"),
                ("anthropic-ratelimit-unified-status", "rejected"),
                ("anthropic-ratelimit-unified-reset", &reset),
                ("anthropic-ratelimit-unified-representative-claim", "five_hour"),
                ("anthropic-ratelimit-unified-5h-status", "rejected"),
                ("anthropic-ratelimit-unified-5h-reset", &reset),
                ("anthropic-ratelimit-unified-5h-utilization", "1.0"),
                ("anthropic-ratelimit-unified-overage-status", "rejected"),
                (
                    "anthropic-ratelimit-unified-overage-disabled-reason",
                    "org_level_disabled",
                ),
            ],
        ),
        Reply::StrangeQuota => error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            "Usage paused for this workspace until the next billing period.",
            &[],
        ),
        Reply::Overloaded => error(
            StatusCode::from_u16(529).unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
            "overloaded_error",
            "Overloaded",
            &[],
        ),
        Reply::Unauthorized => error(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "invalid x-api-key",
            &[],
        ),
        Reply::Billing => error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or \
             purchase credits.",
            &[],
        ),
        Reply::ContextOverflow => error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "prompt is too long: 1210000 tokens > 1000000 maximum",
            &[],
        ),
        Reply::NoModel => error(
            StatusCode::NOT_FOUND,
            "not_found_error",
            &format!("model: {model}"),
            &[],
        ),
        Reply::Hang if stream => {
            let start = sse("message_start", &message_start(n, model));
            let events = tokio_stream::iter([Ok::<_, Infallible>(start)]).chain(tokio_stream::pending());
            respond(StatusCode::OK, "text/event-stream", n, &[], Body::from_stream(events))
        }
        Reply::Hang | Reply::Silent => std::future::pending().await,
    }
}

/// A complete assistant message of one content block, streamed or as JSON.
fn message(n: usize, model: &str, stream: bool, block: Value, delta: Value, stop: &str) -> Response {
    if !stream {
        let mut message = message_start(n, model)["message"].clone();
        message["content"] = json!([block]);
        message["stop_reason"] = json!(stop);
        return respond(
            StatusCode::OK,
            "application/json",
            n,
            &[],
            Body::from(message.to_string()),
        );
    }
    // A streamed block starts empty and is filled by its delta.
    let mut empty = block;
    match empty["type"].as_str() {
        Some("text") => empty["text"] = json!(""),
        _ => empty["input"] = json!({}),
    }
    let events = [
        sse("message_start", &message_start(n, model)),
        sse(
            "content_block_start",
            &json!({ "type": "content_block_start", "index": 0, "content_block": empty }),
        ),
        sse(
            "content_block_delta",
            &json!({ "type": "content_block_delta", "index": 0, "delta": delta }),
        ),
        sse(
            "content_block_stop",
            &json!({ "type": "content_block_stop", "index": 0 }),
        ),
        sse(
            "message_delta",
            &json!({ "type": "message_delta", "delta": { "stop_reason": stop, "stop_sequence": null },
                "usage": { "output_tokens": 3 } }),
        ),
        sse("message_stop", &json!({ "type": "message_stop" })),
    ];
    respond(StatusCode::OK, "text/event-stream", n, &[], Body::from(events.concat()))
}

fn message_start(n: usize, model: &str) -> Value {
    json!({ "type": "message_start", "message": { "id": format!("msg_fake_{n}"), "type": "message",
        "role": "assistant", "model": model, "content": [], "stop_reason": null, "stop_sequence": null,
        "usage": { "input_tokens": 10, "output_tokens": 1, "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0 } } })
}

fn sse(event: &str, data: &Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}

fn error(status: StatusCode, kind: &str, text: &str, headers: &[(&str, &str)]) -> Response {
    let body = json!({ "type": "error", "error": { "type": kind, "message": text } });
    respond(status, "application/json", 0, headers, Body::from(body.to_string()))
}

fn respond(status: StatusCode, content_type: &str, n: usize, headers: &[(&str, &str)], body: Body) -> Response {
    let mut response = Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("request-id", format!("req_fake_{n}"));
    for (name, value) in headers {
        response = response.header(HeaderName::from_bytes(name.as_bytes()).expect("a header name"), *value);
    }
    response.body(body).expect("a valid response")
}
