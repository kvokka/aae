//! The TOML shape of a formula file (docs/design/config.md "Formula"). Unknown keys are errors.

use serde::Deserialize;
use std::{collections::BTreeMap, num::NonZeroU32, path::PathBuf};

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
        prompt: PathBuf,
        timeout: Option<String>,
        harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        back_to: Option<String>,
    },
    Exec {
        name: Option<String>,
        command: Vec<String>,
        timeout: Option<String>,
        back_to: Option<String>,
    },
    Drain {
        name: Option<String>,
        formula: String,
        timeout: Option<String>,
    },
}
