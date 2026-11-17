//! The effective config of a session split into the app-server process and the thread (docs/design/contracts.md
//! 4.6, docs/design/experiments.md "Config and credential delivery"): `CODEX_HOME`, `HOME`, the provider and the
//! process environment are the process's; cwd, model, instructions, MCP, hooks and the command environment are the
//! thread's.

use aae_contract::{
    config::EffectiveConfig,
    driver::{Failure, Fingerprint, ProcessPart, SessionPlan, child::config_error},
};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The variables that carry a credential; a process with one of them uses it, not the host's login.
pub(crate) const CREDENTIALS: [&str; 2] = ["OPENAI_API_KEY", "CODEX_API_KEY"];

/// The sandbox a process runs in when no config sets one (decision 38); `doctor` warns about it.
pub(crate) const DEFAULT_SANDBOX: &str = "danger-full-access";
/// The approval policy of a thread when no config sets one: the harness decides every permission and nothing waits for
/// a person (decision 117).
const DEFAULT_APPROVAL: &str = "never";
/// The native key of the approval policy, codex's own config key.
const APPROVAL: &str = "approval_policy";

/// The process part: `{home, user_home, host, env, overrides, skills}`. An isolated process gets its own `CODEX_HOME`
/// and `HOME` under the state directory, keyed by the rest of the part, so equal parts share one app-server and its
/// threads survive a daemon restart; a process inheriting the host with skills gets its own `CODEX_HOME` there that
/// links the host's (`host`); `overrides` are the `-c key=value` flags of the native block.
pub(crate) fn process(config: &EffectiveConfig, state: &Path, host: &Path) -> Result<Value, Failure> {
    // The contributions of attached plugins are already merged into the config (layer 5 of docs/design/config.md).
    let mut overrides: Vec<String> = match config.native.as_ref().map(|n| &n.value) {
        None => Vec::new(),
        // The approval policy is the thread's (`session`), not the process's.
        Some(Value::Object(native)) => (native.iter())
            .filter(|(key, _)| *key != APPROVAL)
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
    if unconfined(config, host) {
        overrides.push(format!("sandbox_mode={}", toml::Value::from(DEFAULT_SANDBOX)));
    }
    let env: BTreeMap<_, _> = config.env.iter().map(|(k, v)| (k.clone(), v.value.clone())).collect();
    let own = CREDENTIALS.iter().any(|k| env.contains_key(*k));
    let mut part = json!({ "env": env, "overrides": overrides });
    // Skills are the process's: codex reads them from `CODEX_HOME/skills`, with no thread field for them.
    let skills: BTreeMap<_, _> = (config.skills.iter())
        .filter(|(_, s)| s.value.enabled)
        .filter_map(|(name, s)| Some((name.clone(), s.value.path.clone()?)))
        .collect();
    if !skills.is_empty() {
        part["skills"] = json!(skills);
    }
    if config.inherits_host() {
        host_provider(&mut part, host, false);
        if skills.is_empty() {
            part["home"] = json!(host);
            return Ok(part);
        }
        // Skills need a home of the process's own: it links the host's login and config (decision 46).
        part["host"] = json!(host);
    } else if !own {
        host_provider(&mut part, host, true);
    }
    let dir = state
        .join("codex")
        .join(&Fingerprint::of("codex", &ProcessPart(part.clone())).0[..16]);
    part["home"] = json!(dir.join("codex-home"));
    if part["host"].is_null() {
        part["user_home"] = json!(dir.join("home"));
    }
    Ok(part)
}

/// The `thread/start` (and `thread/resume`) parameters of a session.
pub(crate) fn session(config: &EffectiveConfig, plan: &SessionPlan) -> Result<Value, Failure> {
    let (workdir, run, token, actor, bead) = match plan {
        SessionPlan::New {
            workdir,
            run,
            token,
            actor,
            bead,
            ..
        }
        | SessionPlan::Resume {
            workdir,
            run,
            token,
            actor,
            bead,
            ..
        } => (workdir, run, token, actor, bead),
    };
    let value = |s: &Option<aae_contract::config::Sourced<String>>| s.as_ref().map(|s| s.value.clone());
    // The attempt environment and the tracker actor of the attempt, as the core planned it (contracts 5.1); the
    // thread's, not the shared process's.
    let mut attempt = Map::from_iter([
        ("AAE_RUN".to_owned(), json!(run)),
        ("AAE_ATTEMPT_TOKEN".to_owned(), json!(token)),
    ]);
    attempt.extend(actor.iter().map(|(name, actor)| (name.clone(), json!(actor))));
    attempt.extend(bead.iter().map(|bead| ("AAE_BEAD".to_owned(), json!(bead))));
    let mut thread_config = json!({ "shell_environment_policy": { "set": attempt } });
    if let Some(effort) = value(&config.effort) {
        thread_config["model_reasoning_effort"] = json!(effort);
    }
    let mcp = mcp(config)?;
    if !mcp.is_empty() {
        thread_config["mcp_servers"] = Value::Object(mcp);
    }
    let hooks = hooks(config, &attempt)?;
    if !hooks.is_empty() {
        // Hooks given to a thread run only with the bypass (experiment 2).
        thread_config["hooks"] = Value::Object(hooks);
        thread_config["bypass_hook_trust"] = json!(true);
    }
    let mut params = json!({ "cwd": workdir, "config": thread_config });
    let fields = [
        ("model", value(&config.model)),
        ("developerInstructions", value(&config.system_prompt)),
    ];
    for (key, value) in fields {
        if let Some(value) = value {
            params[key] = json!(value);
        }
    }
    // The codex approval policy (`untrusted`, `on-request`, `never`, ...): the native `approval_policy`.
    params["approvalPolicy"] = config
        .native
        .as_ref()
        .and_then(|n| n.value.get(APPROVAL))
        .map_or_else(|| json!(DEFAULT_APPROVAL), Value::clone);
    Ok(params)
}

/// `config.mcp_servers` of the thread. Each is `required`: a server aae gives that does not start fails
/// `thread/start` (or `thread/resume`) before the first prompt, instead of a session without its tools.
fn mcp(config: &EffectiveConfig) -> Result<Map<String, Value>, Failure> {
    let mut servers = Map::new();
    for (name, server) in &config.mcp {
        let server = &server.value;
        // A service is a URL by now (the core binds it); a stdio server runs per session whatever its scope.
        let mut entry = match (&server.command, &server.url) {
            (Some(command), None) if !command.is_empty() => {
                json!({ "command": command[0], "args": command[1..], "env": server.env })
            }
            (None, Some(url)) => json!({ "url": url, "http_headers": server.headers }),
            _ => return Err(config_error(format!("MCP server `{name}` needs a command or a url"))),
        };
        entry["required"] = json!(true);
        servers.insert(name.clone(), entry);
    }
    Ok(servers)
}

/// `config.hooks` of the thread: the `hooks.json` form, the same as claude's. A hook runs with the process
/// environment, not the thread's (experiment 2), so its command exports the attempt environment `vars` first.
fn hooks(config: &EffectiveConfig, vars: &Map<String, Value>) -> Result<Map<String, Value>, Failure> {
    let quote = |v: &str| format!("'{}'", v.replace('\'', r"'\''"));
    let exports: Vec<String> = (vars.iter())
        .map(|(k, v)| format!("{k}={}", quote(v.as_str().unwrap_or_default())))
        .collect();
    let export = format!("export {}; ", exports.join(" "));
    let mut events = Map::new();
    for (name, hook) in config.hooks.iter().filter(|(_, h)| h.value.enabled) {
        let hook = &hook.value;
        let (Some(event), Some(command)) = (&hook.event, &hook.command) else {
            return Err(config_error(format!("hook `{name}` needs an event and a command")));
        };
        let mut entry = json!({ "type": "command", "command": format!("{export}{command}") });
        if let Some(timeout) = &hook.timeout {
            let timeout =
                aae_contract::config::duration(timeout).map_err(|e| config_error(format!("hook `{name}`: {e}")))?;
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
    // Without the delay a thread nobody subscribes to is closed at once: a resume then loads it with its config.
    let mut args: Vec<String> = [
        "app-server",
        "-c",
        "analytics.enabled=false",
        "-c",
        "thread_unload_delay_secs=0",
    ]
    .map(str::to_owned)
    .into();
    for o in p["overrides"].as_array().into_iter().flatten() {
        args.extend(["-c".to_owned(), o.as_str().unwrap_or_default().to_owned()]);
    }
    args
}

/// The environment of the app-server: the daemon's `PATH`, `HOME`, `LANG`, `SHELL` (and network settings when it
/// inherits the host), the agent's variables, the homes, and the instance id the orphan sweep looks for. The attempt
/// variables go to each thread instead.
pub(crate) fn env(p: &Value, id: &str, socket: Option<&Path>) -> Result<Vec<(String, String)>, Failure> {
    let mut env = aae_contract::driver::child::base_env(daemon_vars(p), &p["env"], id, socket)?;
    env.push((
        "CODEX_HOME".to_owned(),
        p["home"].as_str().unwrap_or_default().to_owned(),
    ));
    env.extend(p["user_home"].as_str().map(|h| ("HOME".to_owned(), h.to_owned())));
    // The host's login (docs/design/config.md) may be a provider whose key is a variable of the daemon's environment.
    env.extend(
        p["host_key"]
            .as_str()
            .and_then(|k| Some((k.to_owned(), std::env::var(k).ok()?))),
    );
    Ok(env)
}

/// Creates the homes of an isolated process; without a credential of its own it gets the host's login, linked so
/// a token codex refreshes is the host's too.
pub(crate) fn materialize(p: &Value) -> std::io::Result<()> {
    if p["user_home"].is_null() && p["host"].is_null() {
        return Ok(());
    }
    let home = Path::new(p["home"].as_str().unwrap_or_default());
    std::fs::create_dir_all(home)?;
    if let Some(user_home) = p["user_home"].as_str() {
        std::fs::create_dir_all(user_home)?;
    }
    let host = p["host"].as_str().map_or_else(host_home, PathBuf::from);
    // A home inheriting the host keeps the host's own skills beside the agent's; an agent's skill of the same name
    // wins. Hidden entries such as `.system` are codex's own cache: it makes them in every home.
    let host_skills = (p["host"].is_string().then(|| host.join("skills")))
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), json!(e.path())))
        .filter(|(name, _)| !name.starts_with('.'));
    let own_skills = p["skills"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(n, s)| (n.clone(), s.clone()));
    // Links to the skill directories; the home is keyed by the part, so a link there is never another skill's.
    for (name, path) in own_skills.chain(host_skills) {
        let link = home.join("skills").join(name);
        if link.symlink_metadata().is_err() {
            std::fs::create_dir_all(home.join("skills"))?;
            std::os::unix::fs::symlink(path.as_str().unwrap_or_default(), link)?;
        }
    }
    let own = CREDENTIALS.iter().any(|k| !p["env"][k].is_null());
    // A home inheriting the host links its config and instructions too: codex reads the provider, the MCP
    // servers ... from them.
    let linked: &[&str] = match p["host"].as_str() {
        Some(_) => &["auth.json", "config.toml", "AGENTS.md"],
        None if own => &[],
        None => &["auth.json"],
    };
    for file in linked {
        let (target, link) = (host.join(file), home.join(file));
        if target.exists() && link.symlink_metadata().is_err() {
            std::os::unix::fs::symlink(target, link)?;
        }
    }
    Ok(())
}

/// The daemon's variables the app-server gets.
pub(crate) fn daemon_vars(p: &Value) -> impl Iterator<Item = &'static str> {
    let host = p["user_home"]
        .is_null()
        .then_some(aae_contract::driver::HOST_NETWORK_ENV);
    ["PATH", "HOME", "LANG", "SHELL"]
        .into_iter()
        .chain(host.into_iter().flatten())
}

/// Decision 38: a process runs with no sandbox unless the native block of its config, or the host's codex
/// config in `home` it inherits, sets one.
pub(crate) fn unconfined(config: &EffectiveConfig, home: &Path) -> bool {
    let native = config.native.as_ref().and_then(|n| n.value.as_object());
    let host_sets = config.inherits_host() && host_config(home).is_some_and(|c| c.contains_key("sandbox_mode"));
    !native.is_some_and(|n| n.contains_key("sandbox_mode")) && !host_sets
}

/// `config.toml` in `home`, if it is there and reads.
pub(crate) fn host_config(home: &Path) -> Option<toml::Table> {
    toml::from_str(&std::fs::read_to_string(home.join("config.toml")).ok()?).ok()
}

/// The model provider `config.toml` in `home` selects, by name, if the file defines it.
fn provider(home: &Path) -> Option<(String, toml::Value)> {
    let config = host_config(home)?;
    let name = config.get("model_provider")?.as_str()?;
    Some((name.to_owned(), config.get("model_providers")?.get(name)?.clone()))
}

/// The name of the model provider `config.toml` in `home` selects, if it defines it.
pub(crate) fn host_provider_name(home: &Path) -> Option<String> {
    provider(home).map(|(name, _)| name)
}

/// The variable that holds the key of the model provider `config.toml` in `home` selects (its `env_key`), if any.
pub(crate) fn host_key(home: &Path) -> Option<String> {
    provider(home)?.1.get("env_key")?.as_str().map(str::to_owned)
}

/// The host's login by a model provider of the host's codex config in `host` (docs/design/config.md): the name of its
/// key variable, and for an isolated process, which does not read that config, the provider itself as the first `-c`
/// flags.
pub(crate) fn host_provider(part: &mut Value, host: &Path, isolated: bool) {
    let Some((name, provider)) = self::provider(host) else {
        return;
    };
    if isolated && let Some(overrides) = part["overrides"].as_array_mut() {
        // The flags are in `argv`, which any user of the host sees: keys that hold secrets stay out.
        let mut table = provider.as_table().cloned().unwrap_or_default();
        table.retain(|k, _| k != "http_headers" && k != "experimental_bearer_token");
        // One table value, so a provider name with a dot stays one key.
        let providers = toml::Table::from_iter([(name.clone(), toml::Value::Table(table))]);
        let flags = [
            format!("model_provider={}", toml::Value::from(name.as_str())),
            format!("model_providers={}", toml::Value::Table(providers)),
        ];
        overrides.splice(0..0, flags.map(Value::from));
    }
    if let Some(key) = provider.get("env_key").and_then(toml::Value::as_str) {
        part["host_key"] = json!(key);
    }
}

/// The host's codex home: the daemon's `CODEX_HOME`, else `~/.codex`.
pub(crate) fn host_home() -> PathBuf {
    match std::env::var_os("CODEX_HOME") {
        Some(dir) => dir.into(),
        None => Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".codex"),
    }
}

/// The account the gates use: a hash of where the credential comes from, never the credential itself
/// (docs/design/contracts.md 7).
pub(crate) fn account(p: &Value) -> String {
    let env = &p["env"];
    let source = match CREDENTIALS.iter().find(|k| !env[**k].is_null()) {
        Some(key) => json!({ *key: env[*key] }),
        None => json!({ "host": host_home() }),
    };
    Fingerprint::of("codex", &ProcessPart(source)).0[..16].to_owned()
}

#[cfg(test)]
mod tests;
