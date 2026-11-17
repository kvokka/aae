//! The config commands: the lists of projects, agents, formulas, packs and addons, and `loop-manager config`.

use super::{encode, fail, print};
use crate::{
    api::config::{Explain, checked, explained},
    client::{Client, ClientError},
};
use clap::Subcommand;
use loop_manager_core::config::Config;
use serde_json::Value;
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
pub enum FormulaCommand {
    /// List the formulas a run can start, of every project or of `--project`.
    List {
        #[arg(long)]
        project: Option<String>,
        /// Also the internal ones, which only a step of another formula starts.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Check the daemon's config file as a reload would; exits non-zero when it has errors.
    Check {
        /// Check this file without a daemon.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// The effective config of an agent, or of the agent of a formula's run on a rig or in the project root with the
    /// layers of its pack, its step and the rig's variant: every value with its layer, the host layer in one line.
    Explain {
        /// Explain from this file without a daemon.
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

/// `loop-manager <what> list`: `what` is the API collection (`projects`, `agents`, ...).
pub(super) async fn list(client: &Client, json: bool, what: &str, command: ListCommand) -> Result<(), ClientError> {
    let ListCommand::List { project } = command;
    let query = project.map(|p| format!("?project={}", encode(&p))).unwrap_or_default();
    print(
        json,
        &client.request("GET", &format!("/v1/{what}{query}"), None, None).await?,
    );
    Ok(())
}

/// `loop-manager formula list`: the API gives every formula with `internal`; without `--all` the internal ones are left out.
pub(super) async fn formulas(client: &Client, json: bool, command: FormulaCommand) -> Result<(), ClientError> {
    let FormulaCommand::List { project, all } = command;
    let query = project.map(|p| format!("?project={}", encode(&p))).unwrap_or_default();
    let mut reply = client
        .request("GET", &format!("/v1/formulas{query}"), None, None)
        .await?;
    if !all && let Some(items) = reply.as_array_mut() {
        items.retain(|f| f["internal"] != true);
    }
    print(json, &reply);
    Ok(())
}

/// `loop-manager config` on the daemon; `check` and `explain` of a file named with `--config` run without one, in [`local`].
pub(super) async fn config(client: &Client, json: bool, command: ConfigCommand) -> Result<ExitCode, ClientError> {
    let reply = match command {
        ConfigCommand::Check { .. } => client.request("POST", "/v1/config/check", None, None).await?,
        ConfigCommand::Explain {
            project,
            agent,
            formula,
            rig,
            ..
        } => {
            let query = [("agent", agent), ("formula", formula), ("rig", rig)].into_iter();
            let query: String = (query.filter_map(|(k, v)| v.map(|v| format!("&{k}={}", encode(&v))))).collect();
            let path = format!("/v1/config/explain?project={}{query}", encode(&project));
            client.request("GET", &path, None, None).await?
        }
        ConfigCommand::Reload { idempotency } => {
            let reply = client
                .request("POST", "/v1/config/reload", None, idempotency.as_deref())
                .await?;
            print(json, &reply);
            return Ok(ExitCode::SUCCESS);
        }
    };
    Ok(show(json, &reply))
}

/// `loop-manager config check|explain` on the file of `--config` as the daemon loads it, with no daemon: `load` takes the file.
/// Prints what the API would answer; `reload` needs the daemon.
pub fn local(
    json: bool,
    command: ConfigCommand,
    load: impl FnOnce(Option<PathBuf>) -> Result<Config, Vec<String>>,
) -> ExitCode {
    let reply = match command {
        ConfigCommand::Check { config } => checked(load(config)),
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
                Ok(reply) => reply,
                Err(e) => return fail(json, "not_found", &e),
            }
        }
        ConfigCommand::Reload { .. } => return fail(json, "usage", "`config reload` needs the daemon"),
    };
    show(json, &reply)
}

/// Prints the reply of a check or an explain, the daemon's or a file's alike: the text of an explain, the errors of a
/// check, which fail it; the reply as it is with `--json`.
fn show(json: bool, reply: &Value) -> ExitCode {
    match reply["text"].as_str() {
        _ if json => print(json, reply),
        Some(text) => print!("{text}"),
        None => {}
    }
    let errors = reply["errors"].as_array().map(Vec::as_slice).unwrap_or_default();
    for error in errors.iter().filter(|_| !json) {
        eprintln!("{}", error.as_str().unwrap_or_default());
    }
    match errors.is_empty() {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    }
}
