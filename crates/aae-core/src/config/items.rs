//! The content of a pack: agents (spec §8.1), formulas (§9.1) and triggers (§9.5), each checked on its own.

use super::{
    Agent, Formula, Settings, Source, Step, StepKind, Trigger, duration,
    files::TriggerFile,
    formula_file::{FormulaFile, InputType, StepFile},
    load::{Loader, reference},
    schema::{bead_path, event_path, run_path},
};
use crate::{
    cron::Cron,
    template::{Filter, Inputs, Template},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

/// The roots of a prompt template (contracts 2.3).
const PROMPT_ROOTS: &[&str] = &["inputs", "bead", "rig", "run", "project", "steps", "remarks"];

impl Loader {
    pub(super) fn agent(&mut self, dir: &Path, plugins: &[&str]) -> Option<Agent> {
        let file = dir.join("agent.toml");
        let mut table: toml::Table = self.read(&file)?;
        let (harness, prompt) = (string(&mut table, "harness"), string(&mut table, "prompt"));
        let (harness, prompt, (settings, max_runs)) = match (harness, prompt, agent_keys(table)) {
            (Ok(h), Ok(p), Ok(rest)) => (h, p, rest),
            (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => return self.none(&file, e),
        };
        if let Some(h) = harness.as_ref().filter(|h| !self.harnesses.contains(*h)) {
            self.fail(
                &file,
                format!("harness `{h}` is neither of this build nor in the orchestrator config"),
            );
        }
        let prompt = match prompt {
            Some(p) => self.text(&file, &dir.join(p)),
            None if !dir.join("prompt.md").exists() => None,
            None => self.text(&file, &dir.join("prompt.md")),
        };
        let mut settings = self.settings(&file, dir, settings, plugins);
        // The role prompt is the agent layer's system prompt (spec §8.1, §6.3): one value, so not both.
        if prompt.is_some() && settings.system_prompt.is_some() {
            self.fail(&file, "`system_prompt` and a role prompt file are both set; keep one");
        }
        settings.system_prompt = settings.system_prompt.or(prompt);
        Some(Agent {
            harness,
            settings,
            max_runs,
        })
    }

    /// The formulas of a pack: every `.toml` file under `dir`, subdirectories included.
    pub(super) fn formulas(&mut self, dir: &Path) -> BTreeMap<String, Formula> {
        let (mut formulas, mut files) = (BTreeMap::new(), self.entries(dir));
        while let Some(file) = files.pop() {
            if file.is_dir() {
                files.extend(self.entries(&file));
            }
            if file.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            let Some((formula, f)) = self.formula(&file) else {
                continue;
            };
            if formulas.insert(formula.clone(), f).is_some() {
                self.fail(&file, format!("two formulas of the pack are named `{formula}`"));
            }
        }
        formulas
    }

    /// The entries of a directory in name order; a missing directory has none.
    pub(super) fn entries(&mut self, dir: &Path) -> Vec<PathBuf> {
        if !dir.exists() {
            return Vec::new();
        }
        match fs::read_dir(dir).and_then(|d| d.map(|e| e.map(|e| e.path())).collect::<Result<Vec<_>, _>>()) {
            Ok(mut paths) => {
                paths.sort();
                paths
            }
            Err(e) => {
                self.fail(dir, e.to_string());
                Vec::new()
            }
        }
    }

    /// The text of a prompt file: a prompt is never empty (spec §6.3, §8.1).
    pub(super) fn text(&mut self, file: &Path, path: &Path) -> Option<String> {
        match fs::read_to_string(path) {
            Ok(text) if text.trim().is_empty() => self.none(file, format!("`{}` is empty", path.display())),
            Ok(text) => Some(text),
            Err(e) => self.none(file, format!("`{}`: {e}", path.display())),
        }
    }

    pub(super) fn formula(&mut self, file: &Path) -> Option<(String, Formula)> {
        let raw: FormulaFile = self.read(file)?;
        self.name(file, "formula", &raw.name);
        if raw.step.is_empty() {
            return self.none(file, "a formula has at least one `[[step]]`");
        }
        let mut steps: Vec<Step> = Vec::new();
        let mut variables = BTreeMap::new();
        for name in raw.inputs.keys() {
            variables.insert(super::input_variable(name), format!("input `{name}`"));
        }
        for (at, step) in raw.step.into_iter().enumerate() {
            let (name, back_to) = match &step {
                StepFile::Ai { name, back_to, .. } | StepFile::Exec { name, back_to, .. } => {
                    (name.clone(), back_to.clone())
                }
                StepFile::Drain { name, .. } => (name.clone(), None),
            };
            let name = name.unwrap_or_else(|| (at + 1).to_string());
            if let Some(to) = back_to.as_ref().filter(|to| !steps.iter().any(|s| &s.name == *to)) {
                self.fail(
                    file,
                    format!("step `{name}` goes back to `{to}`, which is no step before it"),
                );
            }
            self.name(file, "step", &name);
            let variable = format!("AAE_STEP_{}_", super::upper(&name));
            if steps.iter().any(|s| s.name == name) {
                self.fail(file, format!("two steps are named `{name}`"));
            } else if let Some(other) = variables.insert(variable.clone(), format!("step `{name}`")) {
                self.fail(file, format!("{other} and step `{name}` are both `{variable}*`"));
            }
            let kind = self.step(file, &raw.inputs, &steps, step)?;
            steps.push(Step { name, kind, back_to });
        }
        // The bead a drain step gives out beads under (spec §9.1).
        let drains = steps.iter().any(|s| matches!(s.kind, StepKind::Drain { .. }));
        if drains && !raw.inputs.values().any(|i| i.kind == InputType::Bead && i.required) {
            self.fail(
                file,
                "a formula with a `drain` step needs a required input of type `bead`",
            );
        }
        if steps.iter().any(|s| matches!(s.kind, StepKind::Exec { .. })) {
            let mut seen = BTreeMap::new();
            for name in raw.inputs.keys() {
                if let Some(other) = seen.insert(super::input_variable(name), name) {
                    self.fail(
                        file,
                        format!(
                            "inputs `{other}` and `{name}` are both `{}`",
                            super::input_variable(name)
                        ),
                    );
                }
            }
        }
        Some((
            raw.name,
            Formula {
                description: raw.description,
                inputs: raw.inputs,
                worktree: raw.worktree,
                rounds: raw.rounds.map_or(3, std::num::NonZeroU32::get),
                steps,
            },
        ))
    }

    /// A step of formula `file`, after the steps `before`.
    fn step(
        &mut self,
        file: &Path,
        inputs: &BTreeMap<String, super::Input>,
        before: &[Step],
        raw: StepFile,
    ) -> Option<StepKind> {
        Some(match raw {
            StepFile::Ai {
                agent,
                prompt,
                timeout,
                harness,
                model,
                effort,
                ..
            } => {
                let launch = super::compose::Launch { harness, model, effort };
                let settings = self.settings(file, &parent(file), launch.into(), &[]);
                let path = resolve(&parent(file), &prompt);
                let text = self.text(file, &path)?;
                let template = Template::parse(&text, PROMPT_ROOTS)
                    .map_err(|e| self.fail(&path, e.0))
                    .ok()?;
                for bad in template.paths().filter(|p| !prompt_path(p, inputs, before)) {
                    self.fail(
                        &path,
                        format!("`{{{{ {bad} }}}}` is not a value of this formula's runs"),
                    );
                }
                let timeout = self.timeout(file, timeout.as_deref());
                StepKind::Ai {
                    agent: reference(&agent),
                    prompt: template,
                    timeout,
                    settings: Box::new(settings),
                }
            }
            StepFile::Exec { command, timeout, .. } => {
                if command.is_empty() {
                    self.fail(file, "`command` is empty");
                }
                StepKind::Exec {
                    command,
                    timeout: self.timeout(file, timeout.as_deref()),
                }
            }
            StepFile::Drain { formula, timeout, .. } => StepKind::Drain {
                formula: reference(&formula),
                timeout: self.timeout(file, timeout.as_deref()),
            },
        })
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

/// Takes the string `key` out of `table`.
pub(super) fn string(table: &mut toml::Table, key: &str) -> Result<Option<String>, String> {
    match table.remove(key) {
        Some(toml::Value::String(s)) => Ok(Some(s)),
        None => Ok(None),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

/// The settings and `max_runs` of an agent's keys, the rest of an `agent.toml` or of an `[agents.<name>]` table.
pub(super) fn agent_keys(mut table: toml::Table) -> Result<(Settings, Option<u32>), String> {
    let max_runs = match table.remove("max_runs") {
        None => None,
        Some(toml::Value::Integer(n)) if u32::try_from(n).is_ok_and(|n| n > 0) => u32::try_from(n).ok(),
        Some(_) => return Err("`max_runs` must be a positive 32-bit integer".to_owned()),
    };
    let settings = toml::Value::Table(table)
        .try_into::<Settings>()
        .map_err(|e| super::load::retired(e.to_string()))?;
    Ok((settings, max_runs))
}

/// Whether a prompt template path can be resolved for a run of a formula with `inputs` (contracts 2.3).
fn prompt_path(path: &str, inputs: &BTreeMap<String, super::Input>, before: &[Step]) -> bool {
    let (root, rest) = path.split_once('.').unwrap_or((path, ""));
    let first = rest.split('.').next().unwrap_or_default();
    match root {
        "inputs" => inputs.contains_key(first),
        "bead" => inputs.values().any(|i| i.kind == InputType::Bead) && bead_path(rest),
        "run" => run_path(rest),
        "project" => matches!(rest, "name" | "path"),
        // The remarks of the checks that sent the run back; empty in the first round (spec §9.1).
        "remarks" => rest.is_empty(),
        // The outcome of an earlier step: `steps.<name>.result`, `.summary` or `.data` and below.
        "steps" => {
            let mut fields = rest.split('.').skip(1);
            let field = matches!(
                (fields.next(), fields.next()),
                (Some("result" | "summary"), None) | (Some("data"), _)
            );
            field && before.iter().any(|s| s.name == first)
        }
        // `name`, `path` or a variable of the rig, which is known only when the run starts.
        _ => !rest.is_empty() && !rest.contains('.'),
    }
}

pub(super) fn parent(file: &Path) -> PathBuf {
    file.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// `path` relative to `base`, with a leading `~/` meaning the home directory, without `.` and `..`.
pub(super) fn resolve(base: &Path, path: &Path) -> PathBuf {
    let joined = match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => base.join(path),
    };
    // Lexically, as written: `.` goes and `..` takes the name before it; a symlink stays as it is.
    let mut out = PathBuf::new();
    for part in joined.components() {
        match (part, out.components().next_back()) {
            (Component::CurDir, _) | (Component::ParentDir, Some(Component::RootDir)) => {}
            (Component::ParentDir, Some(Component::Normal(_))) => {
                out.pop();
            }
            (part, _) => out.push(part),
        }
    }
    out
}
