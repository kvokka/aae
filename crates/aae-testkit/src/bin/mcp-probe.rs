//! The test MCP servers of `aae_testkit::mcp`: `mcp-probe <log>`, the HTTP service as aae starts a service, and
//! `mcp-probe --stdio <log>`, the stdio server.

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let served = match args.as_slice() {
        [flag, log] if flag == "--stdio" => aae_testkit::mcp::serve_stdio(log.as_ref()),
        [log] => aae_testkit::mcp::serve_service(log.as_ref()).await,
        _ => {
            eprintln!("usage: mcp-probe [--stdio] <log>");
            return std::process::ExitCode::FAILURE;
        }
    };
    match served {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mcp-probe: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
