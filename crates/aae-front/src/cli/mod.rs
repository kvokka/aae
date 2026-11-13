//! The CLI (spec §12): a thin client of the API, one command per API operation, JSON output with `--json`.
//! Without a daemon only `aae daemon`, `aae doctor` and `aae --version` work.

mod commands;
mod config;
mod finish;

use crate::client::{Client, ClientError};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
pub use commands::{Decision, GateCommand, InstanceCommand, RunCommand, TriggerCommand};
pub use config::{ConfigCommand, ListCommand};
use finish::finish;
use serde_json::Value;
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
    ("trigger list", "GET", "/v1/triggers"),
    ("project list", "GET", "/v1/projects"),
    ("agent list", "GET", "/v1/agents"),
    ("formula list", "GET", "/v1/formulas"),
    ("pack list", "GET", "/v1/packs"),
    ("plugin list", "GET", "/v1/plugins"),
    ("config explain", "GET", "/v1/config/explain"),
    ("config check", "POST", "/v1/config/check"),
    ("config reload", "POST", "/v1/config/reload"),
    ("events", "GET", "/v1/events"),
    ("status", "GET", "/v1/daemon"),
    ("drain", "POST", "/v1/daemon/drain"),
    ("restart", "POST", "/v1/daemon/restart"),
    ("instance list", "GET", "/v1/instances"),
    ("instance stop", "POST", "/v1/instances/{id}/stop"),
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
    pub json: bool,
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
        /// Print a service file that runs this daemon (a systemd user unit or a launchd agent) and exit.
        #[arg(long)]
        service: Option<Service>,
    },
    /// Check what the daemon needs: harness and br binaries, versions and logins, directories, the state.
    Doctor {
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
    /// Harness instances: list them, stop an idle one.
    #[command(subcommand)]
    Instance(InstanceCommand),
    /// Triggers: list them, fire one by hand.
    #[command(subcommand)]
    Trigger(TriggerCommand),
    /// Projects and their rigs.
    #[command(subcommand)]
    Project(ListCommand),
    /// Agents.
    #[command(subcommand)]
    Agent(ListCommand),
    /// Formulas.
    #[command(subcommand)]
    Formula(ListCommand),
    /// Packs.
    #[command(subcommand)]
    Pack(ListCommand),
    /// Plugins: declared and bundled.
    #[command(subcommand)]
    Plugin(ListCommand),
    /// The config: check, explain, reload.
    #[command(subcommand)]
    Config(ConfigCommand),
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
    /// Stop the daemon once the runs in a turn are done (no run starts meanwhile).
    Drain {
        /// Seconds the runs in a turn may go on; the daemon stops then anyway (default 300).
        #[arg(long)]
        timeout: Option<u64>,
        /// The idempotency key: a repeat with the same key does nothing new.
        #[arg(long)]
        key: Option<String>,
    },
    /// Drain the daemon, then start it again in the same process.
    Restart {
        /// Seconds the runs in a turn may go on; the daemon restarts then anyway (default 300).
        #[arg(long)]
        timeout: Option<u64>,
        /// The idempotency key: a repeat with the same key does nothing new.
        #[arg(long)]
        key: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Service {
    Systemd,
    Launchd,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Verdict {
    Pass,
    Fail,
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
    match cli.command {
        Command::Daemon { .. } | Command::Doctor { .. } => {
            return Ok(fail("`daemon` and `doctor` run without a daemon, not as its client"));
        }
        Command::Run(command) => commands::run(client, json, command).await?,
        Command::Gate(command) => commands::gate(client, json, command).await?,
        Command::Instance(command) => commands::instance(client, json, command).await?,
        Command::Trigger(command) => commands::trigger(client, json, command).await?,
        Command::Project(command) => config::list(client, json, "projects", command).await?,
        Command::Agent(command) => config::list(client, json, "agents", command).await?,
        Command::Formula(command) => config::list(client, json, "formulas", command).await?,
        Command::Pack(command) => config::list(client, json, "packs", command).await?,
        Command::Plugin(command) => config::list(client, json, "plugins", command).await?,
        Command::Config(command) => return config::config(client, json, command).await,
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
        Command::Status => print(json, &client.request("GET", "/v1/daemon", None, None).await?),
        Command::Drain { timeout, key } => print(
            json,
            &client
                .request("POST", &stop_path("drain", timeout), None, key.as_deref())
                .await?,
        ),
        Command::Restart { timeout, key } => print(
            json,
            &client
                .request("POST", &stop_path("restart", timeout), None, key.as_deref())
                .await?,
        ),
    }
    Ok(ExitCode::SUCCESS)
}

/// Prints the API's JSON: as it is with `--json`, else pretty.
fn print(json: bool, value: &Value) {
    let text = if json {
        value.to_string()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_default()
    };
    println!("{text}");
}

fn stop_path(what: &str, timeout: Option<u64>) -> String {
    let query = timeout.map_or_else(String::new, |t| format!("?timeout_s={t}"));
    format!("/v1/daemon/{what}{query}")
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
