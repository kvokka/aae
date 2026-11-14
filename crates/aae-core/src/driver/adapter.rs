//! What an adapter crate exports about itself (docs/design/adapters.md): the root of the build lists one
//! [`Adapter`] per harness and knows no harness by name.

use super::Driver;
use crate::config::EffectiveConfig;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// What the root of the build hands an adapter to make its driver from `[harness.<name>]`.
pub struct Setup<'a> {
    /// The resolved harness binary.
    pub binary: PathBuf,
    /// The keys of `[harness.<name>]` beyond `binary` and `model`: the adapter's own settings.
    pub options: &'a toml::Table,
    /// The daemon's state directory: the generated files of sessions live under it.
    pub state: &'a Path,
    /// The API socket a session gets in `AAE_SOCKET`.
    pub socket: Option<&'a Path>,
}

/// One `aae doctor` finding; `level` is `ok`, `warning` or `error`.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub check: String,
    pub level: &'static str,
    pub detail: String,
}

impl Check {
    pub fn new(check: impl Into<String>, level: &'static str, detail: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            level,
            detail: detail.into(),
        }
    }
}

/// The agents of one harness as `aae doctor` sees them: a label (`<project>/<rig>@<agent>`) and the effective config,
/// `None` where it does not assemble.
pub type DoctorAgents<'a> = &'a [(String, Option<EffectiveConfig>)];

/// One harness adapter.
#[derive(Debug)]
pub struct Adapter {
    /// The harness name: the `[harness.<name>]` table and the binary mise resolves.
    pub name: &'static str,
    /// Short model names and the full names they stand for, the latest of each family; any other name goes to the
    /// harness as it is.
    pub models: &'static [(&'static str, &'static str)],
    /// The harness versions the adapter was tested on, `[lowest, highest]` as `(major, minor, patch)`, inclusive.
    pub tested: [(u32, u32, u32); 2],
    /// The driver on `setup`; the error names the key of the adapter's settings that is wrong.
    pub new: fn(&Setup) -> Result<Arc<dyn Driver>, String>,
    /// The host checks of `aae doctor` for the harness and its agents: whether a session the daemon starts can log
    /// in, the host settings every session inherits, the sandbox.
    pub doctor: fn(DoctorAgents) -> Vec<Check>,
}

/// The finding on the stdio MCP servers `names` of the host settings of `harness` (spec §6.4): with host inheritance
/// on, each starts in every session, and the shared MCP of `aae` does not cover them.
pub fn host_mcp_check(harness: &str, names: &[&str]) -> Check {
    let check = format!("{harness} host mcp");
    match names.is_empty() {
        true => Check::new(check, "ok", "no stdio MCP server in the host settings"),
        false => Check::new(
            check,
            "warning",
            format!(
                "stdio MCP servers of the host settings start in every session that inherits the host: {}; \
                 declare them in the aae config, where they are shared, or set inherit_host = false",
                names.join(", ")
            ),
        ),
    }
}

/// Whether a session the daemon starts can log in: with the host's login `file`, or with one of the `credentials`
/// variables in its agent's own `env`; the daemon's own environment reaches no session (spec §6.5).
pub fn login_check(harness: &str, file: &Path, credentials: &[&str], agents: DoctorAgents) -> Check {
    let check = format!("{harness} login");
    if file.is_file() {
        return Check::new(check, "ok", file.display().to_string());
    }
    let keyless: Vec<&str> = (agents.iter())
        .filter(|(_, c)| {
            !c.as_ref()
                .is_some_and(|c| credentials.iter().any(|k| c.env.contains_key(*k)))
        })
        .map(|(label, _)| label.as_str())
        .collect();
    let file = file.display();
    match (agents.is_empty(), keyless.is_empty()) {
        (false, true) => Check::new(check, "ok", format!("every {harness} agent has a key in `env`")),
        (true, _) => Check::new(
            check,
            "warning",
            format!("no {file}: log in, or give the agents a key in `env`"),
        ),
        (false, false) => Check::new(
            check,
            "warning",
            format!(
                "no {file}: log in, or give these agents a key in `env`: {}",
                keyless.join(", ")
            ),
        ),
    }
}
