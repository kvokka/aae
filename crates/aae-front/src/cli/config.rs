//! The config commands: the lists of projects, agents, formulas, packs and plugins, and `aae config`.

use super::{encode, fail, print};
use crate::{
    api::config::{Explain, checked, explained},
    client::{Client, ClientError},
};
use aae_core::config::Config;
use clap::Subcommand;
use std::{path::PathBuf, process::ExitCode};

#[derive(Subcommand)]
pub enum ListCommand {
    /// List them, of every project or of `--project`.
    List {
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Check the config file as a reload would; exits non-zero when it has errors. Needs no daemon.
    Check {
        /// The orchestrator config (default: `$XDG_CONFIG_HOME/aae/config.toml`).
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// The effective config of an agent, or of the agent of a formula's run on a rig or in the project root with the
    /// layers of its pack, its step and the rig's variant: every value with its layer, the host layer in one line.
    /// Needs no daemon.
    Explain {
        /// The orchestrator config (default: `$XDG_CONFIG_HOME/aae/config.toml`).
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        project: String,
        /// `<pack>.<name>`, or a bare name of the project's own pack.
        #[arg(long, required_unless_present = "formula", conflicts_with = "formula")]
        agent: Option<String>,
        /// A formula, as a run start takes it; on `--rig`, or in the project root without it.
        #[arg(long)]
        formula: Option<String>,
        /// The rig of the run; its layer counts when given.
        #[arg(long)]
        rig: Option<String>,
    },
    /// Read the config file again: new runs and schedules take it, running runs keep theirs.
    Reload {
        /// The idempotency key: a repeat with the same key changes nothing.
        #[arg(long = "key")]
        idempotency: Option<String>,
    },
}

/// `aae <what> list`: `what` is the API collection (`projects`, `agents`, ...).
pub(super) async fn list(client: &Client, json: bool, what: &str, command: ListCommand) -> Result<(), ClientError> {
    let ListCommand::List { project } = command;
    let query = project.map(|p| format!("?project={}", encode(&p))).unwrap_or_default();
    print(
        json,
        &client.request("GET", &format!("/v1/{what}{query}"), None, None).await?,
    );
    Ok(())
}

/// `aae config reload`; `check` and `explain` run without a daemon, in [`local`].
pub(super) async fn reload(client: &Client, json: bool, idempotency: Option<String>) -> Result<(), ClientError> {
    let reply = client
        .request("POST", "/v1/config/reload", None, idempotency.as_deref())
        .await?;
    print(json, &reply);
    Ok(())
}

/// `aae config check|explain` on the config file as the daemon loads it, with no daemon: `load` takes `--config`.
/// Prints what the API would answer; `reload` needs the daemon.
pub fn local(
    json: bool,
    command: ConfigCommand,
    load: impl FnOnce(Option<PathBuf>) -> Result<Config, Vec<String>>,
) -> ExitCode {
    match command {
        ConfigCommand::Check { config } => {
            let reply = checked(load(config));
            if json {
                print(json, &reply);
            }
            let errors = reply["errors"].as_array().cloned().unwrap_or_default();
            for error in errors.iter().filter(|_| !json) {
                eprintln!("{}", error.as_str().unwrap_or_default());
            }
            match errors.is_empty() {
                true => ExitCode::SUCCESS,
                false => ExitCode::FAILURE,
            }
        }
        ConfigCommand::Explain {
            config,
            project,
            agent,
            formula,
            rig,
        } => {
            let config = match load(config) {
                Ok(config) => config,
                Err(errors) => {
                    return fail(
                        json,
                        "config_error",
                        &format!("the config has errors:\n{}", errors.join("\n")),
                    );
                }
            };
            let of = agent.map_or_else(|| Explain::Formula(formula.unwrap_or_default()), Explain::Agent);
            match explained(&config, &project, &of, rig.as_deref()) {
                Ok(reply) if json => print(json, &reply),
                Ok(reply) => print!("{}", reply["text"].as_str().unwrap_or_default()),
                Err(e) => return fail(json, "not_found", &e),
            }
            ExitCode::SUCCESS
        }
        ConfigCommand::Reload { .. } => fail(json, "usage", "`config reload` needs the daemon"),
    }
}
