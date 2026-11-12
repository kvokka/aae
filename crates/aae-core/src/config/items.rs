//! The content of a pack: agents (spec §8.1), formulas (§9.1) and triggers (§9.5), each checked on its own.

use super::{
    Agent, Formula, Settings, Source, Step, Trigger, duration,
    files::{FormulaFile, InputType, StepFile, TriggerFile},
    load::{Loader, parent, reference, resolve},
    schema::{bead_path, event_path, run_path},
};
use crate::{
    cron::Cron,
    template::{Filter, Inputs, Template},
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

/// The roots of a prompt template (contracts 2.3).
const PROMPT_ROOTS: &[&str] = &["inputs", "bead", "rig", "run", "project"];

impl Loader {
    pub(super) fn agent(&mut self, dir: &Path, plugins: &[&str]) -> Option<Agent> {
        let file = dir.join("agent.toml");
        let mut table: toml::Table = self.read(&file)?;
        let mut text = |key: &str| match table.remove(key) {
            Some(toml::Value::String(s)) => Ok(Some(s)),
            None => Ok(None),
            Some(_) => Err(format!("`{key}` must be a string")),
        };
        let (harness, prompt) = (text("harness"), text("prompt"));
        let max_runs = match table.remove("max_runs") {
            None => Ok(None),
            Some(toml::Value::Integer(n)) if u32::try_from(n).is_ok_and(|n| n > 0) => Ok(u32::try_from(n).ok()),
            Some(_) => Err("`max_runs` must be a positive 32-bit integer".to_owned()),
        };
        let parsed = toml::Value::Table(table)
            .try_into::<Settings>()
            .map_err(|e| e.to_string());
        let (harness, prompt, settings, max_runs) = match (harness, prompt, parsed, max_runs) {
            (Ok(Some(h)), Ok(p), Ok(s), Ok(m)) => (h, p, s, m),
            (Ok(None), ..) => return self.none(&file, "`harness` is required"),
            (Err(e), ..) | (_, Err(e), ..) | (_, _, Err(e), _) | (.., Err(e)) => return self.none(&file, e),
        };
        if !self.harnesses.contains(&harness) {
            self.fail(&file, format!("harness `{harness}` is not in the orchestrator config"));
        }
        let prompt = match prompt {
            Some(p) => self.text(&file, &dir.join(p)),
            None if !dir.join("prompt.md").exists() => None,
            None => self.text(&file, &dir.join("prompt.md")),
        };
        let settings = self.settings(&file, dir, settings, plugins);
        Some(Agent {
            harness,
            prompt,
            settings,
            max_runs,
        })
    }

    fn text(&mut self, file: &Path, path: &Path) -> Option<String> {
        fs::read_to_string(path)
            .map_err(|e| self.fail(file, format!("`{}`: {e}", path.display())))
            .ok()
    }

    pub(super) fn formula(&mut self, file: &Path) -> Option<(String, Formula)> {
        let raw: FormulaFile = self.read(file)?;
        self.name(file, "formula", &raw.name);
        let step = match raw.step {
            StepFile::Prompt { agent, prompt, timeout } => {
                let path = resolve(&parent(file), &prompt);
                let text = self.text(file, &path)?;
                let template = Template::parse(&text, PROMPT_ROOTS)
                    .map_err(|e| self.fail(&path, e.0))
                    .ok()?;
                for bad in template.paths().filter(|p| !prompt_path(p, &raw.inputs)) {
                    self.fail(
                        &path,
                        format!("`{{{{ {bad} }}}}` is not a value of this formula's runs"),
                    );
                }
                let timeout = self.timeout(file, timeout.as_deref());
                Step::Prompt {
                    agent: reference(&agent),
                    prompt: template,
                    timeout,
                }
            }
            StepFile::Exec { command, timeout } => {
                if command.is_empty() {
                    self.fail(file, "`command` is empty");
                }
                Step::Exec {
                    command,
                    timeout: self.timeout(file, timeout.as_deref()),
                }
            }
        };
        Some((
            raw.name,
            Formula {
                description: raw.description,
                inputs: raw.inputs,
                step,
            },
        ))
    }

    pub(super) fn trigger(&mut self, file: &Path, raw: TriggerFile, project: bool) -> Option<Trigger> {
        let what = format!("trigger `{}`", raw.name);
        self.name(file, "trigger", &raw.name);
        if raw.rig.is_some() && !project {
            self.fail(
                file,
                format!("{what}: a pack trigger gets its rig from the project, not `rig`"),
            );
        }
        if raw.on.is_none() && !raw.filter.is_empty() {
            self.fail(file, format!("{what}: `where` needs `on`"));
        }
        let source = match (raw.on, raw.cron, raw.every) {
            (Some(on), None, None) => {
                let filter = Filter::parse(&raw.filter)
                    .map_err(|e| self.fail(file, format!("{what}: {e}")))
                    .ok()?;
                match event_path(&on, "seq", false) {
                    None => self.fail(file, format!("{what}: unknown event type `{on}`")),
                    // A skip that fires a trigger could skip again in the same transaction forever.
                    Some(_) if on == "trigger.skipped" => self.fail(file, format!("{what}: no trigger on `{on}`")),
                    Some(_) => {
                        for bad in filter.paths().filter(|p| event_path(&on, p, true) != Some(true)) {
                            self.fail(file, format!("{what}: `{on}` has no `{bad}`"));
                        }
                    }
                }
                Source::Event { on, filter }
            }
            (None, Some(cron), None) => match Cron::parse(&cron) {
                Ok(cron) => Source::Cron(cron),
                Err(e) => return self.none(file, format!("{what}: {e}")),
            },
            (None, None, Some(every)) => Source::Every(duration(&every).map_err(|e| self.fail(file, e)).ok()?),
            _ => return self.none(file, format!("{what}: needs exactly one of `on`, `cron`, `every`")),
        };
        let on = match &source {
            Source::Event { on, .. } => Some(on.as_str()),
            _ => None,
        };
        let inputs = Inputs::parse(&Value::Object(raw.inputs), on.is_some())
            .map_err(|e| self.fail(file, format!("{what}: {e}")))
            .ok()?;
        for path in inputs.paths() {
            let inner = path.strip_prefix("event.").unwrap_or_default();
            if on.is_some_and(|on| event_path(on, inner, false) != Some(true)) {
                self.fail(file, format!("{what}: `{}` has no `{inner}`", on.unwrap_or_default()));
            }
        }
        Some(Trigger {
            name: raw.name,
            source,
            run: reference(&raw.run),
            inputs,
            rig: raw.rig,
        })
    }
}

/// Whether a prompt template path can be resolved for a run of a formula with `inputs` (contracts 2.3).
fn prompt_path(path: &str, inputs: &BTreeMap<String, super::Input>) -> bool {
    let (root, rest) = path.split_once('.').unwrap_or((path, ""));
    let first = rest.split('.').next().unwrap_or_default();
    match root {
        "inputs" => inputs.contains_key(first),
        "bead" => inputs.values().any(|i| i.kind == InputType::Bead) && bead_path(rest),
        "run" => run_path(rest),
        "project" => matches!(rest, "name" | "path"),
        // `name`, `path` or a variable of the rig, which is known only when the run starts.
        _ => !rest.is_empty() && !rest.contains('.'),
    }
}
