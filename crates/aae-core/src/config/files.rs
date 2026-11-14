//! The TOML shape of every config file (docs/design/config.md). Unknown keys are errors.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, num::NonZeroU32, path::PathBuf};

/// The settings of one layer of spec §6.3; paths are absolute once loaded.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub system_prompt: Option<String>,
    pub permission_mode: Option<String>,
    pub inherit_host: Option<bool>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub mcp: BTreeMap<String, Mcp>,
    #[serde(default)]
    pub hooks: BTreeMap<String, Hook>,
    #[serde(default)]
    pub skills: BTreeMap<String, Skill>,
    #[serde(default)]
    pub plugins: Vec<PluginUse>,
    /// Harness-native blocks, by harness name; the core passes them to the adapter as they are.
    #[serde(default)]
    pub native: BTreeMap<String, Value>,
    /// The `[[service]]` tables of the file of this layer: not a key of `[settings]`.
    #[serde(skip)]
    pub services: BTreeMap<String, super::Service>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum McpScope {
    Orchestrator,
    Project,
    Session,
}

/// An MCP server: a command (stdio) or a URL (HTTP); a server the daemon runs is a `[[service]]` (spec §11.4).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mcp {
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub scope: Option<McpScope>,
}

impl Mcp {
    /// A non-empty stdio `command` or a `url`, not both.
    pub(super) fn well_formed(&self) -> bool {
        match (&self.command, &self.url) {
            (Some(command), None) => !command.is_empty(),
            (None, url) => url.is_some(),
            (Some(_), Some(_)) => false,
        }
    }
}

/// A harness hook in the Claude dialect (spec §11.2); `enabled = false` alone switches off a lower layer's hook.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub event: Option<String>,
    pub matcher: Option<String>,
    pub command: Option<String>,
    pub timeout: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

/// A plugin attached to agents, in attachment order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginUse {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrchestratorFile {
    #[serde(default)]
    pub projects: Vec<PathBuf>,
    pub max_runs: Option<NonZeroU32>,
    pub bead_failures: Option<NonZeroU32>,
    pub api_tcp: Option<std::net::SocketAddr>,
    /// The IANA name of the zone `cron` is read in; the system zone when absent.
    pub timezone: Option<String>,
    #[serde(default)]
    pub harness: BTreeMap<String, Harness>,
    #[serde(default)]
    pub plugin: Vec<PluginDecl>,
    /// `[builtin.<name>]`: `enabled` and the parameters of a bundled plugin.
    #[serde(default)]
    pub builtin: BTreeMap<String, Map<String, Value>>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub overrides: BTreeMap<String, Settings>,
    #[serde(default)]
    pub service: Vec<super::service::ServiceFile>,
    /// `[watchdog]`: thresholds of invariant 15 as durations, by key.
    #[serde(default)]
    pub watchdog: BTreeMap<String, String>,
}

/// A harness agents may choose and its defaults (spec §6.1).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Harness {
    pub model: Option<String>,
    pub binary: Option<PathBuf>,
    /// The other keys: the adapter's own settings, which the core passes on unread and the adapter checks (spec
    /// §2.1 item 6: the core knows no harness).
    #[serde(flatten)]
    pub options: toml::Table,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginDecl {
    /// Exactly one of `path` and `git`; `rev` goes with `git`.
    pub path: Option<PathBuf>,
    pub git: Option<String>,
    pub rev: Option<String>,
    /// The parameters its orchestrator hooks get in `AAE_PLUGIN_PARAMS`.
    #[serde(default)]
    pub config: Map<String, Value>,
}

/// Where a pack or plugin lives: a directory, or a git repository at a revision pinned in `aae.lock`.
#[derive(Debug, Deserialize)]
#[serde(untagged, expecting = "a pack is a path or `{ git, rev }`")]
pub(super) enum Source {
    Path(PathBuf),
    Git(super::GitSource),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectFile {
    pub name: Option<String>,
    #[serde(default)]
    pub packs: Vec<Source>,
    #[serde(default)]
    pub plugin: Vec<PluginDecl>,
    /// `[builtin.<name>]` for the runs of this project: a table here replaces the orchestrator's whole.
    #[serde(default)]
    pub builtin: BTreeMap<String, Map<String, Value>>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub overrides: BTreeMap<String, Settings>,
    #[serde(default)]
    pub rigs: BTreeMap<String, RigFile>,
    #[serde(default)]
    pub trigger: Vec<TriggerFile>,
    #[serde(default)]
    pub service: Vec<super::service::ServiceFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RigFile {
    pub path: PathBuf,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub overrides: BTreeMap<String, Settings>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PackFile {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub requires_plugins: Vec<String>,
    pub min_aae: Option<String>,
    #[serde(default)]
    pub trigger: Vec<TriggerFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TriggerFile {
    pub name: String,
    pub on: Option<String>,
    pub cron: Option<String>,
    pub every: Option<String>,
    #[serde(rename = "where", default)]
    pub filter: Map<String, Value>,
    pub run: String,
    #[serde(default)]
    pub inputs: Map<String, Value>,
    pub rig: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FormulaFile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Input>,
    /// Each run works in a git worktree of its own that aae makes and removes (spec §11.5).
    #[serde(default)]
    pub worktree: bool,
    pub step: StepFile,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    Bead,
    String,
    Integer,
    Boolean,
}

/// A declared formula input (spec §9.1).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(rename = "type")]
    pub kind: InputType,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub(super) enum StepFile {
    Prompt {
        agent: String,
        prompt: PathBuf,
        timeout: Option<String>,
    },
    Exec {
        command: Vec<String>,
        timeout: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginFile {
    pub name: String,
    pub version: String,
    /// A file whose text joins the system prompt of the agents that attach the plugin.
    pub prompt: Option<PathBuf>,
    #[serde(default)]
    pub agent: Settings,
    #[serde(default)]
    pub hook: Vec<OrchestratorHookFile>,
    #[serde(default)]
    pub service: Vec<super::service::ServiceFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrchestratorHookFile {
    pub on: String,
    pub command: Vec<String>,
    pub timeout: Option<String>,
}
