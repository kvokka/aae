//! `[agents.<name>]` of the orchestrator and project files — edits of agents and variants of pack agents — and the
//! rigs that run a variant in place of a pack agent (`[rigs.<r>.agents]`): docs/design/config.md "Agent variants".

use super::{
    Name, Pack, Project, Scope, Settings, Variant,
    check::resolve,
    files::retired,
    items::{agent_keys, string},
    load::{Loader, reference},
};
use std::{collections::BTreeMap, path::Path};

impl Loader {
    /// The `[agents.<name>]` tables of one file: the edits of agents by name, and the variants (those with `base`).
    pub(super) fn agents(
        &mut self,
        file: &Path,
        base: &Path,
        tables: BTreeMap<String, toml::Table>,
        plugins: &[&str],
    ) -> (BTreeMap<String, Settings>, BTreeMap<String, Variant>) {
        let (mut edits, mut variants) = (BTreeMap::new(), BTreeMap::new());
        for (name, mut table) in tables {
            let what = format!("agents.{name}");
            let parsed = string(&mut table, "base").and_then(|b| Ok((b, agent_keys(table)?)));
            match parsed {
                Ok((None, (_, Some(_)))) => self.fail(
                    file,
                    format!("`{what}`: `max_runs` is the agent's own; a variant (with `base`) may set its own"),
                ),
                Ok((None, (settings, None))) => {
                    edits.insert(name, self.settings(file, base, settings, plugins));
                }
                Ok((Some(agent), (settings, max_runs))) => {
                    self.name(file, "agent variant", &name);
                    let variant = Variant {
                        base: reference(&agent),
                        settings: self.settings(file, base, settings, plugins),
                        max_runs,
                    };
                    variants.insert(name, variant);
                }
                Err(e) => self.fail(file, format!("`{what}`: {e}")),
            }
        }
        (edits, variants)
    }

    /// `agents` of rig `rig`: the edits of agents on it (tables) and the variants that run in place of agents (names).
    pub(super) fn rig_agents(
        &mut self,
        file: &Path,
        dir: &Path,
        rig: &str,
        tables: BTreeMap<String, toml::Value>,
        plugins: &[&str],
    ) -> (BTreeMap<String, Settings>, BTreeMap<Name, String>) {
        let (mut edits, mut agents) = (BTreeMap::new(), BTreeMap::new());
        for (agent, value) in tables {
            match value {
                // Resolved by `check` once the packs are loaded.
                toml::Value::String(variant) => drop(agents.insert(reference(&agent), variant)),
                toml::Value::Table(table) => match toml::Value::Table(table).try_into::<Settings>() {
                    Ok(s) => drop(edits.insert(agent, self.settings(file, dir, s, plugins))),
                    Err(e) => self.fail(
                        file,
                        format!("rig `{rig}`: `agents.{agent}`: {}", retired(e.to_string())),
                    ),
                },
                _ => self.fail(
                    file,
                    format!("rig `{rig}`: `agents.{agent}` is a table of settings or the name of a variant"),
                ),
            }
        }
        (edits, agents)
    }
}

/// A variant of the orchestrator file has its base in at least one project, and no project has an agent of its name.
pub(super) fn check_orchestrator(
    loader: &mut Loader,
    file: &Path,
    variants: &BTreeMap<String, Variant>,
    projects: &[Project],
) {
    for (name, variant) in variants {
        for p in projects.iter().filter(|p| !p.variants.contains_key(name)) {
            clash(loader, file, name, &p.packs);
        }
        let found = projects.iter().any(|p| base(p, variant).is_ok());
        if !found && !projects.is_empty() {
            let variant_named =
                |n: &str| variants.contains_key(n) || projects.iter().any(|p| p.variants.contains_key(n));
            let why = chained(&variant.base, variant_named)
                .unwrap_or_else(|| format!("no project has agent `{}`", variant.base));
            loader.fail(file, format!("`agents.{name}`: `base`: {why}"));
        }
    }
}

/// The variants of a project and the rig tables that name them: `base` is an agent of the project's packs, a
/// variant's name is no agent's, and a rig maps an agent of the formulas to a variant the project sees.
pub(super) fn check_project(loader: &mut Loader, project: &mut Project, outer: &BTreeMap<String, Variant>) {
    let file = project.dir.join("aae.toml");
    for (name, variant) in &project.variants {
        clash(loader, &file, name, &project.packs);
        if let Err(e) = base(project, variant) {
            let variant_named = |n: &str| project.variants.contains_key(n) || outer.contains_key(n);
            let why = chained(&variant.base, variant_named).unwrap_or(e);
            loader.fail(&file, format!("`agents.{name}`: `base`: {why}"));
        }
    }
    let own = project.packs.first().map(|p| p.name.clone()).unwrap_or_default();
    let has_agent = |p: &Pack, n: &str| p.agents.contains_key(n);
    let packs = &project.packs;
    let of_root = |a: &Name| {
        let agent = packs
            .iter()
            .find(|p| p.name == a.pack)
            .and_then(|p| p.agents.get(&a.name));
        agent.is_some_and(|a| a.scope == Scope::Root)
    };
    let root = "works in the root (`scope = \"root\"`), which no rig edits: edit it in the project's `[agents]`";
    // A variant has the scope of its base; the project's variant shadows the user's of the same name.
    let outer = outer.iter().filter(|(n, _)| !project.variants.contains_key(*n));
    let variants: BTreeMap<&String, bool> = (project.variants.iter().chain(outer))
        .map(|(n, v)| (n, base(project, v).is_ok_and(|a| of_root(&a))))
        .collect();
    for (rig, r) in &mut project.rigs {
        for agent in r.edits.keys() {
            let in_root = (variants.get(agent).copied()).unwrap_or_else(|| {
                resolve(packs, &own, &reference(agent), has_agent, "agent").is_ok_and(|a| of_root(&a))
            });
            if in_root {
                loader.fail(&file, format!("rig `{rig}`: agent `{agent}` {root}"));
            }
        }
        let mut resolved = BTreeMap::new();
        for (agent, variant) in std::mem::take(&mut r.agents) {
            if !variants.contains_key(&variant) {
                loader.fail(&file, format!("rig `{rig}`: `agents`: no agent variant `{variant}`"));
            }
            match resolve(packs, &own, &agent, has_agent, "agent") {
                Ok(found) if of_root(&found) => loader.fail(&file, format!("rig `{rig}`: agent `{found}` {root}")),
                Ok(found) => drop(resolved.insert(found, variant)),
                Err(e) => loader.fail(&file, format!("rig `{rig}`: `agents`: {e}")),
            }
        }
        r.agents = resolved;
    }
}

/// The pack agent `variant` stands for in project `p`.
pub(super) fn base(p: &Project, variant: &Variant) -> Result<Name, String> {
    let own = p.packs.first().map(|pack| pack.name.as_str()).unwrap_or_default();
    resolve(
        &p.packs,
        own,
        &variant.base,
        |pack, n| pack.agents.contains_key(n),
        "agent",
    )
}

/// A variant is one level above a pack agent: why `base` is wrong when it names a variant.
fn chained(base: &Name, variant: impl Fn(&str) -> bool) -> Option<String> {
    (base.pack.is_empty() && variant(&base.name)).then(|| {
        format!(
            "`{}` is a variant: a variant is of a pack agent, not of another variant",
            base.name
        )
    })
}

/// A variant takes no name of a pack agent: a bare reference to it would be ambiguous.
fn clash(loader: &mut Loader, file: &Path, name: &str, packs: &[Pack]) {
    if let Some(pack) = packs.iter().find(|p| p.agents.contains_key(name)) {
        loader.fail(
            file,
            format!("`agents.{name}`: the name is that of agent `{}.{name}`", pack.name),
        );
    }
}
