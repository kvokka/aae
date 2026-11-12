//! `aae` binary entry point: the daemon and the CLI.

mod daemon;

use aae_front::cli::{self, Cli, Command};
use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Command::Daemon { config } = cli.command else {
        return cli::run(cli);
    };
    let started = daemon::Paths::resolve(config, cli.socket).and_then(|paths| {
        let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {e}"))?;
        runtime.block_on(daemon::run(paths))
    });
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aae daemon: {e}");
            ExitCode::FAILURE
        }
    }
}
