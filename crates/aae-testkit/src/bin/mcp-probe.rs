//! The test MCP service of `aae_testkit::mcp` as aae starts a service: `mcp-probe <log>`.

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let Some(log) = std::env::args_os().nth(1) else {
        eprintln!("usage: mcp-probe <log>");
        return std::process::ExitCode::FAILURE;
    };
    match aae_testkit::mcp::serve_service(log.as_ref()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mcp-probe: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
