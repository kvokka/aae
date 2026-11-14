//! The claude adapter as the root of the build lists it: its driver and its `aae doctor` checks.

use crate::{
    Claude, TESTED,
    part::{CREDENTIALS, host_dir},
};
use aae_core::driver::{Adapter, Check, DoctorAgents, Driver, Setup, login_check};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub const ADAPTER: Adapter = Adapter {
    name: "claude",
    // The CLI itself takes the short names (`fable`, `opus`, `sonnet`, `haiku`) for the latest model of the family.
    models: &[],
    tested: TESTED,
    new,
    doctor,
};

/// claude takes no settings of its own in `[harness.claude]`.
fn new(setup: &Setup) -> Result<Arc<dyn Driver>, String> {
    if let Some(key) = setup.options.keys().next() {
        return Err(format!("unknown key `{key}`"));
    }
    let (state, socket) = (setup.state.to_owned(), setup.socket.map(Path::to_owned));
    Ok(Arc::new(Claude::new(setup.binary.clone(), state, socket)))
}

fn doctor(agents: DoctorAgents) -> Vec<Check> {
    // Inherited or delivered, every session without a key of its own gets the host's login.
    let file = host_dir().join(".credentials.json");
    vec![
        login_check("claude", &file, &CREDENTIALS, agents),
        host_mcp(&claude_json()),
    ]
}

/// The user file of claude: in `$CLAUDE_CONFIG_DIR` when set, else `~/.claude.json`.
fn claude_json() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude.json"),
        |d| PathBuf::from(d).join(".claude.json"),
    )
}

/// The stdio MCP servers of the host's user file (spec §6.4): with host inheritance on, each starts in every
/// session, and the shared MCP of `aae` does not cover them.
fn host_mcp(file: &Path) -> Check {
    let user: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap_or_default()).unwrap_or_default();
    let projects = (user["projects"].as_object().into_iter().flatten()).map(|(_, p)| &p["mcpServers"]);
    let mut names: Vec<&str> = std::iter::once(&user["mcpServers"])
        .chain(projects)
        .filter_map(Value::as_object)
        .flatten()
        .filter(|(_, s)| !s["command"].is_null())
        .map(|(n, _)| n.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    aae_core::driver::host_mcp_check("claude", &names)
}
