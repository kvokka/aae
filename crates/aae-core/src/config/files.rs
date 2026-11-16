//! The TOML shape of every config file (docs/design/config.md). Unknown keys are errors.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, num::NonZeroU32, path::PathBuf};

/// The settings of one layer of spec §6.3; paths are absolute once loaded.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The agent CLI (docs/design/config.md "Harness and model").
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub system_prompt: Option<String>,
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

impl Settings {
    /// `${AAE_PLUGIN_DIR}` becomes `dir` wherever a plugin's `[agent]` may name its own files: harness hook
    /// commands, `env`, MCP commands and their `env`, and every string of the `native` tables.
    pub(super) fn in_plugin(&mut self, dir: &str) {
        let at = |text: &mut String| *text = text.replace(super::load::PLUGIN_DIR, dir);
        self.hooks.values_mut().filter_map(|h| h.command.as_mut()).for_each(at);
        self.env.values_mut().for_each(at);
        for mcp in self.mcp.values_mut() {
            mcp.command
                .iter_mut()
                .flatten()
                .chain(mcp.env.values_mut())
                .for_each(at);
        }
        self.native.values_mut().for_each(|v| strings(v, &at));
    }
}

fn strings(value: &mut Value, at: &impl Fn(&mut String)) {
    match value {
        Value::String(text) => at(text),
        Value::Array(list) => list.iter_mut().for_each(|v| strings(v, at)),
        Value::Object(table) => table.values_mut().for_each(|v| strings(v, at)),
        _ => {}
    }
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
    /// A non-empty stdio `command` with its `env` and `scope`, or a `url` with its `headers`; why not, otherwise.
    pub(super) fn malformed(&self) -> Option<&'static str> {
        match (&self.command, &self.url) {
            (Some(command), None) if command.is_empty() => Some("has an empty `command`"),
            (Some(_), None) if !self.headers.is_empty() => Some("has `headers`, which only a `url` server takes"),
            (None, Some(_)) if !self.env.is_empty() || self.scope.is_some() => {
                Some("has `env` or `scope`, which only a `command` server takes")
            }
            (Some(_), None) | (None, Some(_)) => None,
            (None, None) => Some("needs a `command` or a `url`"),
            (Some(_), Some(_)) => {
                Some("has both `command` and `url`: a server the daemon runs is a `[[service]]` with `mcp`")
            }
        }
    }
}

/// A harness hook in the one hook dialect (spec §11.2); `enabled = false` alone switches off a lower layer's hook.
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

/// The hook events of the hook dialect, those of all the adapters together (`docs/design/config.md`).
const HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "Setup",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "Notification",
    "MessageDisplay",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "TeammateIdle",
    "InstructionsLoaded",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "PreCompact",
    "PostCompact",
    "PreModelSwitch",
    "PostModelSwitch",
    "Elicitation",
    "ElicitationResult",
    "SessionEnd",
    "Interrupt",
    "PreModelRequest",
    "PostModelRequest",
];

impl Hook {
    /// An enabled hook has `event` (a known one) and `command`; why not, otherwise.
    pub(super) fn malformed(&self) -> Option<String> {
        match (&self.event, &self.command) {
            _ if !self.enabled => None,
            (Some(event), Some(_)) if !HOOK_EVENTS.contains(&event.as_str()) => {
                Some(format!("has an unknown event `{event}`"))
            }
            (Some(_), Some(_)) => None,
            _ => Some("needs `event` and `command`".to_owned()),
        }
    }
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
    /// `[agents.<name>]`: an edit of the agent of that name, or with `base` a variant of a pack agent.
    #[serde(default)]
    pub agents: BTreeMap<String, toml::Table>,
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
    /// §2, decision 43: the core knows no harness).
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

/// Where a pack or plugin lives: a directory, or a git repository at a tag, a commit or its latest revision.
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
    pub agents: BTreeMap<String, toml::Table>,
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
    /// `agents.<name>`: a table edits that agent on this rig; a string names the variant that runs in its place.
    #[serde(default)]
    pub agents: BTreeMap<String, toml::Value>,
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
    #[serde(default)]
    pub settings: super::compose::Launch,
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

/// What an earlier config shape said, and where it is written now: the error then is that one line.
const RETIRED: [(&str, &str); 3] = [
    (
        "unknown field `permission_mode`",
        "`permission_mode` is native to a harness: `approval_policy` in `[settings.native.codex]`, \
         `permission_mode` in `[settings.native.claude]`",
    ),
    (
        "unknown field `overrides`",
        "`[overrides.<agent>]` is `[agents.<agent>]` now",
    ),
    ("unknown variant `prompt`", "step kind `prompt` is `ai` now"),
];

/// A parse error as one line with the hint of [`RETIRED`] when it is about a retired key, else as it is.
pub(super) fn retired(message: String) -> String {
    let Some((_, hint)) = RETIRED.iter().find(|(said, _)| message.contains(said)) else {
        return message.trim_end().to_owned();
    };
    match message
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("TOML parse error at "))
    {
        Some(at) => format!("{at}: {hint}"),
        None => (*hint).to_owned(),
    }
}
