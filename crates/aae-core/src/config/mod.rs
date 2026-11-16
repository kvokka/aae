//! Configuration: the files of the orchestrator, projects, packs, agents, formulas, triggers and plugins
//! (spec §6, §8, §9.1, §9.5, §11; docs/design/config.md), loaded and validated as a whole.

mod assemble;
mod check;
mod compose;
mod files;
mod formula_file;
mod items;
mod load;
pub(crate) mod schema;
mod service;
mod sources;
mod variants;

use crate::template::{Filter, Inputs, Template};
pub use assemble::{EffectiveConfig, Layer, Sourced};
pub use files::{Harness, Hook, Mcp, McpScope, PluginUse, Settings, Skill};
pub use formula_file::{Input, InputType};
pub use service::{Health, Service, ServiceScope};
pub use sources::{Checkouts, GitSource, commit_id, git_sources};
use std::{collections::BTreeMap, fmt, path::PathBuf, time::Duration};

/// The whole configuration of one daemon. Loading it is all or nothing: the caller keeps its previous `Config`
/// when [`load`] fails (spec §6.5).
#[derive(Clone, Debug)]
pub struct Config {
    /// Runs started at once across every project (spec §9.3).
    pub max_runs: u32,
    /// Failed runs of a bead before the core closes the bead's gate (spec §10.3).
    pub bead_failures: u32,
    /// The loopback address the API also listens on with a token (spec §12); off when absent.
    pub api_tcp: Option<std::net::SocketAddr>,
    /// The token of `api_tcp` for this daemon start: not from a file, the daemon sets it.
    pub api_token: String,
    /// The zone `cron` schedules are read in: `timezone` of the orchestrator config, else the system one.
    pub timezone: jiff::tz::TimeZone,
    pub harnesses: BTreeMap<String, Harness>,
    /// Layer 2 of spec §6.3.
    pub settings: Settings,
    /// The `[agents.<name>]` edits (no `base`), by agent reference; they may name an agent of any project.
    pub edits: BTreeMap<String, Settings>,
    /// The agent variants every project sees, by name; a project's own of the same name replaces one.
    pub variants: BTreeMap<String, Variant>,
    /// The adapters of the build: the short model names of each harness. The harnesses they name are the root's.
    pub adapters: &'static [&'static crate::driver::Adapter],
    pub plugins: Vec<Plugin>,
    /// The enabled bundled plugins with their parameters, run after every declared plugin.
    pub bundled: Vec<Bundled>,
    /// The thresholds of the watchdog (invariant 15): a reload applies to deadlines set from then on.
    pub watchdog: Watchdog,
    pub projects: Vec<Project>,
    /// The command prefix that serves a stdio MCP server over HTTP on `AAE_SERVICE_PORT` (spec §11.4): a stdio
    /// server not of scope `session` runs behind it as a service. Empty: no bridge, stdio servers stay per session.
    /// [`load`] leaves it empty; the binary sets it.
    pub bridge: Vec<String>,
    /// How long a service may take to pass its health check, and a launch waits for it (spec §11.4): 30 s.
    /// Not from a file: [`load`] sets it, a test shortens it.
    pub service_wait: Duration,
}

/// The thresholds of `[watchdog]` in the orchestrator file, in milliseconds (contracts 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watchdog {
    pub silence_ms: i64,
    pub tool_silence_ms: i64,
    pub background_silence_ms: i64,
    pub human_timeout_ms: i64,
    pub start_timeout_ms: i64,
    pub queue_timeout_ms: i64,
    pub park_timeout_ms: i64,
}

/// The plugins bundled with aae (spec §11.5): name, the events they hook, and whether they are on with no
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

/// An enabled bundled plugin; `params` is its `[builtin.<name>]` table without `enabled`.
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
    /// Layer 3.
    pub settings: Settings,
    pub edits: BTreeMap<String, Settings>,
    pub variants: BTreeMap<String, Variant>,
    pub plugins: Vec<Plugin>,
    /// The bundled plugins on for its runs: the orchestrator's, with the project's `[builtin.<name>]` tables.
    pub bundled: Vec<Bundled>,
    pub rigs: BTreeMap<String, Rig>,
    /// The project root pack (named after the project, holding the project triggers) first.
    pub packs: Vec<Pack>,
    pub tracker: TrackerLink,
}

/// How the project's sessions and commands reach its tracker (spec §10.2). The core knows no tracker (spec §2,
/// decision 43): the integration that makes trackers fills this in after loading; empty until then.
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
}

#[derive(Clone, Debug)]
pub struct Rig {
    pub path: PathBuf,
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
    pub requires_plugins: Vec<String>,
    pub agents: BTreeMap<String, Agent>,
    pub formulas: BTreeMap<String, Formula>,
    pub triggers: Vec<Trigger>,
    /// `[settings]` of `pack.toml`: the launch settings of the runs of its formulas.
    pub settings: Settings,
}

#[derive(Clone, Debug)]
pub struct Agent {
    /// The harness `agent.toml` names, part of the agent layer.
    pub harness: Option<String>,
    /// The agent layer but its harness; its `system_prompt` is the role prompt file (spec §8.1) when the agent has one.
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
pub struct Formula {
    pub description: String,
    pub inputs: BTreeMap<String, Input>,
    /// `worktree = true`: each run gets a git worktree of its own (spec §11.5).
    pub worktree: bool,
    /// The most rounds of a run: a failed check with `back_to` starts the next one, past the last the run fails.
    pub rounds: u32,
    /// At least one; a run goes through them in order (spec §9.1).
    pub steps: Vec<Step>,
}

impl Formula {
    /// The step named `name`.
    pub fn step(&self, name: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.name == name)
    }

    /// The step after the one named `name`; `None` after the last.
    pub fn next(&self, name: &str) -> Option<&Step> {
        let at = self.steps.iter().position(|s| s.name == name)?;
        self.steps.get(at + 1)
    }
}

#[derive(Clone, Debug)]
pub struct Step {
    /// `name` in the file, or the position from 1.
    pub name: String,
    pub kind: StepKind,
    /// A check: when it fails, the run goes back to this earlier step with its remarks (spec §9.1).
    pub back_to: Option<String>,
}

#[derive(Clone, Debug)]
pub enum StepKind {
    Ai {
        agent: Name,
        prompt: Template,
        timeout: Option<Duration>,
        /// The launch settings of the step.
        settings: Box<Settings>,
    },
    Exec {
        command: Vec<String>,
        timeout: Option<Duration>,
    },
    /// Every ready bead under the run's bead gets a run of `formula` (spec §9.1, §10.1).
    Drain { formula: Name, timeout: Option<Duration> },
}

impl StepKind {
    /// `ai`, `exec` or `drain`, as the state and the API name the kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ai { .. } => "ai",
            Self::Exec { .. } => "exec",
            Self::Drain { .. } => "drain",
        }
    }
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
pub struct Plugin {
    pub name: String,
    pub version: String,
    pub root: PathBuf,
    /// Layer 5 contribution to the agents that attach the plugin.
    pub agent: Settings,
    /// The text its attached agents get after their system prompt.
    pub prompt: Option<String>,
    /// Its `bin/` directory, when it has one: ahead of `PATH` for its attached agents.
    pub bin: Option<PathBuf>,
    pub hooks: Vec<OrchestratorHook>,
    /// `config` of its declaration: the parameters of its orchestrator hooks.
    pub params: serde_json::Map<String, serde_json::Value>,
}

/// A command on a core event (spec §11.3).
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
    /// The formula a run of this project names; a bare name resolves as written in the project root pack (spec §8.2).
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
pub(crate) fn input_variable(name: &str) -> String {
    format!("AAE_INPUT_{}", upper(name))
}

/// The name part of an environment variable: `note-text` → `NOTE_TEXT`.
pub(crate) fn upper(name: &str) -> String {
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
