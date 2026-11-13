//! References between files: names resolved to their packs (spec §8.2), trigger inputs against their formula,
//! rigs and overrides against what the project has; the tables of the bundled plugins.

use super::{BUNDLED, Bundled, Name, Pack, Project, Step, load::Loader};
use crate::template::Inputs;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};

pub(super) fn project(loader: &mut Loader, project: &mut Project) {
    let index = project.packs.clone();
    let Project {
        root,
        overrides,
        rigs,
        packs,
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
            if let Step::Prompt { agent, .. } = &mut formula.step {
                match resolve(&index, &own, agent, has_agent, "agent") {
                    Ok(found) => *agent = found,
                    Err(e) => loader.fail(&manifest, format!("formula `{name}`: {e}")),
                }
            }
        }
        for trigger in &mut pack.triggers {
            let what = format!("trigger `{}`", trigger.name);
            match resolve(&index, &own, &trigger.run, has_formula, "formula") {
                Ok(found) => {
                    let formula = &index.iter().find(|p| p.name == found.pack).expect("resolved").formulas[&found.name];
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
                        .filter(|(k, i)| i.required && !given.contains(&k.as_str()))
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
            if let Some(rig) = trigger.rig.as_ref().filter(|r| !rigs.contains_key(*r)) {
                loader.fail(&manifest, format!("{what}: no rig `{rig}`"));
            }
        }
    }
    let file = root.join("aae.toml");
    // Project-level references resolve as if written in the project root pack.
    let own = index.first().map(|p| p.name.clone()).unwrap_or_default();
    let mut agents: Vec<&String> = overrides.keys().collect();
    for (rig, r) in rigs.iter_mut() {
        agents.extend(r.overrides.keys());
        for trigger in &mut r.triggers {
            match resolve(&index, &own, trigger, has_trigger, "trigger") {
                Ok(found) => *trigger = found,
                Err(e) => loader.fail(&file, format!("rig `{rig}`: {e}")),
            }
        }
    }
    for agent in agents {
        let name = super::load::reference(agent);
        if let Err(e) = resolve(&index, &own, &name, has_agent, "agent") {
            loader.fail(&file, format!("overrides: {e}"));
        }
    }
}

/// An orchestrator override must name an agent of at least one project and be ambiguous in none.
pub(super) fn orchestrator_overrides<'a>(
    loader: &mut Loader,
    file: &std::path::Path,
    keys: impl Iterator<Item = &'a String>,
    projects: &[Project],
) {
    let has_agent = |p: &Pack, n: &str| p.agents.contains_key(n);
    for key in keys {
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
            loader.fail(file, format!("overrides: {e}"));
        } else if !found.iter().any(Result::is_ok) {
            loader.fail(file, format!("overrides: no project has agent `{key}`"));
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

/// The bundled plugins `[builtin.<name>]` leaves on, in the order of `BUNDLED`.
pub(super) fn bundled(
    loader: &mut Loader,
    file: &Path,
    mut tables: BTreeMap<String, Map<String, Value>>,
) -> Vec<Bundled> {
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
        loader.fail(file, format!("`{name}` is not a bundled plugin"));
    }
    on
}

/// The bundled plugins on for a project: the orchestrator's `on`, except those the project's `tables` decide.
pub(super) fn project_bundled(
    loader: &mut Loader,
    file: &Path,
    on: &[Bundled],
    tables: BTreeMap<String, Map<String, Value>>,
) -> Vec<Bundled> {
    let own: Vec<String> = tables.keys().cloned().collect();
    let decides = |b: &Bundled| own.iter().any(|n| n == b.name);
    let mine = bundled(loader, file, tables).into_iter().filter(decides);
    let mut all: Vec<Bundled> = on.iter().filter(|b| !decides(b)).cloned().chain(mine).collect();
    all.sort_by_key(|b| BUNDLED.iter().position(|(n, ..)| *n == b.name));
    all
}
