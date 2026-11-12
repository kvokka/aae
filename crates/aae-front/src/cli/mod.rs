//! The CLI (spec §12): a thin client of the API, one command per API operation, JSON output with `--json`.
//! Without a daemon only `aae daemon`, `aae doctor` and `aae --version` work.

mod finish;

use crate::client::{Client, ClientError};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use finish::finish;
use serde_json::{Map, Value, json};
use std::{path::PathBuf, process::ExitCode};

/// Every API operation and the command that calls it: `(command, method, path)`.
pub const OPERATIONS: &[(&str, &str, &str)] = &[
    ("run start", "POST", "/v1/runs"),
    ("run list", "GET", "/v1/runs"),
    ("run show", "GET", "/v1/runs/{id}"),
    ("run cancel", "POST", "/v1/runs/{id}/cancel"),
    ("run answer", "POST", "/v1/runs/{id}/answer"),
    ("finish", "POST", "/v1/runs/{id}/finish"),
    ("gate list", "GET", "/v1/gates"),
    ("gate close", "POST", "/v1/gates"),
    ("gate open", "POST", "/v1/gates/open"),
    ("trigger fire", "POST", "/v1/triggers/fire"),
    ("events", "GET", "/v1/events"),
    ("status", "GET", "/v1/daemon"),
];

#[derive(Parser)]
#[command(
    name = "aae",
    version,
    about = "Adaptive Agent Executor: an event-driven orchestrator for coding agents"
)]
pub struct Cli {
    /// The API socket of the daemon (default: the XDG runtime directory).
    #[arg(long, global = true, env = "AAE_SOCKET")]
    pub socket: Option<PathBuf>,
    /// Print the API's JSON as it is.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the daemon in the foreground.
    Daemon {
        /// The orchestrator config (default: `$XDG_CONFIG_HOME/aae/config.toml`).
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Runs of formulas.
    #[command(subcommand)]
    Run(RunCommand),
    /// Issue gates: pause and resume what starts.
    #[command(subcommand)]
    Gate(GateCommand),
    /// Triggers: fire one by hand.
    #[command(subcommand)]
    Trigger(TriggerCommand),
    /// Follow the events, from `--from` or from now on.
    Events {
        #[arg(long)]
        from: Option<i64>,
    },
    /// The finisher: declare the outcome of this session's attempt (`AAE_RUN`, `AAE_ATTEMPT_TOKEN`).
    Finish {
        result: Verdict,
        #[arg(long)]
        summary: String,
        /// JSON, or `@<file>` with JSON.
        #[arg(long)]
        data: Option<String>,
    },
    /// The daemon's state.
    Status,
}

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

#[derive(Subcommand)]
pub enum TriggerCommand {
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

#[derive(Clone, Copy, ValueEnum)]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Decision {
    Allow,
    Deny,
    Answer,
}

/// The command of an `OPERATIONS` entry exists in the CLI.
pub fn has_command(words: &str) -> bool {
    let mut command = Cli::command();
    words.split(' ').all(|word| {
        command
            .find_subcommand(word)
            .cloned()
            .map(|sub| command = sub)
            .is_some()
    })
}

/// Runs a client command (every command but `daemon`).
pub fn run(cli: Cli) -> ExitCode {
    let socket = match cli.socket.clone().map_or_else(crate::dirs::socket, Ok) {
        Ok(socket) => socket,
        Err(e) => return fail(&e),
    };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(e) => return fail(&format!("runtime: {e}")),
    };
    let client = Client::new(socket.clone());
    match runtime.block_on(execute(&client, cli)) {
        Ok(code) => code,
        Err(ClientError::Io(e)) => fail(&format!(
            "no daemon on {} ({e}); start it with `aae daemon`",
            socket.display()
        )),
        Err(e) => fail(&e.to_string()),
    }
}

pub(crate) fn fail(message: &str) -> ExitCode {
    eprintln!("aae: {message}");
    ExitCode::FAILURE
}

async fn execute(client: &Client, cli: Cli) -> Result<ExitCode, ClientError> {
    let json = cli.json;
    let print = |value: &Value| {
        let text = if json {
            value.to_string()
        } else {
            serde_json::to_string_pretty(value).unwrap_or_default()
        };
        println!("{text}");
    };
    match cli.command {
        Command::Daemon { .. } => return Ok(fail("the daemon runs in its own process, not from a client")),
        Command::Run(RunCommand::Start {
            project,
            rig,
            formula,
            inputs,
            continue_from,
            key,
        }) => {
            let inputs: Map<String, Value> = inputs.iter().map(|i| input(i)).collect();
            let body = json!({ "project": project, "rig": rig, "formula": formula, "inputs": inputs,
                "continue_from": continue_from });
            let reply = client.request("POST", "/v1/runs", Some(&body), key.as_deref()).await?;
            if json {
                print(&reply)
            } else {
                println!("{}", reply["run"].as_str().unwrap_or_default())
            }
        }
        Command::Run(RunCommand::List { project, state, limit }) => {
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
                print(&runs);
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
        Command::Run(RunCommand::Show { id }) => print(&client.request("GET", &run_path(&id, ""), None, None).await?),
        Command::Run(RunCommand::Cancel {
            id,
            reason,
            idempotency,
        }) => {
            let body = json!({ "reason": reason.unwrap_or_default() });
            print(
                &client
                    .request("POST", &run_path(&id, "/cancel"), Some(&body), idempotency.as_deref())
                    .await?,
            );
        }
        Command::Run(RunCommand::Answer {
            id,
            request,
            decision,
            text,
            idempotency,
        }) => {
            let answer = match decision {
                Decision::Allow => json!({ "decision": "allow" }),
                Decision::Deny => json!({ "decision": "deny", "message": text.unwrap_or_default() }),
                Decision::Answer => json!({ "decision": "answer", "text": text.unwrap_or_default() }),
            };
            let body = json!({ "request": request, "answer": answer });
            print(
                &client
                    .request("POST", &run_path(&id, "/answer"), Some(&body), idempotency.as_deref())
                    .await?,
            );
        }
        Command::Gate(GateCommand::List) => {
            let gates = client.request("GET", "/v1/gates", None, None).await?;
            if json {
                print(&gates);
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
        }
        Command::Gate(GateCommand::Close {
            gate,
            until,
            reason,
            idempotency,
        }) => {
            let body = json!({ "key": gate, "until": until, "reason": reason });
            print(
                &client
                    .request("POST", "/v1/gates", Some(&body), idempotency.as_deref())
                    .await?,
            );
        }
        Command::Trigger(TriggerCommand::Fire {
            trigger,
            project,
            rig,
            idempotency,
        }) => {
            let body = json!({ "project": project, "rig": rig, "trigger": trigger });
            print(
                &client
                    .request("POST", "/v1/triggers/fire", Some(&body), idempotency.as_deref())
                    .await?,
            );
        }
        Command::Gate(GateCommand::Open { gate, idempotency }) => {
            let body = json!({ "key": gate });
            print(
                &client
                    .request("POST", "/v1/gates/open", Some(&body), idempotency.as_deref())
                    .await?,
            );
        }
        Command::Events { from } => {
            let mut events = client.events(from).await?;
            while let Some(event) = events.next().await? {
                let envelope: Value = serde_json::from_str(&event.data).unwrap_or(Value::String(event.data));
                if json {
                    println!("{envelope}");
                } else {
                    println!(
                        "{}  {}  {}",
                        text(&envelope["seq"]),
                        event.event,
                        text(&envelope["run"])
                    );
                }
            }
        }
        Command::Finish { result, summary, data } => return finish(client, result, summary, data).await,
        Command::Status => print(&client.request("GET", "/v1/daemon", None, None).await?),
    }
    Ok(ExitCode::SUCCESS)
}

/// `name=value`, the value as JSON if it parses, else as a string.
fn input(text: &str) -> (String, Value) {
    let (name, value) = text.split_once('=').unwrap_or((text, ""));
    let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned()));
    (name.to_owned(), value)
}

pub(crate) fn run_path(id: &str, then: &str) -> String {
    format!("/v1/runs/{}{then}", encode(id))
}

fn text(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_owned)
}

/// Percent-encodes everything but the unreserved characters of RFC 3986.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
