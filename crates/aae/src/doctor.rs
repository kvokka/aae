//! `aae doctor` (spec §13), without a daemon or from it (`GET /v1/doctor`): the harness and br binaries with their versions and logins, the
//! private directories, the state and its outbox; and `aae daemon --service`, the service file that runs the daemon.

use crate::{
    daemon::Paths,
    harness::{binary, driver, load, tested},
};
use aae_core::config::{Config, Name};
use aae_front::cli::Service;
use serde_json::{Value, json};
use std::{
    env,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// An outbox row older than this means the journal does not keep up (contracts 3.5).
const OUTBOX_STALE_MS: i64 = 60_000;

/// One finding: `level` is `ok`, `warning` or `error`.
fn check(what: impl Into<String>, level: &str, detail: impl Into<String>) -> Value {
    json!({ "check": what.into(), "level": level, "detail": detail.into() })
}

/// Prints every finding; the exit code fails when one is an error.
pub(crate) fn run(paths: &Paths, json: bool) -> ExitCode {
    let found = findings(paths);
    if json {
        println!("{}", Value::from(found.clone()));
    } else {
        for f in &found {
            let level = f["level"].as_str().unwrap_or_default();
            println!("{level:<8} {:<16} {}", text(&f["check"]), text(&f["detail"]));
        }
    }
    match found.iter().any(|f| f["level"] == "error") {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    }
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

/// Every check, in the order `aae doctor` prints them; `GET /v1/doctor` makes them from the daemon.
pub(crate) fn findings(paths: &Paths) -> Vec<Value> {
    let mut found = Vec::new();
    match load(paths) {
        Err(errors) => found.extend(errors.into_iter().map(|e| check("config", "error", e))),
        Ok(config) => {
            found.push(check("config", "ok", paths.config.display().to_string()));
            for (name, harness) in &config.harnesses {
                if let Some((driver, path, source)) = driver(name, harness, paths) {
                    found.push(binary_check(name, &path, source, tested(&driver)));
                    found.push(login(name));
                    found.push(host_mcp(name));
                }
                if name == "codex" {
                    found.push(sandbox(&config));
                }
            }
            if config.projects.iter().any(|p| p.root.join(".beads").is_dir()) {
                let (path, source) = binary("br", None);
                found.push(binary_check("br", &path, source, aae_br::TESTED));
                found.push(check(
                    "br license",
                    "warning",
                    "br is MIT with an OpenAI/Anthropic rider, not an OSI license; aae only runs the installed \
                     binary — check that the rider fits your use",
                ));
            }
        }
    }
    for dir in [Some(paths.state.as_path()), paths.socket.parent()]
        .into_iter()
        .flatten()
    {
        found.push(private(dir));
    }
    found.push(state(&paths.state.join("state.db")));
    found
}

/// Spec §2.1 item 1: the codex sandbox needs user namespaces, which many hosts lack, so a codex agent whose config
/// sets no `sandbox_mode` runs with none; the warning names every such agent, per rig.
fn sandbox(config: &Config) -> Value {
    let host = home("CODEX_HOME", ".codex");
    let (mut open, mut seen) = (Vec::new(), 0);
    for p in &config.projects {
        let rigs: Vec<Option<&str>> = match p.rigs.is_empty() {
            true => vec![None],
            false => p.rigs.keys().map(|r| Some(r.as_str())).collect(),
        };
        let agents = (p.packs.iter()).flat_map(|pack| pack.agents.iter().map(|(a, agent)| (&pack.name, a, agent)));
        for (pack, name, _) in agents.filter(|(_, _, agent)| agent.harness == "codex") {
            let agent = Name {
                pack: pack.clone(),
                name: name.clone(),
            };
            for rig in &rigs {
                seen += 1;
                // A config that does not assemble cannot be shown to set it, so the agent is named too.
                let effective = config.effective(&p.name, *rig, &agent);
                if effective.map_or(true, |e| aae_codex::unconfined(&e, &host)) {
                    open.push(format!("{}/{}@{agent}", p.name, rig.unwrap_or("-")));
                }
            }
        }
    }
    match (seen, open.is_empty()) {
        (0, _) => check("codex sandbox", "ok", "no codex agents"),
        (_, true) => check("codex sandbox", "ok", "every codex agent sets sandbox_mode"),
        (_, false) => check(
            "codex sandbox",
            "warning",
            format!(
                "sandbox_mode = \"danger-full-access\", commands not confined to the workdir: {}",
                open.join(", ")
            ),
        ),
    }
}

/// The stdio MCP servers of the host's own settings (spec §6.4): with host inheritance on, each starts in every
/// session, and the shared MCP of `aae` does not cover them.
fn host_mcp(harness: &str) -> Value {
    let read = |path: PathBuf| std::fs::read_to_string(path).unwrap_or_default();
    let mut names: Vec<String> = match harness {
        "claude" => {
            let user: Value = serde_json::from_str(&read(claude_json())).unwrap_or_default();
            let projects = user["projects"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(_, p)| &p["mcpServers"]);
            std::iter::once(&user["mcpServers"])
                .chain(projects)
                .filter_map(Value::as_object)
                .flatten()
                .filter(|(_, s)| !s["command"].is_null())
                .map(|(n, _)| n.clone())
                .collect()
        }
        _ => {
            let codex: toml::Table =
                toml::from_str(&read(home("CODEX_HOME", ".codex").join("config.toml"))).unwrap_or_default();
            let servers = codex
                .get("mcp_servers")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flatten();
            servers
                .filter(|(_, s)| s.get("command").is_some())
                .map(|(n, _)| n.clone())
                .collect()
        }
    };
    names.sort();
    names.dedup();
    match names.is_empty() {
        true => check(
            format!("{harness} host mcp"),
            "ok",
            "no stdio MCP server in the host settings",
        ),
        false => check(
            format!("{harness} host mcp"),
            "warning",
            format!(
                "stdio MCP servers of the host settings start in every session that inherits the host: {}; \
                 declare them in the aae config, where they are shared, or set inherit_host = false",
                names.join(", ")
            ),
        ),
    }
}

/// The user file of claude: in `$CLAUDE_CONFIG_DIR` when set, else `~/.claude.json`.
fn claude_json() -> PathBuf {
    env::var_os("CLAUDE_CONFIG_DIR").map_or_else(
        || PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".claude.json"),
        |d| PathBuf::from(d).join(".claude.json"),
    )
}

/// `$<var>`, else `~/<dir>`.
fn home(var: &str, dir: &str) -> PathBuf {
    env::var_os(var).map_or_else(
        || PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(dir),
        PathBuf::from,
    )
}

/// The binary exists and its version is one the adapter was tested on.
fn binary_check(name: &str, path: &Path, source: &str, versions: [(u32, u32, u32); 2]) -> Value {
    let Some(path) = on_disk(path) else {
        return check(name, "error", format!("{} (from {source}): not found", path.display()));
    };
    match aae_core::tested_version(&path, versions) {
        Ok(version) => check(name, "ok", format!("{} `{version}` (from {source})", path.display())),
        Err(why) => check(name, "warning", why),
    }
}

/// `path` if it is a file, or where a bare name is found on `PATH`.
fn on_disk(path: &Path) -> Option<PathBuf> {
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_owned());
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(path))
        .find(|p| p.is_file())
}

/// The host login a harness process starts with, unless its agent's env carries a key of its own.
fn login(harness: &str) -> Value {
    let (file, key) = match harness {
        "claude" => (
            home("CLAUDE_CONFIG_DIR", ".claude").join(".credentials.json"),
            "ANTHROPIC_API_KEY".to_owned(),
        ),
        _ => {
            let codex = home("CODEX_HOME", ".codex");
            // A host provider with `env_key` takes its key from there, not from the login.
            let key = aae_codex::host_key(&codex).unwrap_or_else(|| "OPENAI_API_KEY".to_owned());
            (codex.join("auth.json"), key)
        }
    };
    let what = format!("{harness} login");
    match (file.is_file(), env::var_os(&key).is_some()) {
        (true, _) => check(what, "ok", file.display().to_string()),
        (false, true) => check(what, "ok", format!("{key} is set")),
        (false, false) => check(
            what,
            "warning",
            format!(
                "no {} and no {key}: log in, or give the agents a key in `env`",
                file.display()
            ),
        ),
    }
}

/// A directory of the daemon is the owner's only; one not made yet is made so at start.
fn private(dir: &Path) -> Value {
    let what = format!("dir {}", dir.display());
    match std::fs::metadata(dir) {
        Err(_) => check(what, "ok", "made with mode 0700 at start"),
        Ok(meta) if meta.permissions().mode() & 0o077 == 0 => check(what, "ok", "mode 0700"),
        Ok(meta) => check(
            what,
            "error",
            format!(
                "mode {:o}: others can read it; chmod 700",
                meta.permissions().mode() & 0o777
            ),
        ),
    }
}

/// The state opens, and its outbox drains into the journal.
fn state(db: &Path) -> Value {
    if !db.exists() {
        return check("state", "ok", "no state yet: made at start");
    }
    let status = aae_core::run::Reader::open(db).and_then(|r| r.status());
    let status = match status {
        Ok(status) => status,
        Err(e) => return check("state", "error", format!("{}: {e}", db.display())),
    };
    let depth = status["outbox_depth"].as_i64().unwrap_or_default();
    let age = status["outbox_oldest_ms"]
        .as_i64()
        .map_or(0, |t| aae_core::Clock::system().now_ms() - t);
    let detail = format!("outbox: {depth} rows, oldest {} s", age / 1000);
    // A running daemon drains it at once; an old row means the journal fails, or no daemon ran since.
    let level = if age > OUTBOX_STALE_MS { "warning" } else { "ok" };
    check("state", level, detail)
}

/// The service file that runs `aae daemon` with `config` (absolute), and the `PATH` of this shell so the daemon
/// finds mise (spec §6.5). The daemon is not started: the file is printed for the user to install.
pub(crate) fn service(kind: Service, config: Option<PathBuf>) -> ExitCode {
    let exe = env::current_exe().map_or_else(|_| "aae".into(), |p| p.display().to_string());
    let mut args = vec![exe, "daemon".to_owned()];
    if let Some(config) = config {
        let config = std::path::absolute(&config).unwrap_or(config);
        args.extend(["--config".to_owned(), config.display().to_string()]);
    }
    let path = env::var("PATH").unwrap_or_default();
    print!("{}", unit(kind, &args, &path));
    ExitCode::SUCCESS
}

fn unit(kind: Service, args: &[String], path: &str) -> String {
    match kind {
        Service::Systemd => {
            let quote = |s: &str| {
                let s = s.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{}\"", s.replace('%', "%%").replace('$', "$$"))
            };
            let exec: Vec<String> = args.iter().map(|a| quote(a)).collect();
            format!(
                "# ~/.config/systemd/user/aae.service; then: systemctl --user enable --now aae\n\
                 [Unit]\nDescription=aae daemon\n\n[Service]\nExecStart={}\nEnvironment={}\n\
                 Restart=on-failure\n\n[Install]\nWantedBy=default.target\n",
                exec.join(" "),
                quote(&format!("PATH={path}"))
            )
        }
        Service::Launchd => {
            let xml = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            let program: String = args
                .iter()
                .map(|a| format!("    <string>{}</string>\n", xml(a)))
                .collect();
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!-- ~/Library/LaunchAgents/dev.aae.daemon.plist; then: launchctl load -w <that file> -->\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>dev.aae.daemon</string>\n\
                 \x20 <key>ProgramArguments</key>\n  <array>\n{program}  </array>\n\
                 \x20 <key>EnvironmentVariables</key>\n  <dict>\n    <key>PATH</key>\n    <string>{}</string>\n  </dict>\n\
                 \x20 <key>RunAtLoad</key>\n  <true/>\n\
                 \x20 <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
                 </dict>\n</plist>\n",
                xml(path)
            )
        }
    }
}
