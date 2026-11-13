//! `aae` binary entry point: the daemon and the CLI.

mod daemon;
mod doctor;
mod fetch;

use aae_front::cli::{self, Cli, Command};
use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = match cli.command {
        Command::Daemon {
            config,
            service: Some(kind),
        } => return doctor::service(kind, config),
        Command::Doctor { config } => {
            return match daemon::Paths::resolve(config, cli.socket) {
                Ok(paths) => doctor::run(&paths, cli.json),
                Err(e) => {
                    eprintln!("aae doctor: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Command::Fetch { config, update } => {
            return match daemon::Paths::resolve(config, cli.socket) {
                Ok(paths) => fetch::run(&paths.config, update),
                Err(e) => {
                    eprintln!("aae fetch: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Command::Daemon { config, service: None } => config,
        _ => return cli::run(cli),
    };
    let started = daemon::Paths::resolve(config, cli.socket).and_then(|paths| {
        let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {e}"))?;
        runtime.block_on(daemon::run(paths))
    });
    match started {
        Ok(false) => ExitCode::SUCCESS,
        // A planned restart: the same binary with the same arguments in this process, so a supervisor sees no exit.
        Ok(true) => {
            let exe = std::env::current_exe().unwrap_or_else(|_| "aae".into());
            let mut again = std::process::Command::new(exe);
            again.args(std::env::args_os().skip(1));
            let e = std::os::unix::process::CommandExt::exec(&mut again);
            eprintln!("aae daemon: restart: {e}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("aae daemon: {e}");
            ExitCode::FAILURE
        }
    }
}
