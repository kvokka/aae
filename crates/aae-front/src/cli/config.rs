//! The config commands: the lists of projects, agents, formulas, packs and plugins, and `aae config`.

use super::{encode, print};
use crate::client::{Client, ClientError};
use clap::Subcommand;
use std::process::ExitCode;

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
    /// Check the config file as a reload would; exits non-zero when it has errors.
    Check,
    /// The effective config of an agent: every value with its layer, the host layer in one line.
    Explain {
        #[arg(long)]
        project: String,
        /// `<pack>.<name>`, or a bare name of the project's own pack.
        #[arg(long)]
        agent: String,
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

pub(super) async fn config(client: &Client, json: bool, command: ConfigCommand) -> Result<ExitCode, ClientError> {
    match command {
        ConfigCommand::Check => {
            let reply = client.request("POST", "/v1/config/check", None, None).await?;
            if json {
                print(json, &reply);
            }
            let errors = reply["errors"].as_array().cloned().unwrap_or_default();
            for error in errors.iter().filter(|_| !json) {
                eprintln!("{}", error.as_str().unwrap_or_default());
            }
            return Ok(if errors.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        ConfigCommand::Explain { project, agent, rig } => {
            let mut path = format!(
                "/v1/config/explain?project={}&agent={}",
                encode(&project),
                encode(&agent)
            );
            if let Some(rig) = rig {
                path += &format!("&rig={}", encode(&rig));
            }
            let reply = client.request("GET", &path, None, None).await?;
            match json {
                true => print(json, &reply),
                false => print!("{}", reply["text"].as_str().unwrap_or_default()),
            }
        }
        ConfigCommand::Reload { idempotency } => {
            let reply = client
                .request("POST", "/v1/config/reload", None, idempotency.as_deref())
                .await?;
            print(json, &reply);
        }
    }
    Ok(ExitCode::SUCCESS)
}
