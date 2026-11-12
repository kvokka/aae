//! The effective config of a session split into the app-server process and the thread (docs/design/contracts.md
//! 4.6, docs/design/experiments.md "Config and credential delivery"): `CODEX_HOME`, `HOME`, the provider and the
//! process environment are the process's; cwd, model, instructions, MCP, hooks and the command environment are the
//! thread's.

use aae_core::{
    config::{EffectiveConfig, McpScope},
    driver::{Failure, FailureKind, Fingerprint, ProcessPart, SessionPlan, Source},
};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The variables that carry a credential; a process with one of them uses it, not the host's login.
const CREDENTIALS: [&str; 2] = ["OPENAI_API_KEY", "CODEX_API_KEY"];

fn config_error(raw: String) -> Failure {
    let mut failure = Failure::new(FailureKind::ConfigError, Source::Adapter);
    failure.raw = raw;
    failure
}

/// The process part: `{home, user_home, env, overrides}`. An isolated process gets its own `CODEX_HOME` and `HOME`
/// under the state directory, keyed by the rest of the part, so equal parts share one app-server and its threads
/// survive a daemon restart; `overrides` are the `-c key=value` flags of the native block.
pub(crate) fn process(config: &EffectiveConfig, state: &Path) -> Result<Value, Failure> {
    // shortcut: plugins (WP-32) and skills (WP-29b, `CODEX_HOME/skills`) are refused rather than run without them.
    if !config.plugins.is_empty() || config.skills.values().any(|s| s.value.enabled) {
        return Err(config_error(
            "the codex adapter does not deliver plugins or skills yet".into(),
        ));
    }
    let overrides = match config.native.as_ref().map(|n| &n.value) {
        None => Vec::new(),
        Some(Value::Object(native)) => native
            .iter()
            .map(|(key, value)| {
                let value = toml::Value::try_from(value).map_err(|e| config_error(format!("native `{key}`: {e}")))?;
                Ok(format!("{key}={value}"))
            })
            .collect::<Result<_, Failure>>()?,
        Some(_) => {
            return Err(config_error(
                "the codex native block is a table of codex config keys".into(),
            ));
        }
    };
    let env: BTreeMap<_, _> = config.env.iter().map(|(k, v)| (k.clone(), v.value.clone())).collect();
    let mut part = json!({ "env": env, "overrides": overrides });
    if config.inherits_host() {
        part["home"] = json!(host_home());
        return Ok(part);
    }
    let dir = state
        .join("codex")
        .join(&Fingerprint::of("codex", &ProcessPart(part.clone())).0[..16]);
    part["home"] = json!(dir.join("codex-home"));
    part["user_home"] = json!(dir.join("home"));
    Ok(part)
}

/// The `thread/start` (and `thread/resume`) parameters of a session.
pub(crate) fn session(config: &EffectiveConfig, plan: &SessionPlan) -> Result<Value, Failure> {
    let (workdir, run, attempt, token) = match plan {
        SessionPlan::New {
            workdir,
            run,
            attempt,
            token,
        }
        | SessionPlan::Resume {
            workdir,
            run,
            attempt,
            token,
            ..
        } => (workdir, run, attempt, token),
    };
    let value = |s: &Option<aae_core::config::Sourced<String>>| s.as_ref().map(|s| s.value.clone());
    let mut thread_config = json!({
        "shell_environment_policy": { "set": { "AAE_RUN": run, "AAE_ATTEMPT_TOKEN": token,
            "BD_ACTOR": format!("aae:{run}:{attempt}") } },
    });
    if let Some(effort) = value(&config.effort) {
        thread_config["model_reasoning_effort"] = json!(effort);
    }
    let mcp = mcp(config)?;
    if !mcp.is_empty() {
        thread_config["mcp_servers"] = Value::Object(mcp);
    }
    let hooks = hooks(config)?;
    if !hooks.is_empty() {
        // Hooks given to a thread run only with the bypass (experiment 2).
        thread_config["hooks"] = Value::Object(hooks);
        thread_config["bypass_hook_trust"] = json!(true);
    }
    let mut params = json!({ "cwd": workdir, "config": thread_config });
    let fields = [
        ("model", value(&config.model)),
        ("developerInstructions", value(&config.system_prompt)),
        // The codex approval policy: `untrusted`, `on-request` or `never`.
        ("approvalPolicy", value(&config.permission_mode)),
    ];
    for (key, value) in fields {
        if let Some(value) = value {
            params[key] = json!(value);
        }
    }
    Ok(params)
}

/// `config.mcp_servers` of the thread.
fn mcp(config: &EffectiveConfig) -> Result<Map<String, Value>, Failure> {
    let mut servers = Map::new();
    for (name, server) in &config.mcp {
        let server = &server.value;
        // shortcut: shared servers (scope orchestrator or project) come with the shared MCP of WP-34; until then
        // they are refused rather than started per session.
        if matches!(server.scope, Some(McpScope::Orchestrator | McpScope::Project)) {
            return Err(config_error(format!(
                "MCP server `{name}`: shared scopes are not delivered yet"
            )));
        }
        let entry = match (&server.command, &server.url) {
            (Some(command), None) if !command.is_empty() => {
                json!({ "command": command[0], "args": command[1..], "env": server.env })
            }
            (None, Some(url)) => json!({ "url": url, "http_headers": server.headers }),
            _ => return Err(config_error(format!("MCP server `{name}` needs a command or a url"))),
        };
        servers.insert(name.clone(), entry);
    }
    Ok(servers)
}

/// `config.hooks` of the thread: the `hooks.json` form, the same as claude's.
fn hooks(config: &EffectiveConfig) -> Result<Map<String, Value>, Failure> {
    let mut events = Map::new();
    for (name, hook) in config.hooks.iter().filter(|(_, h)| h.value.enabled) {
        let hook = &hook.value;
        let (Some(event), Some(command)) = (&hook.event, &hook.command) else {
            return Err(config_error(format!("hook `{name}` needs an event and a command")));
        };
        let mut entry = json!({ "type": "command", "command": command });
        if let Some(timeout) = &hook.timeout {
            let timeout =
                aae_core::config::duration(timeout).map_err(|e| config_error(format!("hook `{name}`: {e}")))?;
            entry["timeout"] = json!(timeout.as_secs().max(1));
        }
        let matcher = json!({ "matcher": hook.matcher.clone().unwrap_or_default(), "hooks": [entry] });
        if let Value::Array(list) = events.entry(event.clone()).or_insert_with(|| json!([])) {
            list.push(matcher);
        }
    }
    Ok(events)
}

/// The arguments of `codex` for the process part `p`.
pub(crate) fn args(p: &Value) -> Vec<String> {
    let mut args = vec![
        "app-server".to_owned(),
        "-c".to_owned(),
        "analytics.enabled=false".to_owned(),
    ];
    for o in p["overrides"].as_array().into_iter().flatten() {
        args.extend(["-c".to_owned(), o.as_str().unwrap_or_default().to_owned()]);
    }
    args
}

/// The environment of the app-server: the daemon's `PATH`, `HOME`, `LANG`, `SHELL`, the agent's variables, the
/// homes, and the instance id the orphan sweep looks for. The attempt variables go to each thread instead.
pub(crate) fn env(p: &Value, id: &str, socket: Option<&Path>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = ["PATH", "HOME", "LANG", "SHELL"]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_owned(), v)))
        .collect();
    let text = |v: &Value| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string());
    env.extend(
        p["env"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), text(v))),
    );
    env.push(("CODEX_HOME".to_owned(), text(&p["home"])));
    env.extend(p["user_home"].as_str().map(|h| ("HOME".to_owned(), h.to_owned())));
    env.push((aae_core::orphans::INSTANCE_ENV.to_owned(), id.to_owned()));
    env.extend(socket.map(|s| ("AAE_SOCKET".to_owned(), s.to_string_lossy().into_owned())));
    env
}

/// Creates the homes of an isolated process; without a credential of its own it gets the host's login, linked so
/// a token codex refreshes is the host's too.
pub(crate) fn materialize(p: &Value) -> std::io::Result<()> {
    let Some(user_home) = p["user_home"].as_str() else {
        return Ok(());
    };
    let home = Path::new(p["home"].as_str().unwrap_or_default());
    std::fs::create_dir_all(user_home)?;
    std::fs::create_dir_all(home)?;
    let auth = host_home().join("auth.json");
    let own = CREDENTIALS.iter().any(|k| !p["env"][k].is_null());
    let link = home.join("auth.json");
    if !own && auth.exists() && !link.exists() {
        std::os::unix::fs::symlink(auth, link)?;
    }
    Ok(())
}

/// The host's codex home: the daemon's `CODEX_HOME`, else `~/.codex`.
fn host_home() -> PathBuf {
    match std::env::var_os("CODEX_HOME") {
        Some(dir) => dir.into(),
        None => Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".codex"),
    }
}

/// The account the gates use: a hash of where the credential comes from, never the credential itself (§5.7).
pub(crate) fn account(p: &Value) -> String {
    let env = &p["env"];
    let source = match CREDENTIALS.iter().find(|k| !env[**k].is_null()) {
        Some(key) => json!({ *key: env[*key] }),
        None => json!({ "host": host_home() }),
    };
    Fingerprint::of("codex", &ProcessPart(source)).0[..16].to_owned()
}
