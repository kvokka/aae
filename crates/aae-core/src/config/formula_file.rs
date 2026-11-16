//! A formula: the TOML shape of its file (docs/design/config.md "Formula"; unknown keys are errors) and the checked
//! formula the core runs (spec §9.1).

use super::{Name, Settings};
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
    /// Each run works in a git worktree of its own that aae makes and removes (spec §11.5).
    #[serde(default)]
    pub worktree: bool,
    /// The most rounds of a run that checks send back (spec §9.1); default 3.
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
    Ai {
        name: Option<String>,
        agent: String,
        timeout: Option<String>,
        harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        back_to: Option<String>,
        /// The most times the step runs (spec §9.1); default 1.
        max_attempts: Option<NonZeroU32>,
        /// The script whose exit code says whether the step passed; empty is none.
        step_check_script_path: Option<String>,
    },
    Exec {
        name: Option<String>,
        command: Vec<String>,
        timeout: Option<String>,
        back_to: Option<String>,
        max_attempts: Option<NonZeroU32>,
        step_check_script_path: Option<String>,
    },
    Drain {
        name: Option<String>,
        formula: String,
        timeout: Option<String>,
        /// The most beads in work at once (spec §9.1); default 1.
        pool: Option<NonZeroU32>,
        #[serde(default)]
        on_item_failure: OnItemFailure,
        max_attempts: Option<NonZeroU32>,
        step_check_script_path: Option<String>,
    },
}

/// What a drain step does once the run of a bead failed (spec §9.1).
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
    /// The most times the step runs; a step that does not pass runs again until then (spec §9.1). At least 1.
    pub max_attempts: u32,
    /// The script that says whether the step passed, by its exit code (spec §9.1).
    pub check: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub enum StepKind {
    Ai {
        agent: Name,
        timeout: Option<Duration>,
        /// The launch settings of the step.
        settings: Box<Settings>,
    },
    Exec {
        command: Vec<String>,
        timeout: Option<Duration>,
    },
    /// Every ready bead under the run's bead gets a run of `formula` (spec §9.1, §10.1).
    Drain {
        formula: Name,
        timeout: Option<Duration>,
        /// The most beads in work at once; at least 1.
        pool: u32,
        on_item_failure: OnItemFailure,
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

    /// `ai`, `exec` or `drain`, as the state and the API name the kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ai { .. } => "ai",
            Self::Exec { .. } => "exec",
            Self::Drain { .. } => "drain",
        }
    }
}
