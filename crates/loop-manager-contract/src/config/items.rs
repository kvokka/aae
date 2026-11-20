//! The content of a pack: agents (docs/design/config.md), formulas (docs/design/config.md) and triggers
//! (docs/design/config.md), each checked on its own.

use super::{
    Agent, Formula, Settings, Source, Step, StepKind, Trigger, duration,
    files::TriggerFile,
    formula_file::{FormulaFile, InputType, StepFile},
    load::{Loader, reference},
    schema::event_path,
    variants::{agent_keys, string},
};
use crate::{
    cron::Cron,
    template::{Filter, Inputs},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

impl Loader {
    pub(super) fn agent(&mut self, dir: &Path, addons: &[&super::Addon]) -> Option<Agent> {
        let file = dir.join("agent.toml");
        let mut table: toml::Table = self.read(&file)?;
        let scope = table
            .remove("scope")
            .map_or(Ok(super::Scope::Rig), toml::Value::try_into);
        let Ok(scope) = scope else {
            return self.none(&file, "`scope` is `rig` or `root`".to_owned());
        };
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
        let mut settings = self.settings(&file, dir, settings, addons);
        // The role prompt is the agent layer's system prompt (docs/design/config.md): one value, so not both.
        if prompt.is_some() && settings.system_prompt.is_some() {
            self.fail(&file, "`system_prompt` and a role prompt file are both set; keep one");
        }
        settings.system_prompt = settings.system_prompt.or(prompt);
        Some(Agent {
            dir: dir.to_path_buf(),
            harness,
            scope,
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
            let (name, back_to, max_attempts, script) = match &step {
                StepFile::Ai {
                    name,
                    back_to,
                    max_attempts,
                    step_check_script_path: script,
                    ..
                }
                | StepFile::Exec {
                    name,
                    back_to,
                    max_attempts,
                    step_check_script_path: script,
                    ..
                } => (name.clone(), back_to.clone(), *max_attempts, script.clone()),
                StepFile::Drain {
                    name,
                    max_attempts,
                    step_check_script_path: script,
                    ..
                }
                | StepFile::Formula {
                    name,
                    max_attempts,
                    step_check_script_path: script,
                    ..
                } => (name.clone(), None, *max_attempts, script.clone()),
                StepFile::Await { name, back_to, .. } => (name.clone(), back_to.clone(), None, None),
            };
            let name = name.unwrap_or_else(|| (at + 1).to_string());
            if let Some(to) = back_to.as_ref().filter(|to| !steps.iter().any(|s| &s.name == *to)) {
                self.fail(
                    file,
                    format!("step `{name}` goes back to `{to}`, which is no step before it"),
                );
            }
            self.name(file, "step", &name);
            let variable = format!("LOOP_MANAGER_STEP_{}_", super::upper(&name));
            if steps.iter().any(|s| s.name == name) {
                self.fail(file, format!("two steps are named `{name}`"));
            } else if let Some(other) = variables.insert(variable.clone(), format!("step `{name}`")) {
                self.fail(file, format!("{other} and step `{name}` are both `{variable}*`"));
            }
            let check = script
                .filter(|s| !s.is_empty())
                .map(|s| resolve(&parent(file), Path::new(&s)));
            if let Some(path) = check.as_ref().filter(|p| !executable(p)) {
                let path = path.display();
                self.fail(
                    file,
                    format!("step `{name}`: the check script `{path}` is not an executable file"),
                );
            }
            // A repeat spends no session attempt, so the step's own count takes the same ceiling (invariant 16).
            let ceiling = super::MAX_ATTEMPTS;
            if max_attempts.is_some_and(|n| n.get() > ceiling) {
                self.fail(file, format!("step `{name}`: `max_attempts` is at most {ceiling}"));
            }
            let condition = super::prompt::step_if(step.condition(), &raw.inputs, &steps)
                .map_err(|e| self.fail(file, format!("step `{name}`: {e}")))
                .ok()?;
            let kind = self.step((file, &raw.name), step, &raw.inputs, &steps)?;
            steps.push(Step {
                name,
                kind,
                back_to,
                max_attempts: max_attempts.map_or(1, std::num::NonZeroU32::get),
                check,
                condition,
            });
        }
        self.check_formula_inputs(file, &raw.inputs, &steps);
        let own = raw.rounds.map(|n| (n.get(), super::Layer::Formula(raw.name.clone())));
        let (value, from) = own.unwrap_or((3, super::Layer::Default));
        Some((
            raw.name,
            Formula {
                description: raw.description,
                inputs: raw.inputs,
                worktree: raw.worktree,
                internal: raw.internal,
                rounds: super::Sourced { value, from },
                defaults: BTreeMap::new(),
                steps,
            },
        ))
    }

    fn check_formula_inputs(
        &mut self,
        file: &Path,
        inputs: &BTreeMap<String, super::formula_file::Input>,
        steps: &[Step],
    ) {
        for bad in super::prompt::strangers(steps) {
            self.fail(file, bad);
        }
        // The task bead a drain step gives out beads under (docs/design/config.md).
        let drains = steps.iter().any(|s| matches!(s.kind, StepKind::Drain { .. }));
        if drains && !inputs.values().any(|i| i.kind == InputType::Task && i.required) {
            self.fail(
                file,
                "a formula with a `drain` step needs a required input of type `task`",
            );
        }
        if steps.iter().any(|s| matches!(s.kind, StepKind::Exec { .. })) {
            let mut seen = BTreeMap::new();
            for name in inputs.keys() {
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
    }

    /// A step of formula `of` in `file`.
    fn step(
        &mut self,
        (file, of): (&Path, &str),
        raw: StepFile,
        own: &BTreeMap<String, super::formula_file::Input>,
        before: &[Step],
    ) -> Option<StepKind> {
        Some(match raw {
            StepFile::Ai {
                agent,
                timeout,
                harness,
                model,
                effort,
                native,
                last_comments,
                ..
            } => {
                let launch = super::compose::Launch { harness, model, effort };
                let settings = Settings {
                    native,
                    ..launch.into()
                };
                let settings = self.settings(file, &parent(file), settings, &[]);
                StepKind::Ai {
                    agent: reference(&agent),
                    timeout: self.timeout(file, timeout.as_deref()),
                    settings: Box::new(settings),
                    last_comments,
                }
            }
            StepFile::Exec {
                command,
                timeout,
                inherit_env,
                env,
                ..
            } => {
                if command.is_empty() {
                    self.fail(file, "`command` is empty");
                }
                self.env_values(file, "env", &env);
                let mut own = super::ExecEnv::default();
                let layer = super::Layer::Formula(of.to_owned());
                own.under(&super::env::ExecFile { inherit_env, env }, &layer);
                StepKind::Exec {
                    command,
                    timeout: self.timeout(file, timeout.as_deref()),
                    env: own,
                }
            }
            StepFile::Drain {
                formula,
                timeout,
                pool,
                on_item_failure,
                ..
            } => StepKind::Drain {
                formula: reference(&formula),
                timeout: self.timeout(file, timeout.as_deref()),
                pool: pool.map_or(1, std::num::NonZeroU32::get),
                on_item_failure,
            },
            StepFile::Formula {
                formula,
                inputs,
                timeout,
                ..
            } => StepKind::Formula {
                inputs: (super::prompt::step_inputs(inputs, own, before))
                    .map_err(|e| self.fail(file, format!("step `{formula}`: {e}")))
                    .ok()?,
                formula: reference(&formula),
                timeout: self.timeout(file, timeout.as_deref()),
            },
            StepFile::Await {
                timeout,
                fail_on_timeout,
                ..
            } => StepKind::Await {
                timeout: self.timeout(file, timeout.as_deref()),
                fail_on_timeout: fail_on_timeout.unwrap_or(true),
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

/// A regular file (a symlink is followed) with an execute bit.
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
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
