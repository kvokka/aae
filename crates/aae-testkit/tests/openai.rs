//! The fake OpenAI Responses API answers every mode of its plan over plain HTTP, and the real `codex` takes its
//! answers (spec §7.8).

use aae_testkit::openai::{Provider, Reply};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

/// What came back within `wait`: the status line and headers, then the body as far as it got.
async fn post(provider: &Provider, path: &str, wait: Duration) -> (String, String) {
    let base = provider.base_url();
    let addr = base.trim_start_matches("http://").trim_end_matches("/v1").to_owned();
    let mut stream = TcpStream::connect(&addr).await.unwrap();
    let body = json!({ "model": "gpt-fake", "stream": true, "input": [] }).to_string();
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

const SECOND: Duration = Duration::from_secs(1);

#[tokio::test]
async fn the_plan_goes_in_order_and_its_last_reply_repeats() {
    let p = Provider::start(vec![Reply::RateLimited, Reply::Text("OK".into())]).await;
    let (head, body) = post(&p, "/v1/responses", SECOND).await;
    assert!(
        head.starts_with("http/1.1 429") && head.contains("retry-after: 1"),
        "{head}"
    );
    assert!(body.contains("\"rate_limit_exceeded\""));
    for _ in 0..2 {
        let (head, body) = post(&p, "/v1/responses", SECOND).await;
        assert!(head.contains("content-type: text/event-stream"), "{head}");
        for event in [
            "response.created",
            "response.output_text.delta",
            "response.output_item.done",
        ] {
            assert!(body.contains(&format!("event: {event}\n")), "{body}");
        }
        assert!(body.contains("\"text\":\"OK\"") && body.contains("event: response.completed\n"));
    }
    assert_eq!(p.requests().len(), 3);
    assert_eq!(p.requests()[2].body["model"], "gpt-fake");
    // Other paths are no model requests: 404, the plan does not move.
    let (head, _) = post(&p, "/v1/models", SECOND).await;
    assert!(head.starts_with("http/1.1 404"));
    assert_eq!(p.requests().len(), 3);
}

#[tokio::test]
async fn every_error_has_its_status_code_and_headers() {
    let cases = [
        (
            Reply::UsageLimit(3600),
            "429",
            "usage_limit_reached",
            "x-codex-primary-reset-at: ",
        ),
        (Reply::StrangeQuota, "429", "Usage paused", "content-type"),
        (Reply::InsufficientQuota, "429", "insufficient_quota", "content-type"),
        (Reply::Overloaded, "503", "server_is_overloaded", "content-type"),
        (Reply::Unauthorized, "401", "token_expired", "content-type"),
        (
            Reply::NoModel,
            "404",
            "The model `gpt-fake` does not exist",
            "content-type",
        ),
        (
            Reply::ContextOverflow,
            "200",
            "context_length_exceeded",
            "text/event-stream",
        ),
    ];
    for (reply, status, text, header) in cases {
        let p = Provider::start(vec![reply.clone()]).await;
        let (head, body) = post(&p, "/v1/responses", SECOND).await;
        assert!(head.starts_with(&format!("http/1.1 {status}")), "{reply:?}: {head}");
        assert!(body.contains(text), "{reply:?}: {body}");
        assert!(head.contains(header), "{reply:?}: {head}");
    }
    let p = Provider::start(vec![Reply::StrangeQuota]).await;
    let (head, _) = post(&p, "/v1/responses", SECOND).await;
    assert!(!head.contains("retry-after") && !head.contains("x-codex"), "{head}");
}

#[tokio::test]
async fn a_hang_starts_the_stream_and_a_silence_sends_nothing() {
    let p = Provider::start(vec![Reply::Hang]).await;
    let (head, body) = post(&p, "/v1/responses", SECOND).await;
    assert!(head.starts_with("http/1.1 200"));
    assert!(
        body.contains("event: response.created") && !body.contains("response.completed"),
        "{body}"
    );
    let p = Provider::start(vec![Reply::Silent]).await;
    let (head, body) = post(&p, "/v1/responses", SECOND).await;
    assert_eq!((head.as_str(), body.as_str()), ("", ""));
}

/// The real `codex` (pinned in `mise.toml`) runs the command the fake asks for and ends on its text: the success
/// and command shapes are ones codex accepts.
#[tokio::test]
async fn the_real_codex_runs_the_command_and_ends_on_the_text() {
    let p = Provider::start(vec![
        Reply::Command("echo ran > marker".into()),
        Reply::Text("FAKE-DONE".into()),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (home, work) = (dir.path().join("home"), dir.path().join("work"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let config = format!(
        "model = \"gpt-fake\"\nmodel_provider = \"fake\"\napproval_policy = \"never\"\n\
         sandbox_mode = \"danger-full-access\"\n[analytics]\nenabled = false\n[feedback]\nenabled = false\n\
         [model_providers.fake]\nname = \"fake\"\nbase_url = \"{}\"\nwire_api = \"responses\"\n",
        p.base_url()
    );
    std::fs::write(home.join("config.toml"), config).unwrap();
    let mut codex = std::process::Command::new("codex");
    codex
        .args(["exec", "--json", "--skip-git-repo-check", "run it"])
        .current_dir(&work)
        .env("CODEX_HOME", &home)
        .stdin(std::process::Stdio::null());
    // codex blocks a thread of its own while the fake keeps serving on the test's runtime.
    let out = tokio::task::spawn_blocking(move || codex.output());
    let out = timeout(Duration::from_secs(60), out).await.unwrap().unwrap().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("FAKE-DONE"), "{stdout}");
    assert_eq!(std::fs::read_to_string(work.join("marker")).unwrap().trim(), "ran");
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{stdout}");
    let output = requests[1].body["input"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item: &Value| item["type"] == "function_call_output" && item["call_id"] == "call_fake_1");
    assert!(output, "the second request carries the command's output");
}
