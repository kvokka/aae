//! The harnesses of a config: where each binary is, the driver of each and the adapter settings it takes.

use crate::daemon::Paths;
use aae_claude::Claude;
use aae_codex::Codex;
use aae_core::{
    config,
    driver::{Driver, ProcessPart},
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Where the binary `name` is and who said so (spec §6.5): the config's `explicit` path, else mise (a daemon under
/// systemd or launchd has no mise shims on its `PATH`), else the bare name, looked up on `PATH` at each spawn.
pub(crate) fn binary(name: &str, explicit: Option<&Path>) -> (PathBuf, &'static str) {
    if let Some(path) = explicit {
        return (path.to_owned(), "config");
    }
    let mise = std::process::Command::new("mise").args(["which", name]).output();
    match mise {
        Ok(out) if out.status.success() => {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            (path.into(), "mise")
        }
        _ => (name.into(), "PATH"),
    }
}

/// The driver of harness `name` on its resolved binary, with the binary and its source; `None` for a harness aae
/// has no adapter for.
pub(crate) fn driver(
    name: &str,
    harness: &config::Harness,
    paths: &Paths,
) -> Option<(Arc<dyn Driver>, PathBuf, &'static str)> {
    let (binary, source) = binary(name, harness.binary.as_deref());
    let (state, socket) = (paths.state.clone(), Some(paths.socket.clone()));
    let driver: Arc<dyn Driver> = match name {
        "claude" => Arc::new(Claude::new(binary.clone(), state, socket)),
        "codex" => {
            // `load` refused settings that do not parse.
            let o = codex_options(harness).unwrap_or_default();
            let max_sessions = o.max_sessions.map_or(8, |n| n.get() as usize);
            let idle = std::time::Duration::from_secs(o.instance_idle_s.map_or(60, u64::from));
            Arc::new(Codex::new(binary.clone(), state, socket, max_sessions).instance_idle(idle))
        }
        _ => return None,
    };
    Some((driver, binary, source))
}

/// The settings of `[harness.codex]` beyond `model` and `binary` (contracts 4.5).
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CodexOptions {
    /// Threads one app-server holds before a second one starts.
    max_sessions: Option<std::num::NonZeroU32>,
    /// Seconds an app-server without threads lives on before it stops.
    instance_idle_s: Option<u32>,
}

fn codex_options(harness: &config::Harness) -> Result<CodexOptions, toml::de::Error> {
    harness.options.clone().try_into()
}

/// What is wrong with the adapter settings of harness `name`, if anything; claude takes none.
fn options_error(name: &str, harness: &config::Harness) -> Option<String> {
    match name {
        "codex" => codex_options(harness).err().map(|e| {
            // serde names an unknown key but not the key of a wrong value: each key alone finds it.
            let alone = |k: &String, v: &toml::Value| toml::Table::from_iter([(k.clone(), v.clone())]);
            let wrong = (harness.options.iter()).find(|(k, v)| alone(k, v).try_into::<CodexOptions>().is_err());
            wrong.map_or_else(|| e.message().to_owned(), |(k, _)| format!("`{k}`: {}", e.message()))
        }),
        _ => (harness.options.keys().next()).map(|key| format!("unknown key `{key}`")),
    }
}

/// The harness versions the adapter of `driver` was tested on.
pub(crate) fn tested(driver: &Arc<dyn Driver>) -> [(u32, u32, u32); 2] {
    driver.capabilities(&ProcessPart(json!(null))).versions
}

/// The config in `file` with the parameters of the bundled plugins checked, or every error in it.
pub(crate) fn load(file: &Path) -> Result<config::Config, Vec<String>> {
    let mut config = config::load(file).map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>())?;
    // br is the tracker of a project with a `.beads` directory, always named by an explicit path: its own search
    // would climb to another project's (spec §10.2).
    for p in &mut config.projects {
        let dir = p.root.join(".beads");
        p.tracker.present = dir.is_dir();
        (p.tracker.env).insert("BEADS_DIR".to_owned(), dir.to_string_lossy().into_owned());
        p.tracker.actor = Some("BD_ACTOR".to_owned());
    }
    let harnesses = (config.harnesses.iter())
        .filter_map(|(name, h)| options_error(name, h).map(|e| format!("{}: `harness.{name}`: {e}", file.display())));
    let all = config
        .bundled
        .iter()
        .chain(config.projects.iter().flat_map(|p| &p.bundled));
    // A project inherits the orchestrator's tables, so the same error is said once.
    let errors: Vec<String> = all
        .filter_map(|b| aae_plugins::check(b.name, &b.params).err())
        .map(|e| e.to_string())
        .chain(harnesses)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    match errors.is_empty() {
        true => Ok(config),
        false => Err(errors),
    }
}
