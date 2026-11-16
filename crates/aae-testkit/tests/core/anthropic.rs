//! The fake Anthropic Messages API answers every mode of its plan over plain HTTP (spec §7.8).

use aae_testkit::anthropic::{Provider, Reply};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

/// What came back within `wait`: the status line and headers, then the body as far as it got.
async fn post(provider: &Provider, path: &str, body: &Value, wait: Duration) -> (String, String) {
    let addr = provider.base_url().trim_start_matches("http://").to_owned();
    let mut stream = TcpStream::connect(&addr).await.unwrap();
    let body = body.to_string();
    let request = format!(
        "POST {path} HTTP/1.1\r\nhost: {addr}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut got = Vec::new();
    let _ = timeout(wait, stream.read_to_end(&mut got)).await;
    let got = String::from_utf8_lossy(&got).into_owned();
    let (head, body) = got.split_once("\r\n\r\n").unwrap_or((&got, ""));
    (head.to_lowercase(), body.to_owned())
}

fn request(stream: bool) -> Value {
    json!({ "model": "claude-haiku-fake", "stream": stream, "max_tokens": 16,
        "messages": [{ "role": "user", "content": "reply OK" }] })
}

const SECOND: Duration = Duration::from_secs(1);

#[tokio::test]
async fn the_plan_goes_in_order_and_its_last_reply_repeats() {
    let p = Provider::start(vec![Reply::RateLimited, Reply::Text("OK".into())]).await;
    let (head, body) = post(&p, "/v1/messages", &request(true), SECOND).await;
    assert!(head.starts_with("http/1.1 429"), "{head}");
    assert!(head.contains("retry-after: 1"));
    assert!(body.contains("\"rate_limit_error\""));
    for _ in 0..2 {
        let (head, body) = post(&p, "/v1/messages", &request(true), SECOND).await;
        assert!(head.contains("content-type: text/event-stream"), "{head}");
        for event in ["message_start", "content_block_delta", "message_stop"] {
            assert!(body.contains(&format!("event: {event}\n")), "{body}");
        }
        assert!(body.contains("\"text\":\"OK\"") && body.contains("\"stop_reason\":\"end_turn\""));
    }
    let requests = p.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2].body["model"], "claude-haiku-fake");
    // Other paths are no model requests: 404, the plan does not move.
    let (head, _) = post(&p, "/v1/messages/count_tokens", &request(false), SECOND).await;
    assert!(head.starts_with("http/1.1 404"));
    assert_eq!(p.requests().len(), 3);
}

#[tokio::test]
async fn a_request_without_stream_gets_a_json_message() {
    let p = Provider::start(vec![Reply::Text("hi".into())]).await;
    let (head, body) = post(&p, "/v1/messages", &request(false), SECOND).await;
    assert!(head.contains("content-type: application/json"));
    let message: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(message["content"][0]["text"], "hi");
    assert_eq!(message["stop_reason"], "end_turn");
}

#[tokio::test]
async fn a_bash_reply_is_a_tool_use() {
    let p = Provider::start(vec![Reply::Bash("sleep 100".into())]).await;
    let (_, body) = post(&p, "/v1/messages", &request(true), SECOND).await;
    assert!(
        body.contains("\"type\":\"tool_use\"") && body.contains("\"name\":\"Bash\""),
        "{body}"
    );
    assert!(body.contains("sleep 100") && body.contains("\"stop_reason\":\"tool_use\""));
}

#[tokio::test]
async fn every_error_has_its_status_type_and_headers() {
    let cases = [
        (
            Reply::Quota,
            "429",
            "rate_limit_error",
            "anthropic-ratelimit-unified-status: rejected",
        ),
        (Reply::StrangeQuota, "429", "rate_limit_error", "request-id"),
        (Reply::Overloaded, "529", "overloaded_error", "request-id"),
        (Reply::Unauthorized, "401", "authentication_error", "request-id"),
        (Reply::Billing, "400", "credit balance is too low", "request-id"),
        (Reply::ContextOverflow, "400", "prompt is too long", "request-id"),
        (Reply::NoModel, "404", "model: claude-haiku-fake", "request-id"),
    ];
    for (reply, status, text, header) in cases {
        let p = Provider::start(vec![reply.clone()]).await;
        let (head, body) = post(&p, "/v1/messages", &request(true), SECOND).await;
        assert!(head.starts_with(&format!("http/1.1 {status}")), "{reply:?}: {head}");
        assert!(body.contains(text), "{reply:?}: {body}");
        assert!(head.contains(header), "{reply:?}: {head}");
    }
    let p = Provider::start(vec![Reply::Quota]).await;
    let (head, _) = post(&p, "/v1/messages", &request(true), SECOND).await;
    assert!(head.contains("retry-after: 3600") && head.contains("anthropic-ratelimit-unified-reset: "));
    let p = Provider::start(vec![Reply::StrangeQuota]).await;
    let (head, _) = post(&p, "/v1/messages", &request(true), SECOND).await;
    assert!(!head.contains("retry-after") && !head.contains("anthropic-ratelimit"));
}

#[tokio::test]
async fn a_hang_starts_the_stream_and_a_silence_sends_nothing() {
    let p = Provider::start(vec![Reply::Hang]).await;
    let (head, body) = post(&p, "/v1/messages", &request(true), SECOND).await;
    assert!(head.starts_with("http/1.1 200"));
    assert!(
        body.contains("event: message_start") && !body.contains("message_stop"),
        "{body}"
    );
    let p = Provider::start(vec![Reply::Silent]).await;
    let (head, body) = post(&p, "/v1/messages", &request(true), SECOND).await;
    assert_eq!((head.as_str(), body.as_str()), ("", ""));
}
