//! A formula: the TOML shape of its file (docs/design/config.md "Formula"; unknown keys are errors) and the checked
//! formula the core runs (docs/design/config.md).

use super::{Name, Settings};
use crate::template::{Filter, Inputs};
use serde::Deserialize;
use std::{collections::BTreeMap, num::NonZeroU32, path::PathBuf, time::Duration};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FormulaFile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Input>,
    /// Each run works in a git worktree of its own that aae makes and removes (docs/design/config.md).
    #[serde(default)]
    pub worktree: bool,
    /// Started only by a step of another formula, hidden from the catalog (docs/design/config.md).
    #[serde(default)]
    pub internal: bool,
    /// The most rounds of a run that checks send back (docs/design/config.md); default 3.
    pub rounds: Option<NonZeroU32>,
    pub step: Vec<StepFile>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    Task,
    String,
    Integer,
    Boolean,
}

/// A declared formula input (docs/design/config.md).
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
    Ai {
        name: Option<String>,
        /// `if`: a `where` table over the run (docs/design/config.md); empty, the step always runs.
        #[serde(rename = "if", default)]
        condition: serde_json::Map<String, serde_json::Value>,
        agent: String,
        timeout: Option<String>,
        harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        /// The native settings of the step's agent, by harness (docs/design/config.md).
        #[serde(default)]
        native: BTreeMap<String, serde_json::Value>,
        back_to: Option<String>,
        /// The most times the step runs (docs/design/config.md); default 1.
        max_attempts: Option<NonZeroU32>,
        /// The script whose exit code says whether the step passed; empty is none.
        step_check_script_path: Option<String>,
    },
    Exec {
        name: Option<String>,
        /// `if`: a `where` table over the run (docs/design/config.md); empty, the step always runs.
        #[serde(rename = "if", default)]
        condition: serde_json::Map<String, serde_json::Value>,
        command: Vec<String>,
        timeout: Option<String>,
        back_to: Option<String>,
        max_attempts: Option<NonZeroU32>,
        step_check_script_path: Option<String>,
    },
    Drain {
        name: Option<String>,
        /// `if`: a `where` table over the run (docs/design/config.md); empty, the step always runs.
        #[serde(rename = "if", default)]
        condition: serde_json::Map<String, serde_json::Value>,
        formula: String,
        timeout: Option<String>,
        /// The most beads in work at once (docs/design/config.md); default 1.
        pool: Option<NonZeroU32>,
        #[serde(default)]
        on_item_failure: OnItemFailure,
        max_attempts: Option<NonZeroU32>,
        step_check_script_path: Option<String>,
    },
    Formula {
        name: Option<String>,
        /// `if`: a `where` table over the run (docs/design/config.md); empty, the step always runs.
        #[serde(rename = "if", default)]
        condition: serde_json::Map<String, serde_json::Value>,
        formula: String,
        /// Inputs of the formula given on the step, as a trigger's `inputs`; stronger than the inherited ones.
        #[serde(default)]
        inputs: serde_json::Map<String, serde_json::Value>,
        timeout: Option<String>,
        max_attempts: Option<NonZeroU32>,
        step_check_script_path: Option<String>,
    },
    Await {
        name: Option<String>,
        /// `if`: a `where` table over the run (docs/design/config.md); empty, the step always runs.
        #[serde(rename = "if", default)]
        condition: serde_json::Map<String, serde_json::Value>,
        timeout: Option<String>,
        /// What the end of the wait does (docs/design/config.md); default `true`, the run fails.
        fail_on_timeout: Option<bool>,
        back_to: Option<String>,
    },
}

impl StepFile {
    /// The `if` table as written.
    pub(super) fn condition(&self) -> &serde_json::Map<String, serde_json::Value> {
        match self {
            Self::Ai { condition, .. }
            | Self::Exec { condition, .. }
            | Self::Drain { condition, .. }
            | Self::Formula { condition, .. }
            | Self::Await { condition, .. } => condition,
        }
    }
}

/// What a drain step does once the run of a bead failed (docs/design/config.md).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnItemFailure {
    /// It gives the other beads; the failure does not fail the step.
    #[default]
    Continue,
    /// It gives no new bead and lets the started ones end, then fails.
    SoftStop,
    /// It cancels the started ones, then fails.
    HardStop,
}

#[derive(Clone, Debug)]
pub struct Formula {
    pub description: String,
    pub inputs: BTreeMap<String, Input>,
    /// `worktree = true`: each run gets a git worktree of its own (docs/design/config.md).
    pub worktree: bool,
    /// `internal = true`: started only by a `formula` or `drain` step, hidden from the catalog (docs/design/config.md).
    pub internal: bool,
    /// The most rounds of a run: a failed check with `back_to` starts the next one, past the last the run fails.
    pub rounds: u32,
    /// At least one; a run goes through them in order (docs/design/config.md).
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
    /// A check: when it fails, the run goes back to this earlier step with its remarks (docs/design/config.md).
    pub back_to: Option<String>,
    /// The most times the step runs, at least 1; a step that does not pass runs again until then
    /// (docs/design/config.md).
    pub max_attempts: u32,
    /// The script that says whether the step passed, by its exit code (docs/design/config.md).
    pub check: Option<PathBuf>,
    /// `if`: the step runs only when every entry matches the run (docs/design/config.md); empty, always.
    pub condition: Filter,
}

#[derive(Clone, Debug)]
pub enum StepKind {
    Ai {
        agent: Name,
        timeout: Option<Duration>,
        /// The launch settings and the native blocks of the step.
        settings: Box<Settings>,
    },
    Exec {
        command: Vec<String>,
        timeout: Option<Duration>,
    },
    /// Every ready bead under the run's bead gets a run of `formula` (docs/design/config.md,
    /// docs/design/contracts.md 5).
    Drain {
        formula: Name,
        timeout: Option<Duration>,
        /// The most beads in work at once; at least 1.
        pool: u32,
        on_item_failure: OnItemFailure,
    },
    /// The step is a run of `formula` of the same project (docs/design/config.md): its outcome is the step's.
    Formula {
        formula: Name,
        /// The inputs given on the step, resolved against the parent run.
        inputs: Inputs,
        timeout: Option<Duration>,
    },
    /// The step waits for an outcome given from outside, through the API or the CLI (docs/design/config.md).
    Await {
        timeout: Option<Duration>,
        /// At the timeout the run fails; `false`: the step passes.
        fail_on_timeout: bool,
    },
}

impl StepKind {
    /// The agent of an `ai` step.
    pub fn agent(&self) -> Option<&Name> {
        match self {
            Self::Ai { agent, .. } => Some(agent),
            _ => None,
        }
    }

    /// `ai`, `exec`, `drain`, `formula` or `await`, as the state and the API name the kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ai { .. } => "ai",
            Self::Exec { .. } => "exec",
            Self::Drain { .. } => "drain",
            Self::Formula { .. } => "formula",
            Self::Await { .. } => "await",
        }
    }
}
