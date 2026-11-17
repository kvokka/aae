//! References between files: names resolved to their packs (docs/design/config.md), trigger inputs against their
//! formula, rigs and agent edits against what the project has; the tables of the bundled addons; the watchdog table.

use super::{BUNDLED, Bundled, Name, Pack, Project, StepKind, Variant, Watchdog, load::Loader};
use crate::template::Inputs;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};

pub(super) fn project(loader: &mut Loader, project: &mut Project, variants: &BTreeMap<String, Variant>) {
    let index = project.packs.clone();
    let Project {
        dir,
        root,
        edits,
        rigs,
        packs,
        variants: own_variants,
        ..
    } = project;
    let has_agent = |p: &Pack, n: &str| p.agents.contains_key(n);
    let has_formula = |p: &Pack, n: &str| p.formulas.contains_key(n);
    let has_trigger = |p: &Pack, n: &str| p.triggers.iter().any(|t| t.name == n);
    for pack in packs.iter_mut() {
        let own = pack.name.clone();
        let manifest = pack.root.join(if pack.version.is_some() {
            "pack.toml"
        } else {
            "aae.toml"
        });
        for (name, formula) in &mut pack.formulas {
            let own_inputs = formula.inputs.clone();
            for step in &mut formula.steps {
                match &mut step.kind {
                    StepKind::Ai { agent, .. } => match resolve(&index, &own, agent, has_agent, "agent") {
                        Ok(found) => *agent = found,
                        Err(e) => loader.fail(&manifest, format!("formula `{name}`: {e}")),
                    },
                    StepKind::Drain { formula: run, .. } => {
                        let what = format!("formula `{name}`, step `{}`", step.name);
                        match resolve(&index, &own, run, has_formula, "formula") {
                            Ok(found) => {
                                let pack = index.iter().find(|p| p.name == found.pack).expect("resolved");
                                if let Err(e) = executor(&pack.formulas[&found.name]) {
                                    loader.fail(&manifest, format!("{what}: `{found}` {e}"));
                                }
                                *run = found;
                            }
                            Err(e) => loader.fail(&manifest, format!("{what}: {e}")),
                        }
                    }
                    StepKind::Formula {
                        formula: run, inputs, ..
                    } => {
                        let what = format!("formula `{name}`, step `{}`", step.name);
                        match resolve(&index, &own, run, has_formula, "formula") {
                            Ok(found) => {
                                let pack = index.iter().find(|p| p.name == found.pack).expect("resolved");
                                let child = &pack.formulas[&found.name];
                                for e in step_inputs(child, &own_inputs, inputs) {
                                    loader.fail(&manifest, format!("{what}: `{found}` {e}"));
                                }
                                *run = found;
                            }
                            Err(e) => loader.fail(&manifest, format!("{what}: {e}")),
                        }
                    }
                    StepKind::Exec { .. } | StepKind::Await { .. } => {}
                }
            }
        }
        for trigger in &mut pack.triggers {
            let what = format!("trigger `{}`", trigger.name);
            match resolve(&index, &own, &trigger.run, has_formula, "formula") {
                Ok(found) => {
                    let formula = &index.iter().find(|p| p.name == found.pack).expect("resolved").formulas[&found.name];
                    if formula.internal {
                        loader.fail(&manifest, format!("{what}: formula `{found}` is internal"));
                    }
                    let given: Vec<&str> = match &trigger.inputs {
                        Inputs::Table(entries) => entries.iter().map(|(k, _)| k.as_str()).collect(),
                        _ => Vec::new(),
                    };
                    for unknown in given.iter().filter(|k| !formula.inputs.contains_key(**k)) {
                        loader.fail(&manifest, format!("{what}: formula `{found}` has no input `{unknown}`"));
                    }
                    for (input, _) in formula
                        .inputs
                        .iter()
                        .filter(|(k, _)| formula.needs(k) && !given.contains(&k.as_str()))
                    {
                        loader.fail(
                            &manifest,
                            format!("{what}: required input `{input}` of `{found}` is not given"),
                        );
                    }
                    trigger.run = found;
                }
                Err(e) => loader.fail(&manifest, format!("{what}: {e}")),
            }
            if let Some(rig) = trigger.rig.as_ref().filter(|r| **r != root.0 && !rigs.contains_key(*r)) {
                loader.fail(&manifest, format!("{what}: no rig `{rig}`"));
            }
        }
    }
    let file = dir.join("aae.toml");
    if let Some(cycle) = self_call(packs) {
        loader.fail(
            &file,
            format!("formula `{cycle}` runs itself through its `formula` steps"),
        );
    }
    // Project-level references resolve as if written in the project root pack.
    let own = index.first().map(|p| p.name.clone()).unwrap_or_default();
    let mut agents: Vec<&String> = edits.keys().collect();
    for (rig, r) in rigs.iter_mut() {
        agents.extend(r.edits.keys());
        for trigger in &mut r.triggers {
            match resolve(&index, &own, trigger, has_trigger, "trigger") {
                Ok(found) => *trigger = found,
                Err(e) => loader.fail(&file, format!("rig `{rig}`: {e}")),
            }
        }
    }
    // An edit names a pack agent or a variant, by its name.
    for agent in agents
        .into_iter()
        .filter(|a| !own_variants.contains_key(*a) && !variants.contains_key(*a))
    {
        let name = super::load::reference(agent);
        if let Err(e) = resolve(&index, &own, &name, has_agent, "agent") {
            loader.fail(&file, format!("`agents.{agent}`: {e}{NEW_NAME}"));
        }
    }
}

/// What an `[agents.<name>]` table of an unknown name lacks.
const NEW_NAME: &str = "; a table of a new name is a variant and needs `base`";

/// An orchestrator edit (`[agents.<name>]` without `base`) must name an agent of at least one project and be
/// ambiguous in none.
pub(super) fn orchestrator_edits<'a>(
    loader: &mut Loader,
    file: &std::path::Path,
    keys: impl Iterator<Item = &'a String>,
    projects: &[Project],
    variants: &BTreeMap<String, Variant>,
) {
    let has_agent = |p: &Pack, n: &str| p.agents.contains_key(n);
    let variant = |key: &String| variants.contains_key(key) || projects.iter().any(|p| p.variants.contains_key(key));
    for key in keys.filter(|k| !variant(k)) {
        let name = super::load::reference(key);
        let found: Vec<Result<Name, String>> = projects
            .iter()
            .map(|p| {
                let own = p.packs.first().map(|pack| pack.name.as_str()).unwrap_or_default();
                resolve(&p.packs, own, &name, has_agent, "agent")
            })
            .collect();
        if let Some(Err(e)) = found
            .iter()
            .find(|r| r.as_ref().is_err_and(|e| e.contains("ambiguous")))
        {
            loader.fail(file, format!("`agents.{key}`: {e}"));
        } else if !found.iter().any(Result::is_ok) {
            loader.fail(file, format!("`agents.{key}`: no project has agent `{key}`{NEW_NAME}"));
        }
    }
}

/// A bare name resolves to the referring pack's own item first, then to the only pack that has it.
pub(super) fn resolve(
    packs: &[Pack],
    own: &str,
    name: &Name,
    has: impl Fn(&Pack, &str) -> bool,
    what: &str,
) -> Result<Name, String> {
    let found = |p: &&Pack| has(p, &name.name);
    let qualified = |p: &Pack| Name {
        pack: p.name.clone(),
        name: name.name.clone(),
    };
    if !name.pack.is_empty() {
        return packs
            .iter()
            .filter(|p| p.name == name.pack)
            .find(found)
            .map(qualified)
            .ok_or_else(|| format!("no {what} `{name}`"));
    }
    if let Some(p) = packs.iter().filter(|p| p.name == own).find(found) {
        return Ok(qualified(p));
    }
    match packs.iter().filter(found).collect::<Vec<_>>().as_slice() {
        [p] => Ok(qualified(p)),
        [] => Err(format!("no {what} `{}`", name.name)),
        many => {
            let forms: Vec<String> = many.iter().map(|p| qualified(p).to_string()).collect();
            Err(format!(
                "{what} `{}` is ambiguous, write one of: {}",
                name.name,
                forms.join(", ")
            ))
        }
    }
}

/// A formula a drain step runs takes the bead and nothing else it needs: one input of type `task`, no other
/// required one (docs/design/config.md).
fn executor(formula: &super::Formula) -> Result<(), String> {
    let beads = formula.inputs.iter().filter(|(_, i)| i.kind == super::InputType::Task);
    if beads.count() != 1 {
        return Err("needs exactly one input of type `task` to run a drained bead".to_owned());
    }
    match formula
        .inputs
        .iter()
        .find(|(k, i)| formula.needs(k) && i.kind != super::InputType::Task)
    {
        Some((input, _)) => Err(format!("requires input `{input}` that a drain step does not give")),
        None => Ok(()),
    }
}

/// The bundled addons `[builtin.<name>]` leaves on, in the order of `BUNDLED`.
pub(super) fn bundled(
    loader: &mut Loader,
    file: &Path,
    mut tables: BTreeMap<String, Map<String, Value>>,
) -> Vec<Bundled> {
    if tables.remove("worktree").is_some() {
        loader.fail(
            file,
            "`[builtin.worktree]` is gone: set `worktree = true` in the formulas whose runs need a worktree",
        );
    }
    let mut on = Vec::new();
    for (name, events, by_default) in BUNDLED {
        let mut params = tables.remove(name).unwrap_or_default();
        let enabled = match params.remove("enabled") {
            None => by_default || !params.is_empty(),
            Some(Value::Bool(enabled)) => enabled,
            Some(_) => {
                loader.fail(file, format!("[builtin.{name}]: `enabled` is a boolean"));
                false
            }
        };
        if enabled {
            on.push(Bundled {
                name,
                on: events,
                params: Value::Object(params),
            });
        }
    }
    for name in tables.keys() {
        loader.fail(file, format!("`{name}` is not a bundled addon"));
    }
    on
}

/// The bundled addons on for a project: the orchestrator's `on`, except those the project's `tables` decide, and
/// `worktree` for the formulas of its `packs` with `worktree = true`, named in its parameters.
pub(super) fn project_bundled(
    loader: &mut Loader,
    file: &Path,
    on: &[Bundled],
    tables: BTreeMap<String, Map<String, Value>>,
    packs: &[Pack],
) -> Vec<Bundled> {
    let own: Vec<String> = tables.keys().cloned().collect();
    let decides = |b: &Bundled| own.iter().any(|n| n == b.name);
    let mine = bundled(loader, file, tables).into_iter().filter(decides);
    let mut all: Vec<Bundled> = on.iter().filter(|b| !decides(b)).cloned().chain(mine).collect();
    let worktree: Vec<String> = (packs.iter())
        .flat_map(|p| {
            p.formulas
                .iter()
                .filter(|(_, f)| f.worktree)
                .map(|(n, _)| format!("{}.{n}", p.name))
        })
        .collect();
    if let Some(&(name, on, _)) = BUNDLED.iter().find(|b| b.0 == "worktree" && !worktree.is_empty()) {
        let params = serde_json::json!({ "formulas": worktree });
        all.push(Bundled { name, on, params });
    }
    all.sort_by_key(|b| BUNDLED.iter().position(|(n, ..)| *n == b.name));
    all
}

/// `[watchdog]` over the defaults of contracts 6.1; each threshold is from 1 ms to 8760 h, which keeps every
/// `now + threshold` far from overflow.
pub(super) fn watchdog(loader: &mut Loader, file: &Path, table: BTreeMap<String, String>) -> Watchdog {
    const MAX_MS: u128 = 8760 * 3_600_000;
    let mut w = Watchdog {
        silence_ms: 15 * 60_000,
        tool_silence_ms: 40 * 60_000,
        background_silence_ms: 120 * 60_000,
        start_timeout_ms: 120_000,
        queue_timeout_ms: 7 * 86_400_000,
        park_timeout_ms: 7 * 86_400_000,
    };
    for (key, text) in table {
        let field = match key.as_str() {
            "silence" => &mut w.silence_ms,
            "tool_silence" => &mut w.tool_silence_ms,
            "background_silence" => &mut w.background_silence_ms,
            "start_timeout" => &mut w.start_timeout_ms,
            "queue_timeout" => &mut w.queue_timeout_ms,
            "park_timeout" => &mut w.park_timeout_ms,
            _ => {
                loader.fail(file, format!("[watchdog]: unknown key `{key}`"));
                continue;
            }
        };
        match super::duration(&text).map(|d| d.as_millis()) {
            Ok(ms @ 1..=MAX_MS) => *field = ms as i64,
            Ok(_) => loader.fail(file, format!("[watchdog]: `{key}` is from 1ms to 8760h")),
            Err(e) => loader.fail(file, format!("[watchdog]: `{key}`: {e}")),
        }
    }
    w
}

/// What is wrong with the inputs a `formula` step gives `child`: the ones it names that `child` has not, the
/// required ones of `child` neither given nor inherited.
fn step_inputs(child: &super::Formula, parent: &BTreeMap<String, super::Input>, given: &Inputs) -> Vec<String> {
    let given: Vec<&str> = match given {
        Inputs::Table(entries) => entries.iter().map(|(k, _)| k.as_str()).collect(),
        _ => Vec::new(),
    };
    let unknown = (given.iter()).filter(|k| !child.inputs.contains_key(**k));
    let mut errors: Vec<String> = unknown.map(|k| format!("has no input `{k}`")).collect();
    let inherited = |k: &str| parent.get(k).is_some_and(|i| i.required);
    for k in child.inputs.keys() {
        if child.needs(k) && !given.contains(&k.as_str()) && !inherited(k) {
            errors.push(format!("needs input `{k}`, which the step neither gives nor inherits"));
        }
    }
    errors
}

/// A formula that runs itself through `formula` steps, directly or through others; `None` when there is none.
fn self_call(packs: &[Pack]) -> Option<Name> {
    let calls = |at: &Name| -> Vec<Name> {
        let formula = packs
            .iter()
            .find(|p| p.name == at.pack)
            .and_then(|p| p.formulas.get(&at.name));
        let steps = formula.map(|f| f.steps.as_slice()).unwrap_or_default();
        (steps.iter())
            .filter_map(|s| match &s.kind {
                StepKind::Formula { formula, .. } => Some(formula.clone()),
                _ => None,
            })
            .collect()
    };
    for pack in packs {
        for name in pack.formulas.keys() {
            let start = Name {
                pack: pack.name.clone(),
                name: name.clone(),
            };
            // Depth-first from `start`; a path back to it is a cycle. The visited set keeps it finite.
            let (mut stack, mut seen) = (calls(&start), Vec::new());
            while let Some(next) = stack.pop() {
                if next == start {
                    return Some(start);
                }
                if !seen.contains(&next) {
                    stack.extend(calls(&next));
                    seen.push(next);
                }
            }
        }
    }
    None
}
