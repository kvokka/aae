//! The effective config of a session as `claude` flags and environment (docs/design/experiments.md "Config and
//! credential delivery"). claude runs a process per session, so everything is in the process part.

use aae_contract::{
    config::{EffectiveConfig, Hook, Sourced},
    driver::{Failure, Fingerprint, ProcessPart, SessionPlan, child::config_error},
};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// The process part of a session: `{session_id, resume, workdir, config_dir, run, attempt, token, actor, bead, model,
/// effort, system_prompt, permission_mode, inherit_host, env, settings, mcp, agents, skills}`; `settings`, `mcp` and
/// `agents` are the JSON of `--settings`, `--mcp-config` and `--agents`, `skills` the skill directories by name, the
/// targets' among them.
pub(crate) fn process(config: &EffectiveConfig, plan: &SessionPlan, state: &Path) -> Result<Value, Failure> {
    let (settings, permission_mode) = settings(config)?;
    let mcp = mcp(config)?;
    // The skills of the targets join the session's in its one plugin: the session's first, the same name is the same
    // directory (a load error otherwise).
    let mut skills = BTreeMap::new();
    for c in std::iter::once(config).chain(config.targets.values()) {
        for (name, s) in c.skills.iter().filter(|(_, s)| s.value.enabled) {
            if let Some(path) = &s.value.path {
                skills.entry(name.clone()).or_insert_with(|| path.clone());
            }
        }
    }
    let (session_id, resume, workdir, run, attempt, token, actor, bead) = match plan {
        SessionPlan::New {
            workdir,
            run,
            attempt,
            token,
            actor,
            bead,
        } => {
            let name = format!("aae:session:{run}/{attempt}");
            let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()).to_string();
            (id, false, workdir, run, attempt, token, actor, bead)
        }
        SessionPlan::Resume {
            session,
            workdir,
            run,
            attempt,
            token,
            actor,
            bead,
        } => {
            let id = session.0["session_id"].as_str().unwrap_or_default().to_owned();
            (id, true, workdir, run, attempt, token, actor, bead)
        }
    };
    let inherit = config.inherits_host();
    // An isolated session keeps its settings, transcripts and session registry under the state directory.
    // Generated files and, for an isolated session, its settings, transcripts and session registry live under the
    // state directory (docs/design/config.md).
    let files = state.join("claude").join(&session_id);
    let config_dir = (!inherit).then(|| files.join("config"));
    let env: BTreeMap<_, _> = config.env.iter().map(|(k, v)| (k.clone(), v.value.clone())).collect();
    Ok(json!({
        "session_id": session_id,
        "resume": resume,
        "workdir": workdir,
        "config_dir": config_dir,
        "files": files,
        "run": run,
        "attempt": attempt,
        "token": token,
        "actor": actor,
        "bead": bead,
        "model": text(&config.model),
        "effort": text(&config.effort),
        "system_prompt": text(&config.system_prompt),
        "permission_mode": permission_mode,
        "inherit_host": inherit,
        "env": env,
        "settings": settings,
        "mcp": mcp,
        "agents": agents(config)?,
        "skills": skills,
    }))
}

fn text(s: &Option<Sourced<String>>) -> Option<String> {
    s.as_ref().map(|s| s.value.clone())
}

/// The permission mode of a session whose config names none: the harness decides every permission and nothing waits
/// for a person (decision 117).
const DEFAULT_PERMISSION_MODE: &str = "bypassPermissions";

/// The `--settings` JSON: the native block (claude's own settings keys) with the enabled hooks added to its own; and
/// the `--permission-mode` the block's `permission_mode` gives, which is a flag and not a settings key, or the default
/// when neither it nor `permissions.defaultMode` names a mode.
fn settings(config: &EffectiveConfig) -> Result<(Value, Option<String>), Failure> {
    let mut settings = match config.native.as_ref().map(|n| &n.value) {
        None => json!({}),
        Some(native @ Value::Object(_)) => native.clone(),
        Some(_) => {
            return Err(config_error(
                "the claude native block is a table of claude settings".into(),
            ));
        }
    };
    let mode = match settings.as_object_mut().and_then(|s| s.remove("permission_mode")) {
        None if settings["permissions"]["defaultMode"].is_null() => Some(DEFAULT_PERMISSION_MODE.to_owned()),
        None => None,
        Some(Value::String(mode)) => Some(mode),
        Some(_) => return Err(config_error("the claude native `permission_mode` is a string".into())),
    };
    // Without the host layer the own config directory and the setting sources drop the user memory, but claude also
    // reads `CLAUDE.md` up the rig's ancestors, and a rig under `HOME` reaches the host's (criterion 12).
    if let Some(home) = std::env::var_os("HOME").filter(|_| !config.inherits_host()) {
        let host = Path::new(&home).join(".claude");
        let excluded = [host.join("CLAUDE.md"), host.join("rules/**")];
        settings["claudeMdExcludes"] = json!(excluded);
    }
    add_hooks(&mut settings, config.hooks.iter().map(|(name, h)| (name, &h.value)))?;
    let settings = if settings == json!({}) { Value::Null } else { settings };
    Ok((settings, mode))
}

/// The `--mcp-config` JSON of the session's own servers.
fn mcp(config: &EffectiveConfig) -> Result<Value, Failure> {
    let servers = servers(config)?;
    Ok(if servers.is_empty() {
        Value::Null
    } else {
        json!({ "mcpServers": servers })
    })
}

/// The `--mcp-config` entries of the servers of `config`, by name.
fn servers(config: &EffectiveConfig) -> Result<Map<String, Value>, Failure> {
    let mut servers = Map::new();
    for (name, server) in &config.mcp {
        let server = &server.value;
        // A service is a URL by now (the core binds it); a stdio server runs per session whatever its scope.
        let entry = match (&server.command, &server.url) {
            (Some(command), None) if !command.is_empty() => {
                json!({ "type": "stdio", "command": command[0], "args": command[1..], "env": server.env })
            }
            (None, Some(url)) => json!({ "type": "http", "url": url, "headers": server.headers }),
            _ => return Err(config_error(format!("MCP server `{name}` needs a command or a url"))),
        };
        servers.insert(name.clone(), entry);
    }
    Ok(servers)
}

/// The `--agents` JSON: a definition per target, by its name. A target's server of a name the session has, and a
/// hook equal to one of the session's, are the session's: the subagent sees the server and runs the hook already.
fn agents(config: &EffectiveConfig) -> Result<Value, Failure> {
    let mut agents = Map::new();
    for (name, t) in &config.targets {
        let mut agent =
            json!({ "description": text(&t.description), "prompt": text(&t.system_prompt).unwrap_or_default() });
        for (key, value) in [("model", &t.model), ("effort", &t.effort)] {
            if let Some(value) = text(value) {
                agent[key] = json!(value);
            }
        }
        // claude takes the servers of a definition as a list of one-server maps, not as a map.
        let servers: Vec<_> = servers(t)?
            .into_iter()
            .filter(|(k, _)| !config.mcp.contains_key(k))
            .map(|(k, v)| json!({ k: v }))
            .collect();
        if !servers.is_empty() {
            agent["mcpServers"] = json!(servers);
        }
        let own = |h: &Hook| !config.hooks.values().any(|s| s.value == *h);
        add_hooks(
            &mut agent,
            t.hooks.iter().map(|(k, h)| (k, &h.value)).filter(|(_, h)| own(h)),
        )?;
        agents.insert(name.clone(), agent);
    }
    Ok(if agents.is_empty() {
        Value::Null
    } else {
        Value::Object(agents)
    })
}

/// Adds the enabled `hooks` to the `hooks` key of `into`, in the shape of `--settings`.
fn add_hooks<'a>(into: &mut Value, hooks: impl Iterator<Item = (&'a String, &'a Hook)>) -> Result<(), Failure> {
    for (name, hook) in hooks.filter(|(_, h)| h.enabled) {
        let (Some(event), Some(command)) = (&hook.event, &hook.command) else {
            return Err(config_error(format!("hook `{name}` needs an event and a command")));
        };
        let mut entry = json!({ "type": "command", "command": command });
        if let Some(timeout) = &hook.timeout {
            let timeout =
                aae_contract::config::duration(timeout).map_err(|e| config_error(format!("hook `{name}`: {e}")))?;
            entry["timeout"] = json!(timeout.as_secs().max(1));
        }
        let matcher = json!({ "matcher": hook.matcher.clone().unwrap_or_default(), "hooks": [entry] });
        let shape = || config_error(format!("the claude native `hooks.{event}` must be a list of matchers"));
        let hooks = &mut into["hooks"];
        if hooks.is_null() {
            *hooks = json!({});
        }
        let Value::Object(hooks) = hooks else {
            return Err(shape());
        };
        match hooks.entry(event.clone()).or_insert(Value::Null) {
            Value::Array(list) => list.push(matcher),
            slot @ Value::Null => *slot = json!([matcher]),
            _ => return Err(shape()),
        }
    }
    Ok(())
}

/// The arguments of `claude` for the process part `p`.
pub(crate) fn args(p: &Value) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--permission-prompts",
        "none",
    ]
    .map(str::to_owned)
    .into();
    let session = p["session_id"].as_str().unwrap_or_default().to_owned();
    let flag = if p["resume"] == true {
        "--resume"
    } else {
        "--session-id"
    };
    args.extend([flag.to_owned(), session]);
    // Host inheritance off keeps the rig's own settings, `CLAUDE.md` and `.mcp.json` (experiments 4).
    if p["inherit_host"] == false {
        args.extend(["--setting-sources".to_owned(), "project,local".to_owned()]);
    }
    let files = Path::new(p["files"].as_str().unwrap_or_default());
    let file = |name: &str| files.join(name).to_string_lossy().into_owned();
    if !p["settings"].is_null() {
        args.extend(["--settings".to_owned(), file("settings.json")]);
    }
    // Not `--strict-mcp-config`: it would cut the rig's `.mcp.json` too; the host's servers are already cut by the
    // setting sources and the own config directory when inheritance is off.
    if !p["mcp"].is_null() {
        args.extend(["--mcp-config".to_owned(), file("mcp.json")]);
    }
    if !p["agents"].is_null() {
        args.extend(["--agents".to_owned(), file("agents.json")]);
    }
    if p["skills"].as_object().is_some_and(|s| !s.is_empty()) {
        args.extend(["--plugin-dir".to_owned(), file("plugin")]);
    }
    // Every flag again on a resume: claude keeps none of them (experiments 4), the model included.
    for (key, flag) in [
        ("model", "--model"),
        ("effort", "--effort"),
        ("system_prompt", "--append-system-prompt"),
        ("permission_mode", "--permission-mode"),
    ] {
        if let Some(value) = p[key].as_str() {
            args.extend([flag.to_owned(), value.to_owned()]);
        }
    }
    args
}

/// Writes the files the flags of [`args`] name: settings, MCP config, agent definitions and a plugin holding the
/// skills (as links to their directories). The directory is the owner's alone: the MCP config may carry secrets.
pub(crate) fn materialize(p: &Value) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, symlink};
    let files = Path::new(p["files"].as_str().unwrap_or_default());
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(files)?;
    for (key, name) in [
        ("settings", "settings.json"),
        ("mcp", "mcp.json"),
        ("agents", "agents.json"),
    ] {
        if !p[key].is_null() {
            std::fs::write(files.join(name), p[key].to_string())?;
        }
    }
    let Some(skills) = p["skills"].as_object().filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let plugin = files.join("plugin");
    // A resume rebuilds the plugin: the skills of the new process part, not of the last one.
    let _ = std::fs::remove_dir_all(&plugin);
    std::fs::create_dir_all(plugin.join(".claude-plugin"))?;
    std::fs::create_dir_all(plugin.join("skills"))?;
    let manifest = json!({ "name": "aae", "version": "0.0.0", "description": "Skills delivered by aae" });
    std::fs::write(plugin.join(".claude-plugin/plugin.json"), manifest.to_string())?;
    for (name, path) in skills {
        symlink(path.as_str().unwrap_or_default(), plugin.join("skills").join(name))?;
    }
    Ok(())
}

/// The whole environment of `claude` for the process part `p` of instance `id`; nothing else is inherited.
/// `host` is the host's claude config directory: an isolated session without a credential of its own gets the
/// host's login from there (docs/design/config.md).
pub(crate) fn env(p: &Value, id: &str, socket: Option<&Path>, host: &Path) -> Result<Vec<(String, String)>, Failure> {
    let mut env = aae_contract::driver::child::base_env(daemon_vars(p), &p["env"], id, socket)?;
    let text = |v: &Value| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string());
    env.extend([
        ("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS".to_owned(), "1".to_owned()),
        // A claude that cannot start says why in a `result`, not only on stderr.
        ("CLAUDE_CODE_STARTUP_FAILURE_RESULTS".to_owned(), "1".to_owned()),
        // The subagent types are those of the agent's list alone: no built-in one (decision 143).
        ("CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS".to_owned(), "1".to_owned()),
        ("AAE_RUN".to_owned(), text(&p["run"])),
        ("AAE_ATTEMPT_TOKEN".to_owned(), text(&p["token"])),
    ]);
    // The tracker actor of the attempt, as the core planned it (contracts 5.1).
    env.extend(serde_json::from_value::<(String, String)>(p["actor"].clone()).ok());
    env.extend(p["bead"].as_str().map(|bead| ("AAE_BEAD".to_owned(), bead.to_owned())));
    env.extend(
        p["config_dir"]
            .as_str()
            .map(|d| ("CLAUDE_CONFIG_DIR".to_owned(), d.to_owned())),
    );
    let own = CREDENTIALS.iter().any(|k| !p["env"][k].is_null());
    if !p["config_dir"].is_null() && !own {
        env.extend(host_token(host).map(|t| ("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), t)));
    }
    Ok(env)
}

/// The daemon's variables `claude` gets; a session inheriting the host also gets the host's config directory and
/// network settings.
pub(crate) fn daemon_vars(p: &Value) -> impl Iterator<Item = &'static str> {
    let inherits = p["config_dir"].is_null();
    let host = inherits.then_some(aae_contract::driver::HOST_NETWORK_ENV);
    ["PATH", "HOME", "LANG"]
        .into_iter()
        .chain(host.into_iter().flatten())
        .chain(inherits.then_some("CLAUDE_CONFIG_DIR"))
}

/// The variables that carry a credential; a session with one of them uses it, not the host's login.
pub(crate) const CREDENTIALS: [&str; 3] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"];

/// The host's claude config directory: the daemon's `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub(crate) fn host_dir() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => dir.into(),
        None => Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".claude"),
    }
}

/// The subscription access token of the login in `host` (`claudeAiOauth.accessToken` of `.credentials.json`),
/// read at every spawn, so a token the host refreshed reaches the next process.
// shortcut: the file only — macOS keeps the login in the keychain; read it there once macOS is checked
// (docs/overview.md).
fn host_token(host: &Path) -> Option<String> {
    let text = std::fs::read_to_string(host.join(".credentials.json")).ok()?;
    let credentials: Value = serde_json::from_str(&text).ok()?;
    credentials["claudeAiOauth"]["accessToken"].as_str().map(str::to_owned)
}

/// The account the gates use: a hash of where the credential comes from, never the credential itself
/// (docs/design/contracts.md 7).
pub(crate) fn account(p: &Value) -> String {
    let env = &p["env"];
    let source = match CREDENTIALS.iter().find(|k| !env[**k].is_null()) {
        Some(key) => json!({ *key: env[*key] }),
        // Inherited or delivered, it is the host's login.
        None => json!({ "host": host_dir() }),
    };
    let hash = Fingerprint::of("claude", &ProcessPart(source)).0;
    hash[..16].to_owned()
}
