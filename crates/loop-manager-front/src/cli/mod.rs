//! The CLI (docs/api/frontends.md): a thin client of the API, one command per API operation, TOON output by default.
//! Without a daemon only `loop-manager daemon`, `loop-manager cache clear`, `loop-manager --version`, and `loop-manager doctor` and `loop-manager config
//! check|explain` on a file named with `--config` work.

mod commands;
mod config;
mod finish;
mod wait;

use crate::client::{Client, ClientError, Events};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
pub use commands::{GateCommand, InstanceCommand, RunCommand, TriggerCommand};
pub use config::{ConfigCommand, FormulaCommand, ListCommand, local as config_local};
use finish::finish;
use serde_json::{Value, json};
use std::{path::PathBuf, process::ExitCode};

/// Every API operation and the command that does it: `(command, method, path)`. `config check`, `config explain` and
/// `doctor` with `--config <file>` do it on that file, without a daemon.
pub const OPERATIONS: &[(&str, &str, &str)] = &[
    ("run start", "POST", "/v1/runs"),
    ("run list", "GET", "/v1/runs"),
    ("run show", "GET", "/v1/runs/{id}"),
    ("run watch", "GET", "/v1/runs/{id}/live"),
    ("run cancel", "POST", "/v1/runs/{id}/cancel"),
    ("run resolve", "POST", "/v1/runs/{id}/resolve"),
    ("finish", "POST", "/v1/runs/{id}/finish"),
    ("gate list", "GET", "/v1/gates"),
    ("gate close", "POST", "/v1/gates"),
    ("gate open", "POST", "/v1/gates/open"),
    ("trigger fire", "POST", "/v1/triggers/fire"),
    ("trigger list", "GET", "/v1/triggers"),
    ("trigger enable", "POST", "/v1/triggers/enable"),
    ("trigger disable", "POST", "/v1/triggers/disable"),
    ("project list", "GET", "/v1/projects"),
    ("agent list", "GET", "/v1/agents"),
    ("formula list", "GET", "/v1/formulas"),
    ("pack list", "GET", "/v1/packs"),
    ("addon list", "GET", "/v1/addons"),
    ("config explain", "GET", "/v1/config/explain"),
    ("config check", "POST", "/v1/config/check"),
    ("config reload", "POST", "/v1/config/reload"),
    ("events", "GET", "/v1/events"),
    ("status", "GET", "/v1/daemon"),
    ("drain", "POST", "/v1/daemon/drain"),
    ("restart", "POST", "/v1/daemon/restart"),
    ("doctor", "GET", "/v1/doctor"),
    ("instance list", "GET", "/v1/instances"),
    ("instance stop", "POST", "/v1/instances/{id}/stop"),
];

#[derive(Parser)]
#[command(
    name = "loop-manager",
    version,
    about = "An event-driven orchestrator for coding agents and the loops they work in"
)]
pub struct Cli {
    /// The API socket of the daemon (default: the XDG runtime directory).
    #[arg(long, global = true, env = "LOOP_MANAGER_SOCKET")]
    pub socket: Option<PathBuf>,
    /// Print structured replies as JSON (default: TOON).
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Remove every downloaded pack and addon; the next load of the config downloads them again.
    Clear,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the daemon in the foreground.
    Daemon {
        /// The orchestrator config (default: `$XDG_CONFIG_HOME/loop-manager/config.toml`).
        #[arg(long)]
        config: Option<PathBuf>,
        /// Print a service file that runs this daemon (a systemd user unit or a launchd agent) and exit.
        #[arg(long)]
        service: Option<Service>,
    },
    /// Check what the daemon needs: harness and br binaries, versions and logins, directories, the state.
    Doctor {
        /// Check this file without a daemon.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// The cache of packs and addons downloaded from git.
    #[command(subcommand)]
    Cache(CacheCommand),
    /// Runs of formulas.
    #[command(subcommand)]
    Run(RunCommand),
    /// Issue gates: pause and resume what starts.
    #[command(subcommand)]
    Gate(GateCommand),
    /// Harness instances: list them, stop an idle one.
    #[command(subcommand)]
    Instance(InstanceCommand),
    /// Triggers: list them, fire one by hand, switch one off and on.
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
    Formula(FormulaCommand),
    /// Packs.
    #[command(subcommand)]
    Pack(ListCommand),
    /// Addons: declared and bundled.
    #[command(subcommand)]
    Addon(ListCommand),
    /// The config: check, explain, reload.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Follow the events, from `--from` or from now on.
    Events {
        #[arg(long)]
        from: Option<i64>,
    },
    /// The finisher: declare the outcome of this session's attempt (`LOOP_MANAGER_RUN`, `LOOP_MANAGER_ATTEMPT_TOKEN`).
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
    /// Serve a stdio MCP server over HTTP on `LOOP_MANAGER_SERVICE_PORT` behind `LOOP_MANAGER_SERVICE_TOKEN`: how the daemon shares one.
    #[command(hide = true)]
    McpBridge {
        /// How long a request may wait for the server's answer, in seconds; then the caller gets an error.
        #[arg(long, default_value_t = 600)]
        timeout: u64,
        /// The command of the stdio MCP server.
        #[arg(last = true, required = true)]
        command: Vec<String>,
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
        Err(e) => return fail(cli.json, "config_error", &e),
    };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(e) => return fail(cli.json, "internal", &format!("runtime: {e}")),
    };
    let (client, json) = (Client::new(socket.clone()), cli.json);
    let checks = matches!(
        cli.command,
        Command::Doctor { .. } | Command::Config(ConfigCommand::Check { .. } | ConfigCommand::Explain { .. })
    );
    let error = match runtime.block_on(execute(&client, cli)) {
        Ok(code) => return code,
        Err(ClientError::Io(e)) => {
            let mut message = format!(
                "no daemon on {} ({e}); start it with `loop-manager daemon`",
                socket.display()
            );
            if checks {
                message += ", or check a config file without one: `--config <file>`";
            }
            json!({ "code": "no_daemon", "message": message })
        }
        Err(ClientError::Api { error, .. }) => error,
    };
    report(json, error)
}

/// Prints a local error on stderr in the selected format.
pub fn fail(json: bool, code: &str, message: &str) -> ExitCode {
    report(json, json!({ "code": code, "message": message }))
}

fn report(json: bool, error: Value) -> ExitCode {
    eprintln!("{}", render(json, &json!({ "error": error })));
    ExitCode::FAILURE
}

async fn execute(client: &Client, cli: Cli) -> Result<ExitCode, ClientError> {
    let json = cli.json;
    match cli.command {
        Command::Daemon { .. } | Command::Cache(_) | Command::McpBridge { .. } => {
            let message = "`daemon`, `cache clear` and `mcp-bridge` run without a daemon, not as its client";
            return Ok(fail(json, "usage", message));
        }
        Command::Doctor { .. } => return Ok(doctor(json, &client.request("GET", "/v1/doctor", None, None).await?)),
        Command::Config(command) => return config::config(client, json, command).await,
        Command::Run(command) => return commands::run(client, json, command).await,
        Command::Gate(command) => commands::gate(client, json, command).await?,
        Command::Instance(command) => commands::instance(client, json, command).await?,
        Command::Trigger(command) => commands::trigger(client, json, command).await?,
        Command::Project(command) => config::list(client, json, "projects", command).await?,
        Command::Agent(command) => config::list(client, json, "agents", command).await?,
        Command::Formula(command) => config::formulas(client, json, command).await?,
        Command::Pack(command) => config::list(client, json, "packs", command).await?,
        Command::Addon(command) => config::list(client, json, "addons", command).await?,
        Command::Events { from } => follow(client.events(from).await?, json).await?,
        Command::Finish { result, summary, data } => return finish(client, json, result, summary, data).await,
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

/// Prints the findings of `loop-manager doctor`, the daemon's or a file's alike; fails when one is an error.
pub fn doctor(json: bool, found: &Value) -> ExitCode {
    print(json, found);
    match found
        .as_array()
        .is_some_and(|items| items.iter().any(|f| f["level"] == "error"))
    {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    }
}

fn render(json: bool, value: &Value) -> String {
    if json {
        value.to_string()
    } else {
        loop_manager_core::toon::encode(value)
    }
}

/// Prints one structured CLI reply.
pub fn print(json: bool, value: &Value) {
    println!("{}", render(json, value));
}

fn stop_path(what: &str, timeout: Option<u64>) -> String {
    let query = timeout.map_or_else(String::new, |t| format!("?timeout_s={t}"));
    format!("/v1/daemon/{what}{query}")
}

pub(crate) fn run_path(id: &str, then: &str) -> String {
    format!("/v1/runs/{}{then}", encode(id))
}

/// Emits a TOON document per event, separated by a blank line. Journal envelopes carry their type;
/// other SSE events (harness messages, gaps) carry it only in the SSE event field. JSON streams are raw data as NDJSON.
async fn follow(mut events: Events, json: bool) -> Result<(), ClientError> {
    while let Some(event) = events.next().await? {
        let mut data: Value = serde_json::from_str(&event.data).unwrap_or(Value::String(event.data));
        if !json && event.id.is_none() {
            data = json!({ "event": event.event, "data": data });
        }
        print(json, &data);
        if !json {
            println!();
        }
    }
    Ok(())
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
