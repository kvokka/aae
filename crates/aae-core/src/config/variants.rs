//! `[agents.<name>]` of the orchestrator and project files — edits of agents and variants of pack agents — and the
//! rigs that run a variant in place of a pack agent (`[rigs.<r>.agents]`): docs/design/config.md "Agent variants".

use super::{
    Name, Pack, Project, Settings, Variant,
    check::resolve,
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
    let file = project.root.join("aae.toml");
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
    for (rig, r) in &mut project.rigs {
        let mut resolved = BTreeMap::new();
        for (agent, variant) in std::mem::take(&mut r.agents) {
            if !project.variants.contains_key(&variant) && !outer.contains_key(&variant) {
                loader.fail(&file, format!("rig `{rig}`: `agents`: no agent variant `{variant}`"));
            }
            match resolve(&project.packs, &own, &agent, has_agent, "agent") {
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
