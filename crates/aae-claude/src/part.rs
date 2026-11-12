//! The effective config of a session as `claude` flags and environment (docs/design/experiments.md "Config and
//! credential delivery"). claude runs a process per session, so everything is in the process part.

use aae_core::{
    config::EffectiveConfig,
    driver::{Failure, FailureKind, Fingerprint, ProcessPart, SessionPlan, Source},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;

/// The process part of a session: `{session_id, resume, workdir, config_dir, run, attempt, token, model, effort,
/// system_prompt, permission_mode, inherit_host, env}`.
pub(crate) fn process(config: &EffectiveConfig, plan: &SessionPlan, state: &Path) -> Result<Value, Failure> {
    // shortcut: MCP servers, harness hooks, skills, plugins and the native block are delivered from WP-27b; until
    // then a config that has them is refused rather than run without them.
    let unsupported = [
        (!config.mcp.is_empty(), "mcp"),
        (!config.hooks.is_empty(), "hooks"),
        (!config.skills.is_empty(), "skills"),
        (!config.plugins.is_empty(), "plugins"),
        (config.native.is_some(), "native"),
    ];
    if let Some((_, what)) = unsupported.iter().find(|(has, _)| *has) {
        let mut failure = Failure::new(FailureKind::ConfigError, Source::Adapter);
        failure.raw = format!("the claude adapter does not deliver `{what}` yet");
        return Err(failure);
    }
    let (session_id, resume, workdir, run, attempt, token) = match plan {
        SessionPlan::New {
            workdir,
            run,
            attempt,
            token,
        } => {
            let name = format!("aae:session:{run}/{attempt}");
            let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()).to_string();
            (id, false, workdir, run, attempt, token)
        }
        SessionPlan::Resume {
            session,
            workdir,
            run,
            attempt,
            token,
        } => {
            let id = session.0["session_id"].as_str().unwrap_or_default().to_owned();
            (id, true, workdir, run, attempt, token)
        }
    };
    let inherit = config.inherits_host();
    // An isolated session keeps its settings, transcripts and session registry under the state directory.
    let config_dir = (!inherit).then(|| state.join("claude").join(&session_id));
    let value = |s: &Option<aae_core::config::Sourced<String>>| s.as_ref().map(|s| s.value.clone());
    let env: BTreeMap<_, _> = config.env.iter().map(|(k, v)| (k.clone(), v.value.clone())).collect();
    Ok(json!({
        "session_id": session_id,
        "resume": resume,
        "workdir": workdir,
        "config_dir": config_dir,
        "run": run,
        "attempt": attempt,
        "token": token,
        "model": value(&config.model),
        "effort": value(&config.effort),
        "system_prompt": value(&config.system_prompt),
        "permission_mode": value(&config.permission_mode),
        "inherit_host": inherit,
        "env": env,
    }))
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
        "--permission-prompt-tool",
        "stdio",
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

/// The whole environment of `claude` for the process part `p` of instance `id`; nothing else is inherited.
pub(crate) fn env(p: &Value, id: &str, socket: Option<&Path>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = ["PATH", "HOME", "LANG"]
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
    env.extend([
        ("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS".to_owned(), "1".to_owned()),
        ("AAE_RUN".to_owned(), text(&p["run"])),
        ("AAE_ATTEMPT_TOKEN".to_owned(), text(&p["token"])),
        (aae_core::orphans::INSTANCE_ENV.to_owned(), id.to_owned()),
    ]);
    env.extend(
        p["config_dir"]
            .as_str()
            .map(|d| ("CLAUDE_CONFIG_DIR".to_owned(), d.to_owned())),
    );
    env.extend(socket.map(|s| ("AAE_SOCKET".to_owned(), s.to_string_lossy().into_owned())));
    env
}

/// The account the gates use: a hash of where the credential comes from, never the credential itself (§5.7).
pub(crate) fn account(p: &Value) -> String {
    let source = match p["env"]["ANTHROPIC_API_KEY"].as_str() {
        Some(key) => json!({ "api_key": key }),
        None if p["inherit_host"] == true => json!({ "host": std::env::var("HOME").unwrap_or_default() }),
        None => json!({ "oauth": p["env"]["CLAUDE_CODE_OAUTH_TOKEN"] }),
    };
    let hash = Fingerprint::of("claude", &ProcessPart(source)).0;
    hash[..16].to_owned()
}
