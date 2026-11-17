//! The values of an `env` table (docs/design/config.md "Environment values", decision 137): a literal, or
//! `${NAME}` for a variable of the daemon's environment, and `$$` for a `$`. The config keeps the value as written;
//! it is resolved when the process starts and is never stored resolved. And the environment of an exec step: `[exec]`
//! of the orchestrator and the project, and the step's own keys.

use super::{Layer, Sourced, load::Loader};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

/// `[exec]` of a config file, or the same keys of an exec step.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecFile {
    pub inherit_env: Option<bool>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// The environment an exec step or a check script gets besides aae's own variables, each value with its layer.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ExecEnv {
    /// The daemon's whole environment under `env`; no layer saying so is `false`.
    pub inherit_env: Option<Sourced<bool>>,
    pub env: BTreeMap<String, Sourced<String>>,
}

impl ExecEnv {
    /// Takes from `file` what no stronger layer gave: applied strongest first, `env` merges by key and
    /// `inherit_env` is the strongest layer's that has it.
    pub(super) fn under(&mut self, file: &ExecFile, from: &Layer) {
        if self.inherit_env.is_none() {
            self.inherit_env = (file.inherit_env).map(|value| Sourced {
                value,
                from: from.clone(),
            });
        }
        for (key, value) in &file.env {
            self.env.entry(key.clone()).or_insert_with(|| Sourced {
                value: value.clone(),
                from: from.clone(),
            });
        }
    }

    /// One line per value with its layer, as `config explain` prints them.
    pub fn explain(&self) -> String {
        let mut out = String::new();
        if let Some(inherit) = &self.inherit_env {
            let _ = writeln!(out, "exec.inherit_env = {}  # {}", inherit.value, inherit.from);
        }
        for (key, value) in &self.env {
            let _ = writeln!(out, "exec.env.{key} = {:?}  # {}", value.value, value.from);
        }
        out
    }
}

/// `text` with every `${NAME}` replaced by `var(NAME)` and every `$$` by `$`. Any other `$` is an error, and so is a
/// variable `var` does not give; an error names the variable, never the rest of the value.
pub fn env_value(text: &str, var: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if let Some(after) = rest.strip_prefix('$') {
            out.push('$');
            rest = after;
            continue;
        }
        let name = (rest.strip_prefix('{'))
            .and_then(|r| r.split_once('}'))
            .filter(|(name, _)| is_name(name));
        let Some((name, after)) = name else {
            return Err("a `$` is written `$$`, a variable of the daemon `${NAME}`".to_owned());
        };
        out.push_str(&var(name).ok_or_else(|| format!("the daemon has no variable `{name}`"))?);
        rest = after;
    }
    out.push_str(rest);
    Ok(out)
}

/// `text` as an `env` value that stands for itself.
pub fn env_literal(text: &str) -> String {
    text.replace('$', "$$")
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

impl Loader {
    /// Checks the form of each value of `env` (the daemon's variables are not there to see at load), an error of
    /// `file` naming `what` and the key.
    pub(super) fn env_values(&mut self, file: &Path, what: &str, env: &BTreeMap<String, String>) {
        for (key, value) in env {
            if let Err(e) = env_value(value, |_| Some(String::new())) {
                self.fail(file, format!("`{what}.{key}`: {e}"));
            }
        }
    }
}
