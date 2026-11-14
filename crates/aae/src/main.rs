//! `aae` binary entry point: the daemon and the CLI.

mod bridge;
mod daemon;
mod doctor;
mod fetch;
mod harness;

use aae_front::cli::{self, Cli, Command, ConfigCommand};
use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Taken now: once the binary is replaced on disk, Linux names the running one `<path> (deleted)`.
    let exe = std::env::current_exe().unwrap_or_else(|_| "aae".into());
    let cli = Cli::parse();
    if !matches!(cli.command, Command::Daemon { service: None, .. }) {
        quiet_on_closed_stdout();
    }
    let config = match cli.command {
        Command::Daemon {
            config,
            service: Some(kind),
        } => return doctor::service(kind, config),
        Command::Doctor { config } => {
            return match daemon::Paths::resolve(config, cli.socket) {
                Ok(paths) => doctor::run(&paths, cli.json),
                Err(e) => cli::fail(cli.json, "config_error", &format!("doctor: {e}")),
            };
        }
        Command::Fetch { config, update } => {
            return match daemon::Paths::resolve(config, cli.socket) {
                Ok(paths) => fetch::run(&paths.config, update),
                Err(e) => cli::fail(cli.json, "config_error", &format!("fetch: {e}")),
            };
        }
        Command::McpBridge { command } => return bridge::run(command),
        // Loaded here as the daemon loads it, so a check or an explain needs no daemon.
        Command::Config(command @ (ConfigCommand::Check { .. } | ConfigCommand::Explain { .. })) => {
            return cli::config_local(cli.json, command, |config| {
                harness::load(&daemon::Paths::resolve(config, cli.socket).map_err(|e| vec![e])?)
            });
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
        // A planned restart: the binary at the same path, so a new one put there, with the same arguments in this
        // process, so a supervisor sees no exit. Should that fail, the exit is an error a supervisor restarts.
        Ok(true) => {
            let mut again = std::process::Command::new(&exe);
            again.args(std::env::args_os().skip(1));
            let e = std::os::unix::process::CommandExt::exec(&mut again);
            eprintln!("aae daemon: restart: {}: {e}", exe.display());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("aae daemon: {e}");
            ExitCode::FAILURE
        }
    }
}

/// A command whose reader went away (`aae status | head -1`) ends quietly with the exit of a SIGPIPE, as other CLI
/// tools do, not with a panic of `print!`. Only the CLI: the daemon is not printing to a reader.
fn quiet_on_closed_stdout() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.payload().downcast_ref::<String>();
        if message.is_some_and(|m| m.starts_with("failed printing to stdout") && m.contains("Broken pipe")) {
            std::process::exit(141);
        }
        default(info);
    }));
}
