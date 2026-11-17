//! The codex adapter as the root of the build lists it: its driver on the settings of `[harness.codex]` and its
//! `loop-manager doctor` checks.

use crate::{
    Codex, TESTED,
    part::{CREDENTIALS, host_config, host_home, host_key, host_provider_name, unconfined},
};
use loop_manager_contract::driver::{Adapter, Check, DoctorAgents, Driver, Setup, login_check};
use std::{num::NonZeroU32, path::Path, sync::Arc, time::Duration};

pub const ADAPTER: Adapter = Adapter {
    name: "codex",
    models: MODELS,
    tested: TESTED,
    agent_types: &["default", "explorer", "worker"],
    subagents_inherit: true,
    new,
    doctor,
};

/// The short model names of the owner's table (docs/design/adapters.md): the latest model of each family.
const MODELS: &[(&str, &str)] = &[
    ("astra", "gpt-6-astra"),
    ("luna", "gpt-6-luna"),
    ("terra", "gpt-6.1-terra"),
    ("sol", "gpt-6.1-sol"),
];

/// The settings of `[harness.codex]` beyond `model` and `binary` (contracts 4.5).
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Threads one app-server holds before a second one starts.
    max_sessions: Option<NonZeroU32>,
    /// Seconds an app-server without threads lives on before it stops.
    instance_idle_s: Option<u32>,
}

fn new(setup: &Setup) -> Result<Arc<dyn Driver>, String> {
    let options: Options = setup.options.clone().try_into().map_err(|e: toml::de::Error| {
        // serde names an unknown key but not the key of a wrong value: each key alone finds it.
        let alone = |k: &String, v: &toml::Value| toml::Table::from_iter([(k.clone(), v.clone())]);
        let wrong = (setup.options.iter()).find(|(k, v)| alone(k, v).try_into::<Options>().is_err());
        wrong.map_or_else(|| e.message().to_owned(), |(k, _)| format!("`{k}`: {}", e.message()))
    })?;
    let max_sessions = options.max_sessions.map_or(8, |n| n.get() as usize);
    let idle = Duration::from_secs(options.instance_idle_s.map_or(60, u64::from));
    let (state, socket) = (setup.state.to_owned(), setup.socket.map(Path::to_owned));
    Ok(Arc::new(
        Codex::new(setup.binary.clone(), state, socket, max_sessions).instance_idle(idle),
    ))
}

fn doctor(agents: DoctorAgents) -> Vec<Check> {
    let host = host_home();
    vec![login(&host, agents), host_mcp(&host), sandbox(&host, agents)]
}

/// The host's login file or a key of the agent's own `env`, as for any harness; or, with a model provider in the
/// host config, the daemon's variable it names in `env_key`: the adapter passes that one on. Such a provider logs
/// codex in without `auth.json`, so its absence says nothing: a provider with no `env_key` is not checked.
pub(crate) fn login(host: &Path, agents: DoctorAgents) -> Check {
    let check = |level, detail: String| Check::new("codex login", level, detail);
    match (host_provider_name(host), host_key(host)) {
        (_, Some(key)) if std::env::var_os(&key).is_some() => check("ok", format!("{key} is set")),
        (Some(name), Some(key)) => check(
            "warning",
            format!("{key}, the `env_key` of the host's model provider `{name}`, is not set for the daemon"),
        ),
        (Some(name), None) => check(
            "warning",
            format!("not checked: the host's model provider `{name}` has no `env_key`"),
        ),
        (None, _) => login_check("codex", &host.join("auth.json"), &CREDENTIALS, agents),
    }
}

/// The stdio MCP servers of the host's `config.toml`.
fn host_mcp(host: &Path) -> Check {
    let config = host_config(host).unwrap_or_default();
    let servers = config
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten();
    let names: Vec<&str> = (servers.filter(|(_, s)| s.get("command").is_some()))
        .map(|(n, _)| n.as_str())
        .collect();
    loop_manager_contract::driver::host_mcp_check("codex", &names)
}

/// Decision 38: the codex sandbox needs user namespaces, which many hosts lack, so a codex agent whose config
/// sets no `sandbox_mode` runs with none; the warning names every such agent, per rig. A config that does not
/// assemble cannot be shown to set it, so its agent is named too.
fn sandbox(host: &Path, agents: DoctorAgents) -> Check {
    let open: Vec<&str> = (agents.iter())
        .filter(|(_, c)| c.as_ref().is_none_or(|c| unconfined(c, host)))
        .map(|(label, _)| label.as_str())
        .collect();
    match (agents.is_empty(), open.is_empty()) {
        (true, _) => Check::new("codex sandbox", "ok", "no codex agents"),
        (_, true) => Check::new("codex sandbox", "ok", "every codex agent sets sandbox_mode"),
        (_, false) => Check::new(
            "codex sandbox",
            "warning",
            format!(
                "sandbox_mode = \"danger-full-access\", commands not confined to the workdir: {}",
                open.join(", ")
            ),
        ),
    }
}
