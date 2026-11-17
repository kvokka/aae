//! Configuration: the files of the orchestrator, projects, packs, agents, formulas, triggers and addons
//! (docs/guide/addons.md; docs/design/config.md), loaded and validated as a whole.

mod addon;
mod assemble;
mod check;
mod compose;
mod defaults;
mod env;
mod files;
mod formula_file;
mod items;
mod load;
mod places;
mod prompt;
mod retired;
pub(crate) mod schema;
mod service;
mod sources;
mod subagents;
mod variants;

use crate::template::{Filter, Inputs};
pub use assemble::{Attached, EffectiveConfig, Layer, Sourced};
pub use env::{ExecEnv, env_literal, env_value};
pub use files::{AddonUse, Harness, Hook, Mcp, McpScope, Settings, Skill};
pub use formula_file::{Formula, Input, InputType, OnItemFailure, Step, StepKind};
pub use places::Scope;
pub use prompt::{Prompt, TEMPLATE};
pub use service::{Health, Service, ServiceScope};
pub use sources::{Checkouts, GitSource, commit_id, git_sources};
use std::{collections::BTreeMap, fmt, path::PathBuf, time::Duration};

/// The whole configuration of one daemon. Loading it is all or nothing: the caller keeps its previous `Config`
/// when [`load`] fails (docs/design/config.md).
#[derive(Clone, Debug)]
pub struct Config {
    /// Runs started at once across every project (docs/design/contracts.md 6).
    pub max_runs: u32,
    /// Failed runs of a bead before the core closes the bead's gate (docs/design/config.md).
    pub bead_failures: u32,
    /// The loopback address the API also listens on with a token (docs/api/frontends.md); off when absent.
    pub api_tcp: Option<std::net::SocketAddr>,
    /// The token of `api_tcp` for this daemon start: not from a file, the daemon sets it.
    pub api_token: String,
    /// The zone `cron` schedules are read in: `timezone` of the orchestrator config, else the system one.
    pub timezone: jiff::tz::TimeZone,
    pub harnesses: BTreeMap<String, Harness>,
    /// Layer 2 of docs/design/config.md.
    pub settings: Settings,
    /// The `[agents.<name>]` edits (no `base`), by agent reference; they may name an agent of any project.
    pub edits: BTreeMap<String, Settings>,
    /// The agent variants every project sees, by name; a project's own of the same name replaces one.
    pub variants: BTreeMap<String, Variant>,
    /// The adapters of the build: the short model names of each harness. The harnesses they name are the root's.
    pub adapters: &'static [&'static crate::driver::Adapter],
    pub addons: Vec<Addon>,
    /// The enabled bundled addons with their parameters, run after every declared addon.
    pub bundled: Vec<Bundled>,
    /// The thresholds of the watchdog (invariant 15): a reload applies to deadlines set from then on.
    pub watchdog: Watchdog,
    pub projects: Vec<Project>,
    /// The command prefix that serves a stdio MCP server over HTTP on `AAE_SERVICE_PORT` (docs/design/config.md): a
    /// stdio server not of scope `session` runs behind it as a service. Empty: no bridge, stdio servers stay per
    /// session. [`load`] leaves it empty; the binary sets it.
    pub bridge: Vec<String>,
    /// Where the daemon keeps the data of addons: `${AAE_ADDON_DATA}` of addon `p` is `<addon_data>/p`. `None`:
    /// [`load`] leaves the reference as it is; the binary sets it.
    pub addon_data: Option<PathBuf>,
    /// How long a service may take to pass its health check, and a launch waits for it (docs/design/config.md): 30 s.
    /// Not from a file: [`load`] sets it, a test shortens it.
    pub service_wait: Duration,
}

/// The thresholds of `[watchdog]` in the orchestrator file, in milliseconds (contracts 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watchdog {
    pub silence_ms: i64,
    pub tool_silence_ms: i64,
    pub background_silence_ms: i64,
    pub start_timeout_ms: i64,
    pub queue_timeout_ms: i64,
    pub park_timeout_ms: i64,
}

/// Ceiling of the core: attempts per run, not counting those that parked on an account gate (invariant 16).
pub const MAX_ATTEMPTS: u32 = 20;

/// The addons bundled with aae (docs/design/config.md): name, the events they hook, and whether they are on with no
/// `[builtin.<name>]` table. `worktree` has no table: it is on in a project with a formula of `worktree = true`.
pub const BUNDLED: [(&str, &[&str], bool); 2] = [
    (
        "default-policy",
        &["run.attempt_failed", "run.turn_ended_without_outcome"],
        true,
    ),
    (
        "worktree",
        &["run.preparing", "run.passed", "run.failed", "run.cancelled"],
        false,
    ),
];

/// An enabled bundled addon; `params` is its `[builtin.<name>]` table without `enabled`.
#[derive(Clone, Debug)]
pub struct Bundled {
    pub name: &'static str,
    pub on: &'static [&'static str],
    pub params: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    /// The directory of `aae.toml`: the config files, packs and tracker of the project live here.
    pub dir: PathBuf,
    /// The root's name and the root: a place like a rig, without its triggers and settings (docs/design/config.md).
    pub root: (String, Rig),
    /// Layer 3.
    pub settings: Settings,
    pub edits: BTreeMap<String, Settings>,
    pub variants: BTreeMap<String, Variant>,
    pub addons: Vec<Addon>,
    /// The bundled addons on for its runs: the orchestrator's, with the project's `[builtin.<name>]` tables.
    pub bundled: Vec<Bundled>,
    pub rigs: BTreeMap<String, Rig>,
    /// The project root pack (named after the project, holding the project triggers) first.
    pub packs: Vec<Pack>,
    /// The prompt of each agent its formulas' `ai` steps name, by the pack agent the step names.
    pub prompts: BTreeMap<Name, Prompt>,
    pub tracker: TrackerLink,
    /// `[exec]` of the orchestrator and the project: the environment of a check script of a step that is no exec
    /// step (an exec step's own is in its kind).
    pub exec: ExecEnv,
}

/// How the project's sessions and commands reach its tracker (docs/design/config.md). The core knows no tracker
/// (decision 43): the integration that makes trackers fills this in after loading; empty until then.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackerLink {
    /// The project has a tracker.
    pub present: bool,
    /// Variables every session and command of the project gets, tracker or not, under every layer: they name the
    /// tracker explicitly, so a tool's own search never climbs to another one.
    pub env: BTreeMap<String, String>,
    /// The variable that carries the attempt's tracker actor, `aae:<run>:<attempt>` (contracts 5.1), into a session
    /// or a command.
    pub actor: Option<String>,
    /// The variable that gives the beads a session or command files on a rig that rig's prefix
    /// (docs/design/config.md).
    pub prefix: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Rig {
    pub path: PathBuf,
    /// The prefix of the ids of its beads (docs/design/config.md); none for the root, whose beads carry the
    /// tracker's own.
    pub prefix: Option<String>,
    pub vars: BTreeMap<String, String>,
    /// Pack triggers that run on this rig.
    pub triggers: Vec<Name>,
    /// Layer 4.
    pub settings: Settings,
    pub edits: BTreeMap<String, Settings>,
    /// The variant that runs in place of a pack agent the formulas name, on this rig.
    pub agents: BTreeMap<Name, String>,
}

#[derive(Clone, Debug)]
pub struct Pack {
    pub name: String,
    pub root: PathBuf,
    /// `None` for the project root pack.
    pub version: Option<String>,
    pub requires_addons: Vec<String>,
    pub agents: BTreeMap<String, Agent>,
    pub formulas: BTreeMap<String, Formula>,
    pub triggers: Vec<Trigger>,
    /// `[settings]` of `pack.toml`: the launch settings of the runs of its formulas.
    pub settings: Settings,
}

#[derive(Clone, Debug)]
pub struct Agent {
    /// Its directory in the pack: `agent.toml`, the role prompt, `prompt.template.md` and the files it names.
    pub dir: PathBuf,
    /// The harness `agent.toml` names, part of the agent layer.
    pub harness: Option<String>,
    /// Where it works; an agent of the root takes no rig's layer, edits or variant.
    pub scope: Scope,
    /// The agent layer but its harness; its `system_prompt` is the role prompt file (docs/design/config.md) when the
    /// agent has one.
    pub settings: Settings,
    /// Runs of this agent started at once in its project; no limit but the orchestrator's when absent.
    pub max_runs: Option<u32>,
}

/// A pack agent under another name with settings of its own (`[agents.<name>]`): the role is the base agent's,
/// the limit, gates and events are the variant's.
#[derive(Clone, Debug)]
pub struct Variant {
    /// The pack agent, as written; it resolves in the project of the run.
    pub base: Name,
    pub settings: Settings,
    /// Its own limit; the base agent's when absent.
    pub max_runs: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Trigger {
    pub name: String,
    pub source: Source,
    pub run: Name,
    pub inputs: Inputs,
    /// Set only on a project trigger; a pack trigger gets its rigs from [`Rig::triggers`].
    pub rig: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Source {
    Event {
        on: String,
        filter: Filter,
    },
    Cron(crate::cron::Cron),
    /// At most one firing per interval.
    Every(Duration),
}

#[derive(Clone, Debug)]
pub struct Addon {
    pub name: String,
    pub version: String,
    /// The harnesses it is for; any when `None`.
    pub harnesses: Option<Vec<String>>,
    pub root: PathBuf,
    /// Layer 5 contribution to the agents that attach the addon.
    pub agent: Settings,
    /// The text its attached agents get after their system prompt.
    pub prompt: Option<String>,
    /// Its `bin/` directory, when it has one: ahead of `PATH` for its attached agents.
    pub bin: Option<PathBuf>,
    pub hooks: Vec<OrchestratorHook>,
    /// `[params]` of `addon.toml`: each parameter's default, which gives its type.
    pub params: serde_json::Map<String, serde_json::Value>,
}

/// A command on a core event (docs/design/contracts.md 3).
#[derive(Clone, Debug)]
pub struct OrchestratorHook {
    pub on: String,
    pub command: Vec<String>,
    pub timeout: Option<Duration>,
}

/// An item of a pack: an agent, formula or trigger, always resolved to its pack.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name {
    pub pack: String,
    pub name: String,
}

impl Project {
    /// The formula a run of this project names; a bare name resolves as written in the project root pack
    /// (docs/design/config.md).
    pub fn formula(&self, reference: &str) -> Result<(Name, &Formula), String> {
        let own = self.packs.first().map(|p| p.name.as_str()).unwrap_or_default();
        let has = |p: &Pack, n: &str| p.formulas.contains_key(n);
        let name = check::resolve(&self.packs, own, &load::reference(reference), has, "formula")?;
        let pack = self.packs.iter().find(|p| p.name == name.pack);
        let formula = pack
            .and_then(|p| p.formulas.get(&name.name))
            .ok_or(format!("no formula `{name}`"))?;
        Ok((name, formula))
    }
}

impl Config {
    /// The tracker actor of attempt `attempt` of run `run` in `project` (contracts 5.1): the variable and
    /// `aae:<run>:<attempt>`; none when the project has no actor variable.
    pub fn actor(&self, project: &str, run: &str, attempt: u32) -> Option<(String, String)> {
        let p = self.projects.iter().find(|p| p.name == project)?;
        Some((p.tracker.actor.clone()?, format!("aae:{run}:{attempt}")))
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.pack, self.name)
    }
}

/// One problem found while loading, with the file it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub file: PathBuf,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file.display(), self.message)
    }
}

/// Loads the orchestrator file (absent means empty), the projects it lists and everything they use, for a build
/// with `adapters` (see [`Config::adapters`]) and the git sources downloaded to `checkouts`. Every error found is
/// returned; a config with any error is never returned in part.
pub fn load_for(
    orchestrator_file: &std::path::Path,
    adapters: &'static [&'static crate::driver::Adapter],
    checkouts: Checkouts,
) -> Result<Config, Vec<ConfigError>> {
    let loader = load::Loader {
        adapters,
        checkouts,
        ..load::Loader::default()
    };
    loader.run(orchestrator_file)
}

/// [`load_for`] a build with no adapters, as tests of the core on a scripted harness load: every agent names its
/// harness, and no model name is expanded.
pub fn load(orchestrator_file: &std::path::Path) -> Result<Config, Vec<ConfigError>> {
    load_for(orchestrator_file, &[], Checkouts::new())
}

/// The environment variable an exec step gets input `name` in: `note-text` → `AAE_INPUT_NOTE_TEXT`.
pub fn input_variable(name: &str) -> String {
    format!("AAE_INPUT_{}", upper(name))
}

/// The name part of an environment variable: `note-text` → `NOTE_TEXT`.
pub fn upper(name: &str) -> String {
    let upper = name.chars().map(|c| {
        if c.is_ascii_alphanumeric() {
            c.to_ascii_uppercase()
        } else {
            '_'
        }
    });
    upper.collect()
}

/// A duration with one unit: `"500ms"`, `"30s"`, `"45m"`, `"2h"` (contracts conventions).
pub fn duration(text: &str) -> Result<Duration, String> {
    let split = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let n: u64 = number
        .parse()
        .map_err(|_| format!("`{text}` is not a duration like \"30s\""))?;
    let seconds = match unit {
        "ms" => return Ok(Duration::from_millis(n)),
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return Err(format!("`{text}`: the unit is one of ms, s, m, h")),
    };
    n.checked_mul(seconds)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("`{text}` is too long"))
}
