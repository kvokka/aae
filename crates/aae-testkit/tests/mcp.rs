//! The test MCP service answers both protocol epochs, refuses a wrong token and logs every request; the real `claude`
//! and `codex` (pinned in `mise.toml`) call its tool when the fake providers ask for it (spec §15 criterion 14).

use aae_testkit::{anthropic, mcp, openai};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

/// The service with token `tok` on a free port: its `/mcp` URL and its log under `dir`.
async fn service(dir: &Path) -> (String, PathBuf) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let log = dir.join("mcp.log");
    let at = log.clone();
    tokio::spawn(async move { mcp::serve(listener, "tok", &at).await.unwrap() });
    (url, log)
}

fn log(path: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// The status line and the body of `POST /mcp` with `headers`.
async fn post(url: &str, headers: &str, body: &Value) -> (String, String) {
    let addr = url.trim_start_matches("http://").trim_end_matches("/mcp");
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let body = body.to_string();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nhost: {addr}\r\ncontent-type: application/json\r\n{headers}content-length: {}\r\n\
         connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut got = String::new();
    stream.read_to_string(&mut got).await.unwrap();
    let (head, body) = got.split_once("\r\n\r\n").unwrap_or((&got, ""));
    (head.lines().next().unwrap_or_default().to_owned(), body.to_owned())
}

#[tokio::test]
async fn both_epochs_a_wrong_token_and_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let (url, path) = service(dir.path()).await;
    let auth = "authorization: Bearer tok\r\nx-aae-run: r1\r\nx-aae-agent: dev\r\n";
    let rpc = |method: &str, params: Value| json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });

    let (status, _) = post(&url, "authorization: Bearer bad\r\n", &rpc("tools/list", json!({}))).await;
    assert!(status.contains(" 401 "), "{status}");
    let (_, body) = post(
        &url,
        auth,
        &rpc("initialize", json!({ "protocolVersion": "2025-06-18" })),
    )
    .await;
    assert!(body.contains("\"protocolVersion\":\"2025-06-18\""), "{body}");
    let modern = format!("{auth}mcp-protocol-version: 2026-07-28\r\n");
    let (_, body) = post(&url, &modern, &rpc("server/discover", json!({}))).await;
    assert!(body.contains("\"supportedVersions\":[\"2026-07-28\"]"), "{body}");
    let (_, body) = post(&url, &modern, &rpc("tools/call", json!({ "name": mcp::TOOL }))).await;
    assert!(body.contains(r#"\"run\":\"r1\""#), "{body}");

    let lines = log(&path);
    let seen: Vec<_> = lines.iter().map(|l| (&l["method"], &l["authorized"])).collect();
    assert_eq!(
        seen,
        [
            (&json!("tools/list"), &json!(false)),
            (&json!("initialize"), &json!(true)),
            (&json!("server/discover"), &json!(true)),
            (&json!("tools/call"), &json!(true)),
        ]
    );
    assert!(lines.iter().all(|l| l["pid"] == std::process::id()));
    assert_eq!(
        (&lines[3]["tool"], &lines[3]["agent"]),
        (&json!("whoami"), &json!("dev"))
    );
}

/// `command` on a blocking thread while the fakes keep serving on the test's runtime.
async fn output(mut command: Command) -> Output {
    let out = tokio::task::spawn_blocking(move || command.output());
    let out = timeout(Duration::from_secs(60), out).await.unwrap().unwrap().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    out
}

/// The `tools/call` lines of the log.
fn calls(path: &Path) -> Vec<Value> {
    log(path).into_iter().filter(|l| l["method"] == "tools/call").collect()
}

#[tokio::test]
async fn the_real_claude_calls_the_tool_with_the_headers() {
    let dir = tempfile::tempdir().unwrap();
    let (url, path) = service(dir.path()).await;
    let p = anthropic::Provider::start(vec![anthropic::Reply::Turn(vec![
        anthropic::Reply::Mcp(mcp::TOOL.into()),
        anthropic::Reply::Text("Done.".into()),
    ])])
    .await;
    let config = json!({ "mcpServers": { "probe": { "type": "http", "url": url,
        "headers": { "Authorization": "Bearer tok", "X-Aae-Run": "r1" } } } });
    let mut claude = Command::new("claude");
    claude
        .args([
            "-p",
            "go",
            "--model",
            "claude-haiku-5-5",
            "--dangerously-skip-permissions",
        ])
        .args(["--strict-mcp-config", "--mcp-config", &config.to_string()])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("ANTHROPIC_BASE_URL", p.base_url())
        .env("ANTHROPIC_API_KEY", "fake")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        // With tool search on, claude defers MCP tools behind `ToolSearch` and the request lists none of them.
        .env("ENABLE_TOOL_SEARCH", "false");
    let out = output(claude).await;
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Done.");
    let calls = calls(&path);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        (&calls[0]["run"], &calls[0]["authorized"]),
        (&json!("r1"), &json!(true))
    );
    assert!(
        p.requests()[0].body["tools"]
            .to_string()
            .contains("\"mcp__probe__whoami\"")
    );
}

#[tokio::test]
async fn the_real_codex_calls_the_tool_with_the_headers() {
    let dir = tempfile::tempdir().unwrap();
    let (url, path) = service(dir.path()).await;
    let p = openai::Provider::start(vec![openai::Reply::Turn(vec![
        openai::Reply::Mcp(mcp::TOOL.into()),
        openai::Reply::Text("Done.".into()),
    ])])
    .await;
    let config = format!(
        "model = \"gpt-fake\"\nmodel_provider = \"fake\"\napproval_policy = \"never\"\n\
         sandbox_mode = \"danger-full-access\"\n[analytics]\nenabled = false\n[feedback]\nenabled = false\n\
         [model_providers.fake]\nname = \"fake\"\nbase_url = \"{}\"\nwire_api = \"responses\"\n\
         [mcp_servers.probe]\nurl = \"{url}\"\nhttp_headers = {{ Authorization = \"Bearer tok\", X-Aae-Run = \"r2\" }}\n",
        p.base_url()
    );
    std::fs::write(dir.path().join("config.toml"), config).unwrap();
    let mut codex = Command::new("codex");
    codex
        .args(["exec", "--skip-git-repo-check", "go"])
        .current_dir(dir.path())
        .env("CODEX_HOME", dir.path())
        .stdin(std::process::Stdio::null());
    let out = output(codex).await;
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Done.");
    let calls = calls(&path);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        (&calls[0]["run"], &calls[0]["authorized"]),
        (&json!("r2"), &json!(true))
    );
    // codex puts the tools of server `probe` in the namespace `mcp__probe`; the call answered names both.
    let sent = p.requests()[1].body["input"].to_string();
    assert!(sent.contains("\"namespace\":\"mcp__probe\""), "{sent}");
}
