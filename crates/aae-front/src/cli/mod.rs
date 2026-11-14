//! The CLI (spec §12): a thin client of the API, one command per API operation, JSON output with `--json`.
//! Without a daemon only `aae daemon`, `aae doctor`, `aae fetch`, `aae config check|explain` and `aae --version`
//! work.

mod commands;
mod config;
mod finish;

use crate::client::{Client, ClientError, Events};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
pub use commands::{Decision, GateCommand, InstanceCommand, RunCommand, TriggerCommand};
pub use config::{ConfigCommand, ListCommand, local as config_local};
use finish::finish;
use serde_json::{Value, json};
use std::{path::PathBuf, process::ExitCode};

/// Every API operation and the command that does it: `(command, method, path)`. `config check` and `config explain`
/// do it locally, without a daemon.
pub const OPERATIONS: &[(&str, &str, &str)] = &[
    ("run start", "POST", "/v1/runs"),
    ("run list", "GET", "/v1/runs"),
    ("run show", "GET", "/v1/runs/{id}"),
    ("run watch", "GET", "/v1/runs/{id}/live"),
    ("run cancel", "POST", "/v1/runs/{id}/cancel"),
    ("run answer", "POST", "/v1/runs/{id}/answer"),
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
    /// Pin the git packs and plugins of the config to commits in `aae.lock` and check them out.
    Fetch {
        /// The orchestrator config (default: `$XDG_CONFIG_HOME/aae/config.toml`).
        #[arg(long)]
        config: Option<PathBuf>,
        /// Resolve every revision again instead of keeping the commit the lock pins.
        #[arg(long)]
        update: bool,
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
    /// Serve a stdio MCP server over HTTP on `AAE_SERVICE_PORT` behind `AAE_SERVICE_TOKEN`: how the daemon shares one.
    #[command(hide = true)]
    McpBridge {
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
    let error = match runtime.block_on(execute(&client, cli)) {
        Ok(code) => return code,
        Err(ClientError::Io(e)) => {
            let message = format!("no daemon on {} ({e}); start it with `aae daemon`", socket.display());
            json!({ "code": "no_daemon", "message": message })
        }
        Err(e @ ClientError::Api { .. }) if !json => return fail(false, "", &e.to_string()),
        Err(ClientError::Api { error, .. }) => error,
    };
    report(json, error)
}

/// A local error: `aae: <message>`, or under `--json` the API's error object with `code`.
pub fn fail(json: bool, code: &str, message: &str) -> ExitCode {
    report(json, json!({ "code": code, "message": message }))
}

fn report(json: bool, error: Value) -> ExitCode {
    match json {
        true => eprintln!("{}", json!({ "error": error })),
        false => eprintln!("aae: {}", error["message"].as_str().unwrap_or_default()),
    }
    ExitCode::FAILURE
}

async fn execute(client: &Client, cli: Cli) -> Result<ExitCode, ClientError> {
    let json = cli.json;
    match cli.command {
        Command::Daemon { .. }
        | Command::Doctor { .. }
        | Command::Fetch { .. }
        | Command::McpBridge { .. }
        | Command::Config(ConfigCommand::Check { .. } | ConfigCommand::Explain { .. }) => {
            return Ok(fail(
                json,
                "usage",
                "`daemon`, `doctor`, `fetch`, `mcp-bridge` and `config check|explain` run without a daemon, not as its \
                 client",
            ));
        }
        Command::Run(command) => return commands::run(client, json, command).await,
        Command::Gate(command) => commands::gate(client, json, command).await?,
        Command::Instance(command) => commands::instance(client, json, command).await?,
        Command::Trigger(command) => commands::trigger(client, json, command).await?,
        Command::Project(command) => config::list(client, json, "projects", command).await?,
        Command::Agent(command) => config::list(client, json, "agents", command).await?,
        Command::Formula(command) => config::list(client, json, "formulas", command).await?,
        Command::Pack(command) => config::list(client, json, "packs", command).await?,
        Command::Plugin(command) => config::list(client, json, "plugins", command).await?,
        Command::Config(ConfigCommand::Reload { idempotency }) => config::reload(client, json, idempotency).await?,
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

/// Prints the API's JSON: as it is with `--json`, else pretty (and nothing for `null`).
fn print(json: bool, value: &Value) {
    if !json && value.is_null() {
        return;
    }
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

/// Prints a stream until it ends: an event per line, or its data as it is with `--json`. An event with no `seq` (a
/// harness message, `lagged`, `gap`) shows its `type` or `method`, or its data.
async fn follow(mut events: Events, json: bool) -> Result<(), ClientError> {
    while let Some(event) = events.next().await? {
        let data: Value = serde_json::from_str(&event.data).unwrap_or(Value::String(event.data));
        if json {
            println!("{data}");
        } else if event.id.is_some() {
            println!("{}  {}  {}", text(&data["seq"]), event.event, text(&data["run"]));
        } else {
            let what = [&data["type"], &data["method"]].into_iter().find(|v| !v.is_null());
            println!("-  {}  {}", event.event, text(what.unwrap_or(&data)));
        }
    }
    Ok(())
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
