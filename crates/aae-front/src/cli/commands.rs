//! The command groups of the CLI: `run`, `gate`, `trigger` and `instance`, each with the API calls behind it.

use super::{encode, print, run_path, text};
use crate::client::{Client, ClientError};
use clap::{Subcommand, ValueEnum};
use serde_json::{Map, Value, json};

#[derive(Subcommand)]
pub enum RunCommand {
    /// Start a run of a formula; prints its id.
    Start {
        project: String,
        rig: String,
        formula: String,
        /// An input as `name=value`; a value that parses as JSON is taken as JSON.
        #[arg(long = "input", value_name = "NAME=VALUE")]
        inputs: Vec<String>,
        /// A terminal run whose last session this run continues.
        #[arg(long)]
        continue_from: Option<String>,
        /// The idempotency key: a repeat with the same key starts nothing new.
        #[arg(long)]
        key: Option<String>,
    },
    /// List runs, newest first.
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Show a run.
    Show { id: String },
    /// Cancel a run.
    Cancel {
        id: String,
        #[arg(long)]
        reason: Option<String>,
        /// The idempotency key: a repeat with the same key changes nothing.
        #[arg(long = "key")]
        idempotency: Option<String>,
    },
    /// Answer the open request of a run: `allow`, `deny <message>` or `answer <text>`.
    Answer {
        id: String,
        request: String,
        decision: Decision,
        text: Option<String>,
        /// The idempotency key: a repeat with the same key changes nothing.
        #[arg(long = "key")]
        idempotency: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Decision {
    Allow,
    Deny,
    Answer,
}

#[derive(Subcommand)]
pub enum GateCommand {
    /// List the closed gates.
    List,
    /// Close a gate as the operator: only the operator opens it again.
    Close {
        /// `orchestrator`, `project:<p>`, `rig:<p>/<r>`, `agent:<a>`, `account:<h>/<id>`, ...
        gate: String,
        /// UTC, `YYYY-MM-DDTHH:MM:SSZ`; absent: until opened by hand.
        #[arg(long)]
        until: Option<String>,
        #[arg(long, default_value = "")]
        reason: String,
        /// The idempotency key: a repeat with the same key changes nothing.
        #[arg(long = "key")]
        idempotency: Option<String>,
    },
    /// Open a gate.
    Open {
        gate: String,
        /// The idempotency key: a repeat with the same key changes nothing.
        #[arg(long = "key")]
        idempotency: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum TriggerCommand {
    /// List the triggers on their rigs with their next firing.
    List,
    /// Fire a trigger on a rig as its schedule would: prints the run it started, or why it skipped.
    Fire {
        /// `<pack>.<name>`, or the name of a project trigger.
        trigger: String,
        #[arg(long)]
        project: String,
        #[arg(long)]
        rig: String,
        /// The idempotency key: a repeat with the same key changes nothing.
        #[arg(long = "key")]
        idempotency: Option<String>,
    },
}

pub(super) async fn run(client: &Client, json: bool, command: RunCommand) -> Result<(), ClientError> {
    match command {
        RunCommand::Start {
            project,
            rig,
            formula,
            inputs,
            continue_from,
            key,
        } => {
            let inputs: Map<String, Value> = inputs.iter().map(|i| input(i)).collect();
            let body = json!({ "project": project, "rig": rig, "formula": formula, "inputs": inputs,
                "continue_from": continue_from });
            let reply = client.request("POST", "/v1/runs", Some(&body), key.as_deref()).await?;
            if json {
                print(json, &reply)
            } else {
                println!("{}", reply["run"].as_str().unwrap_or_default())
            }
        }
        RunCommand::List { project, state, limit } => {
            let query: Vec<String> = [
                ("project", project),
                ("state", state),
                ("limit", limit.map(|l| l.to_string())),
            ]
            .into_iter()
            .filter_map(|(k, v)| Some(format!("{k}={}", encode(&v?))))
            .collect();
            let runs = client
                .request("GET", &format!("/v1/runs?{}", query.join("&")), None, None)
                .await?;
            if json {
                print(json, &runs);
            }
            for run in runs.as_array().filter(|_| !json).into_iter().flatten() {
                let at = format!("{}/{}", text(&run["project"]), text(&run["rig"]));
                let line = [
                    text(&run["id"]),
                    text(&run["state"]),
                    at,
                    text(&run["formula"]),
                    text(&run["created_at"]),
                ];
                println!("{}", line.join("  "));
            }
        }
        RunCommand::Show { id } => print(json, &client.request("GET", &run_path(&id, ""), None, None).await?),
        RunCommand::Cancel {
            id,
            reason,
            idempotency,
        } => {
            let body = json!({ "reason": reason.unwrap_or_default() });
            let path = run_path(&id, "/cancel");
            print(
                json,
                &client
                    .request("POST", &path, Some(&body), idempotency.as_deref())
                    .await?,
            );
        }
        RunCommand::Answer {
            id,
            request,
            decision,
            text,
            idempotency,
        } => {
            let answer = match decision {
                Decision::Allow => json!({ "decision": "allow" }),
                Decision::Deny => json!({ "decision": "deny", "message": text.unwrap_or_default() }),
                Decision::Answer => json!({ "decision": "answer", "text": text.unwrap_or_default() }),
            };
            let body = json!({ "request": request, "answer": answer });
            let path = run_path(&id, "/answer");
            print(
                json,
                &client
                    .request("POST", &path, Some(&body), idempotency.as_deref())
                    .await?,
            );
        }
    }
    Ok(())
}

pub(super) async fn gate(client: &Client, json: bool, command: GateCommand) -> Result<(), ClientError> {
    let (path, body, idempotency) = match command {
        GateCommand::List => {
            let gates = client.request("GET", "/v1/gates", None, None).await?;
            if json {
                print(json, &gates);
            }
            for gate in gates.as_array().filter(|_| !json).into_iter().flatten() {
                let until = gate["until"].as_str().unwrap_or("until opened");
                let line = [
                    text(&gate["key"]),
                    text(&gate["closed_by"]),
                    until.to_owned(),
                    text(&gate["reason"]),
                ];
                println!("{}", line.join("  "));
            }
            return Ok(());
        }
        GateCommand::Close {
            gate,
            until,
            reason,
            idempotency,
        } => (
            "/v1/gates",
            json!({ "key": gate, "until": until, "reason": reason }),
            idempotency,
        ),
        GateCommand::Open { gate, idempotency } => ("/v1/gates/open", json!({ "key": gate }), idempotency),
    };
    print(
        json,
        &client
            .request("POST", path, Some(&body), idempotency.as_deref())
            .await?,
    );
    Ok(())
}

pub(super) async fn trigger(client: &Client, json: bool, command: TriggerCommand) -> Result<(), ClientError> {
    let TriggerCommand::Fire {
        trigger,
        project,
        rig,
        idempotency,
    } = command
    else {
        print(json, &client.request("GET", "/v1/triggers", None, None).await?);
        return Ok(());
    };
    let body = json!({ "project": project, "rig": rig, "trigger": trigger });
    let reply = client
        .request("POST", "/v1/triggers/fire", Some(&body), idempotency.as_deref())
        .await?;
    print(json, &reply);
    Ok(())
}

#[derive(Subcommand)]
pub enum InstanceCommand {
    /// List the harness instances.
    List,
    /// Stop an idle instance.
    Stop {
        id: String,
        /// The idempotency key: a repeat with the same key changes nothing.
        #[arg(long = "key")]
        idempotency: Option<String>,
    },
}

pub(super) async fn instance(client: &Client, json: bool, command: InstanceCommand) -> Result<(), ClientError> {
    let reply = match command {
        InstanceCommand::List => client.request("GET", "/v1/instances", None, None).await?,
        InstanceCommand::Stop { id, idempotency } => {
            let path = format!("/v1/instances/{}/stop", encode(&id));
            client.request("POST", &path, None, idempotency.as_deref()).await?
        }
    };
    print(json, &reply);
    Ok(())
}

/// `name=value`, the value as JSON if it parses, else as a string.
fn input(text: &str) -> (String, Value) {
    let (name, value) = text.split_once('=').unwrap_or((text, ""));
    let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned()));
    (name.to_owned(), value)
}
