//! `loop-manager` binary entry point: the daemon and the CLI.

#![deny(clippy::disallowed_methods)]

mod bridge;
mod daemon;
mod doctor;
mod git_cache;
mod harness;

use clap::Parser;
use loop_manager_front::cli::{self, CacheCommand, Cli, Command, ConfigCommand};
use std::process::ExitCode;

fn main() -> ExitCode {
    // Taken now: once the binary is replaced on disk, Linux names the running one `<path> (deleted)`.
    let exe = std::env::current_exe().unwrap_or_else(|_| "loop-manager".into());
    let cli = Cli::parse();
    if !matches!(cli.command, Command::Daemon { service: None, .. }) {
        quiet_on_closed_stdout();
    }
    let config = match cli.command {
        Command::Format => {
            let value = match serde_json::from_reader(std::io::stdin().lock()) {
                Ok(value) => value,
                Err(e) => return cli::fail(cli.json, "usage", &format!("reading JSON from stdin: {e}")),
            };
            cli::print(cli.json, &value);
            return ExitCode::SUCCESS;
        }
        Command::Daemon {
            config,
            service: Some(kind),
        } => return doctor::service(kind, config),
        // A file named with `--config` is checked here as the daemon would load it, with no daemon; without one the
        // daemon is asked, as by any other command.
        Command::Doctor { config: Some(config) } => {
            return match daemon::Paths::resolve(Some(config), cli.socket) {
                Ok(paths) => cli::doctor(cli.json, &doctor::findings(&paths).into()),
                Err(e) => cli::fail(cli.json, "config_error", &format!("doctor: {e}")),
            };
        }
        Command::Cache(CacheCommand::Clear) => {
            return match loop_manager_front::dirs::cache() {
                Ok(dir) => git_cache::clear(&dir, cli.json),
                Err(e) => cli::fail(cli.json, "config_error", &format!("cache clear: {e}")),
            };
        }
        Command::McpBridge { command, timeout } => {
            return bridge::run(command, std::time::Duration::from_secs(timeout));
        }
        Command::Config(
            command @ (ConfigCommand::Check { config: Some(_) } | ConfigCommand::Explain { config: Some(_), .. }),
        ) => {
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
            eprintln!("loop-manager daemon: restart: {}: {e}", exe.display());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("loop-manager daemon: {e}");
            ExitCode::FAILURE
        }
    }
}

/// A command whose reader went away (`loop-manager status | head -1`) ends quietly with the exit of a SIGPIPE, as other CLI
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
