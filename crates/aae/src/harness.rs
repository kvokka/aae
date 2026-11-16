//! The harnesses of a config: the adapter of each, where its binary is and its driver.

use crate::daemon::Paths;
use aae_core::{
    config,
    driver::{Adapter, Driver, Setup},
};
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
    let mise = aae_core::orphans::output_std(std::process::Command::new("mise").args(["which", name]));
    match mise {
        Ok(out) if out.status.success() => {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            (path.into(), "mise")
        }
        _ => (name.into(), "PATH"),
    }
}

/// The adapters of this build: a harness is a row here, and nothing else in the root knows its name.
const ADAPTERS: &[&Adapter] = &[&aae_claude::ADAPTER, &aae_codex::ADAPTER];

/// The adapter of harness `name`.
pub(crate) fn adapter(name: &str) -> Option<&'static Adapter> {
    ADAPTERS.iter().copied().find(|a| a.name == name)
}

fn setup<'a>(binary: PathBuf, harness: &'a config::Harness, paths: &'a Paths) -> Setup<'a> {
    Setup {
        binary,
        options: &harness.options,
        state: &paths.state,
        socket: Some(&paths.socket),
    }
}

/// The driver of harness `name` on its resolved binary, with the binary and its source; `None` for a harness aae
/// has no adapter for or settings it does not take, which `load` refused.
pub(crate) fn driver(
    name: &str,
    harness: &config::Harness,
    paths: &Paths,
) -> Option<(Arc<dyn Driver>, PathBuf, &'static str)> {
    let adapter = adapter(name)?;
    let (binary, source) = binary(name, harness.binary.as_deref());
    let driver = (adapter.new)(&setup(binary.clone(), harness, paths)).ok()?;
    Some((driver, binary, source))
}

/// The harness versions the adapter of `driver` was tested on.
pub(crate) fn tested(driver: &Arc<dyn Driver>) -> [(u32, u32, u32); 2] {
    adapter(driver.harness())
        .expect("every driver comes from an adapter")
        .tested
}

/// What is wrong with `[harness.<name>]`: no adapter for it, or settings its adapter does not take.
fn harness_error(name: &str, harness: &config::Harness, paths: &Paths) -> Option<String> {
    let Some(adapter) = adapter(name) else {
        let known = ADAPTERS.iter().map(|a| a.name).collect::<Vec<_>>().join(", ");
        return Some(format!("aae has no adapter for harness `{name}`; known: {known}"));
    };
    // The binary is not resolved for a check: no adapter reads it before a spawn.
    let binary = harness.binary.clone().unwrap_or_else(|| name.into());
    (adapter.new)(&setup(binary, harness, paths)).err()
}

/// The config of `paths` with the parameters of the bundled plugins checked, or every error in it.
pub(crate) fn load(paths: &Paths) -> Result<config::Config, Vec<String>> {
    let file = &paths.config;
    let checkouts = crate::git_cache::checkouts(&paths.cache, file);
    let mut config = config::load_for(file, ADAPTERS, checkouts)
        .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>())?;
    // br is the tracker of a project with a `.beads` directory, always named by an explicit path: its own search
    // would climb to another project's (spec §10.2).
    for p in &mut config.projects {
        let dir = p.dir.join(".beads");
        p.tracker.present = dir.is_dir();
        (p.tracker.env).insert("BEADS_DIR".to_owned(), dir.to_string_lossy().into_owned());
        p.tracker.actor = Some("BD_ACTOR".to_owned());
        p.tracker.prefix = Some("BD_ISSUE_PREFIX".to_owned());
    }
    config.bridge = [paths.exe.to_string_lossy().as_ref(), "mcp-bridge", "--"]
        .map(str::to_owned)
        .to_vec();
    let harnesses = (config.harnesses.iter()).filter_map(|(name, h)| {
        harness_error(name, h, paths).map(|e| format!("{}: `harness.{name}`: {e}", file.display()))
    });
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
