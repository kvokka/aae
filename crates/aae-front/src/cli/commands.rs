//! The command groups of the CLI: `run`, `gate`, `trigger` and `instance`, each with the API calls behind it.

use super::{encode, follow, print, run_path, text};
use crate::client::{Client, ClientError};
use clap::{Subcommand, ValueEnum};
use serde_json::{Map, Value, json};
use std::process::ExitCode;

#[derive(Subcommand)]
pub enum RunCommand {
    /// Start a run of a formula; prints its id. Inside an agent session (`AAE_RUN`, `AAE_ATTEMPT_TOKEN`) the run is
    /// a child of the session's run: same correlation, one level deeper. Options go after the positionals.
    #[command(allow_missing_positional = true)]
    Start {
        project: String,
        /// Optional: without it the run works in the project root, or on the rig of the bead its task names (its
        /// id prefix).
        rig: Option<String>,
        formula: String,
        /// An input as `name=value`: text, which an integer or a boolean input takes as its number or `true`/`false`
        /// and a task input as the task or the id of the bead it takes up; a JSON object or array (a task input's
        /// `{"next_ready": true}`) is taken as JSON.
        #[arg(long = "input", value_name = "NAME=VALUE", value_parser = name_value)]
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
        /// Only the runs whose `parent_run` is this one.
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Show a run.
    Show { id: String },
    /// Follow a run live until it ends: its events from `--from` (default: the oldest held) and the raw messages of
    /// its session from now on. Exits 0 however the run ends; `aae run wait` gives the outcome.
    Watch {
        id: String,
        #[arg(long)]
        from: Option<i64>,
    },
    /// Wait until every named run ends, then print each: state, class and reason of a failure, the finisher's summary
    /// and data. Exits 0 when all passed, 1 when not, 124 when `--timeout` ran out first.
    Wait {
        #[arg(required = true)]
        ids: Vec<String>,
        /// How long to wait at most: `90s`, `30m`, `2h`.
        #[arg(long, value_parser = aae_core::config::duration)]
        timeout: Option<std::time::Duration>,
    },
    /// Cancel a run, and the live runs its sessions started.
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
    Fire(OnRig),
    /// Switch a trigger on a rig back on.
    Enable(OnRig),
    /// Switch a trigger on a rig off: neither its schedule nor its events fire it, `fire` still does.
    Disable(OnRig),
}

/// A trigger on a rig.
#[derive(clap::Args)]
pub struct OnRig {
    /// `<pack>.<name>`, or the name of a project trigger.
    trigger: String,
    #[arg(long)]
    project: String,
    #[arg(long)]
    rig: String,
    /// The idempotency key: a repeat with the same key changes nothing.
    #[arg(long = "key")]
    idempotency: Option<String>,
}

/// Runs a `run` command; exits 3 when a run start found nothing ready to take.
pub(super) async fn run(client: &Client, json: bool, command: RunCommand) -> Result<ExitCode, ClientError> {
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
            // The session the command runs in, as the finisher finds it.
            let parent = match (std::env::var("AAE_RUN"), std::env::var("AAE_ATTEMPT_TOKEN")) {
                (Ok(run), Ok(token)) => json!({ "run": run, "token": token }),
                _ => Value::Null,
            };
            let body = json!({ "project": project, "rig": rig, "formula": formula, "inputs": inputs,
                "continue_from": continue_from, "parent": parent });
            let reply = client.request("POST", "/v1/runs", Some(&body), key.as_deref()).await?;
            match reply["run"].as_str() {
                _ if json => print(json, &reply),
                Some(run) => println!("{run}"),
                None => eprintln!("aae: nothing ready: no ready bead to take, no run started"),
            }
            for (bead, why) in reply["passed_over"].as_object().into_iter().flatten().filter(|_| !json) {
                eprintln!("aae: passed over {bead}: {}", why.as_str().unwrap_or_default());
            }
            if reply["nothing_ready"] == true {
                return Ok(ExitCode::from(3));
            }
        }
        RunCommand::List {
            project,
            state,
            parent,
            limit,
        } => {
            let query: Vec<String> = [
                ("project", project),
                ("state", state),
                ("parent", parent),
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
        RunCommand::Watch { id, from } => {
            let query = from.map(|f| format!("?from={f}")).unwrap_or_default();
            follow(client.stream(&run_path(&id, &format!("/live{query}"))).await?, json).await?;
        }
        RunCommand::Wait { ids, timeout } => return super::wait::wait(client, json, ids, timeout).await,
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
    Ok(ExitCode::SUCCESS)
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
    let (path, on) = match command {
        TriggerCommand::List => {
            print(json, &client.request("GET", "/v1/triggers", None, None).await?);
            return Ok(());
        }
        TriggerCommand::Fire(on) => ("/v1/triggers/fire", on),
        TriggerCommand::Enable(on) => ("/v1/triggers/enable", on),
        TriggerCommand::Disable(on) => ("/v1/triggers/disable", on),
    };
    let body = json!({ "project": on.project, "rig": on.rig, "trigger": on.trigger });
    let reply = client
        .request("POST", path, Some(&body), on.idempotency.as_deref())
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

/// An `--input` with its `=`: a bare name is a mistake, not an empty value.
fn name_value(text: &str) -> Result<String, String> {
    match text.split_once('=') {
        Some((name, _)) if !name.is_empty() => Ok(text.to_owned()),
        _ => Err(format!("`{text}` is not NAME=VALUE")),
    }
}

/// `name=value`, the value as text unless it is a JSON object or array: the core reads text by the input's type, so
/// `version=1.10` stays `1.10` and `name=null` is the text `null`.
fn input(text: &str) -> (String, Value) {
    let (name, value) = text.split_once('=').unwrap_or((text, ""));
    let parsed = serde_json::from_str(value)
        .ok()
        .filter(|v: &Value| v.is_object() || v.is_array());
    let value = parsed.unwrap_or_else(|| Value::String(value.to_owned()));
    (name.to_owned(), value)
}
