//! The layers of one agent put together (docs/design/config.md "Layers"): the agent or its variant, the
//! pack and step of the formula of a run, the edits; the harness they come to, and the model name the adapter
//! expands.

use super::{
    Agent, Attached, Config, EffectiveConfig, Layer, Name, Pack, Project, Rig, Scope, Settings, Sourced, StepKind,
    check::resolve,
    load::{ADDON_DATA, Loader, PARAMS, reference},
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// The three launch settings: all that a pack's `[settings]` may set, and what an ai step sets besides `native`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Launch {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl From<Launch> for Settings {
    fn from(l: Launch) -> Self {
        Settings {
            harness: l.harness,
            model: l.model,
            effort: l.effort,
            ..Settings::default()
        }
    }
}

impl Config {
    /// The effective config of `agent` alone, with no formula, in `project`, on `rig` when given
    /// (docs/design/config.md).
    pub fn effective(&self, project: &str, rig: Option<&str>, agent: &Name) -> Result<EffectiveConfig, String> {
        let (p, r) = self.place(project, rig)?;
        self.named(p, r, &agent.to_string(), Vec::new())
    }

    /// The config of `name` in `p` at `r` — a variant's by its name, else the pack agent's `<pack>.<name>` — with
    /// `middle` between the agent and the edits, and its `subagents` resolved.
    pub(super) fn named(
        &self,
        p: &Project,
        r: Option<&Rig>,
        name: &str,
        mut middle: Vec<(Layer, Settings)>,
    ) -> Result<EffectiveConfig, String> {
        let mut config = match p.variants.get(name).or_else(|| self.variants.get(name)) {
            Some(v) => {
                let agent = super::variants::base(p, v)?;
                middle.push((Layer::Variant(name.to_owned()), v.settings.clone()));
                // The edits of a variant are those that name it: the base agent's are for the base agent.
                let config = self.assemble(p, r, agent_of(p, &agent)?, middle, &|key| key == name)?;
                (agent, config)
            }
            None => {
                let agent = reference(name);
                let own = p.packs.first().map(|pack| pack.name.as_str()).unwrap_or_default();
                // An edit names its agent as written in the project; one that names an agent of another project or
                // nobody here does not apply.
                let has_agent = |pack: &Pack, n: &str| pack.agents.contains_key(n);
                let named =
                    |key: &String| resolve(&p.packs, own, &reference(key), has_agent, "agent").as_ref() == Ok(&agent);
                let config = self.assemble(p, r, agent_of(p, &agent)?, middle, &named)?;
                (agent, config)
            }
        };
        if let Some(list) = &mut config.1.subagents {
            list.value = super::subagents::resolve(self, p, r, &config.0.pack, name, &list.value)?;
        }
        Ok(config.1)
    }

    /// The agent step `step` of a run of `formula` on `rig` is of — the rig's variant of the step's agent, else that
    /// agent — and its effective config, with the layers of the formula's pack and step and of the variant.
    pub fn for_run(
        &self,
        project: &str,
        rig: &str,
        formula: &str,
        step: &str,
    ) -> Result<(String, EffectiveConfig), String> {
        self.for_run_as(project, rig, formula, step, None)
    }

    /// [`Self::for_run`] with the agent `as_agent` (a variant's name, else an agent's) in place of the step's: the
    /// one the `agent:` label of a run's bead names (docs/design/config.md).
    pub fn for_run_as(
        &self,
        project: &str,
        rig: &str,
        formula: &str,
        step: &str,
        as_agent: Option<&str>,
    ) -> Result<(String, EffectiveConfig), String> {
        let (p, r) = self.place(project, Some(rig))?;
        let (name, f) = p.formula(formula)?;
        let kind = f.step(step).map(|s| &s.kind);
        let Some(StepKind::Ai { agent, settings, .. }) = kind else {
            return Err(format!("step `{step}` of `{name}` is not an ai step: it has no agent"));
        };
        let pack = p.packs.iter().find(|pack| pack.name == name.pack);
        let mut middle: Vec<(Layer, Settings)> = (pack.into_iter())
            .map(|pack| (Layer::Pack(pack.name.clone()), pack.settings.clone()))
            .collect();
        middle.push((Layer::Formula(format!("{name} step {step}")), (**settings).clone()));
        let own = p.packs.first().map(|pack| pack.name.as_str()).unwrap_or_default();
        let has_agent = |pack: &Pack, n: &str| pack.agents.contains_key(n);
        let is_variant = |n: &str| p.variants.contains_key(n) || self.variants.contains_key(n);
        // A rig maps an agent of its own to a variant; it maps none of the root (the config refuses that).
        let mapped = |a: &Name| r.and_then(|r| r.agents.get(a)).cloned();
        let (agent, variant) = match as_agent {
            Some(n) if is_variant(n) => (agent.clone(), Some(n.to_owned())),
            Some(n) => {
                let agent = resolve(&p.packs, own, &reference(n), has_agent, "agent")?;
                let variant = mapped(&agent);
                (agent, variant)
            }
            None => (agent.clone(), mapped(agent)),
        };
        let name = variant.unwrap_or_else(|| agent.to_string());
        let mut config = self.named(p, r, &name, middle)?;
        config.targets = super::subagents::targets(self, p, r, &name, &config);
        Ok((name, config))
    }

    /// The addons whose orchestrator hooks an event of `project` gets, by name, with the values of their params in
    /// force (docs/design/contracts.md 3.1): those the agent of `snapshot`, a run's config snapshot, attaches; with no
    /// agent, those `[settings]` of the orchestrator and of `project` attach.
    pub fn hook_addons(&self, project: Option<&str>, snapshot: Option<&str>) -> BTreeMap<String, serde_json::Value> {
        let agent = (snapshot.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()))
            .filter(|c| c.get("harness").is_some());
        let addons: Vec<Sourced<Attached>> = match agent {
            Some(mut c) => serde_json::from_value(c["addons"].take()).unwrap_or_default(),
            None => self.settings_addons(project),
        };
        let values = |p: Attached| {
            let params = p.params.into_iter().map(|(k, s)| (k, s.value));
            (p.name, serde_json::Value::Object(params.collect()))
        };
        addons.into_iter().map(|p| values(p.value)).collect()
    }

    /// The addons `[settings]` of the orchestrator and of `project` attach, with their params.
    fn settings_addons(&self, project: Option<&str>) -> Vec<Sourced<Attached>> {
        let p = self.projects.iter().find(|p| Some(p.name.as_str()) == project);
        let declared: Vec<&super::Addon> = (self.addons.iter())
            .chain(p.into_iter().flat_map(|p| &p.addons))
            .collect();
        let mut layers = vec![(Layer::Orchestrator, self.settings.clone())];
        layers.extend(p.map(|p| (Layer::Project, p.settings.clone())));
        attach(&declared, layers.iter()).into_iter().map(|(_, s)| s).collect()
    }

    /// The prompt of `agent` in `project`: a pack agent's qualified name, or a variant's, whose role and so prompt
    /// are its base agent's (docs/design/config.md).
    pub fn prompt(&self, project: &str, agent: &str) -> Result<&super::Prompt, String> {
        let (p, _) = self.place(project, None)?;
        let name = match p.variants.get(agent).or_else(|| self.variants.get(agent)) {
            Some(v) => super::variants::base(p, v)?,
            None => reference(agent),
        };
        p.prompts
            .get(&name)
            .ok_or_else(|| format!("agent `{agent}` has no prompt template"))
    }

    /// Runs of `agent` (a pack agent's qualified name or a variant's) started at once in `project`: a variant's own
    /// limit, else its base agent's.
    pub fn max_runs(&self, project: &str, agent: &str) -> Option<u32> {
        let p = self.projects.iter().find(|p| p.name == project)?;
        let of = |name: &Name| agent_of(p, name).ok().and_then(|a| a.max_runs);
        match p.variants.get(agent).or_else(|| self.variants.get(agent)) {
            Some(v) => v.max_runs.or_else(|| of(&super::variants::base(p, v).ok()?)),
            None => of(&reference(agent)),
        }
    }

    fn place(&self, project: &str, rig: Option<&str>) -> Result<(&Project, Option<&Rig>), String> {
        let p = (self.projects.iter().find(|p| p.name == project)).ok_or(format!("no project `{project}`"))?;
        let r = rig
            .map(|r| p.place(r).ok_or(format!("no rig `{r}` in `{project}`")))
            .transpose()?;
        Ok((p, r))
    }

    /// The layers of docs/design/config.md merged, weakest first; `middle` goes between the agent and the edits, of
    /// which those whose key `named` accepts apply.
    fn assemble(
        &self,
        p: &Project,
        r: Option<&Rig>,
        a: &Agent,
        middle: Vec<(Layer, Settings)>,
        named: &dyn Fn(&String) -> bool,
    ) -> Result<EffectiveConfig, String> {
        let r = r.filter(|_| a.scope == Scope::Rig);
        let edits = |map: &BTreeMap<String, Settings>| {
            (map.iter().filter(|(k, _)| named(k)).map(|(_, s)| s.clone())).collect::<Vec<_>>()
        };
        let mut layers: Vec<(Layer, Settings)> = vec![
            (Layer::Orchestrator, self.settings.clone()),
            (Layer::Project, p.settings.clone()),
        ];
        layers.extend(r.map(|r| (Layer::Rig, r.settings.clone())));
        let mut agent = a.settings.clone();
        agent.harness.clone_from(&a.harness);
        let mut stronger = vec![(Layer::Agent, agent)];
        stronger.extend(middle);
        stronger.extend(edits(&self.edits).into_iter().map(|s| (Layer::OrchestratorAgent, s)));
        stronger.extend(edits(&p.edits).into_iter().map(|s| (Layer::ProjectAgent, s)));
        stronger.extend((r.into_iter().flat_map(|r| edits(&r.edits))).map(|s| (Layer::RigAgent, s)));

        // Addons attach first: their contributions are a layer between the rig and the agent.
        let declared: Vec<&super::Addon> = self.addons.iter().chain(&p.addons).collect();
        let attached = attach(&declared, layers.iter().chain(&stronger));
        for (d, used) in &attached {
            let mut agent = d.agent.clone();
            for (key, value) in &used.value.params {
                let (pattern, text) = (format!("{PARAMS}{key}}}"), value.value.to_string());
                agent.substitute_whole(&pattern, &value.value);
                agent.substitute(&pattern, value.value.as_str().unwrap_or(&text));
            }
            if let Some(data) = &self.addon_data {
                agent.substitute(ADDON_DATA, &data.join(&d.name).to_string_lossy());
            }
            layers.push((Layer::Addon(d.name.clone()), agent));
        }
        layers.extend(stronger);

        let (harness, harness_from) = Self::launch(&layers)?;
        let other = |d: &super::Addon| d.harnesses.as_ref().is_some_and(|h| !h.contains(&harness));
        if let Some((d, _)) = attached.iter().find(|(d, _)| other(d)) {
            let list = d.harnesses.as_deref().unwrap_or_default().join(", ");
            return Err(format!("addon `{}` is for {list}, not for harness `{harness}`", d.name));
        }
        let model = (self.harnesses.get(&harness).and_then(|h| h.model.clone())).map(|value| Sourced {
            value,
            from: Layer::Harness,
        });
        let mut config = EffectiveConfig::new(harness, harness_from, model);
        // The project's tracker, by name, even where the project has none (docs/design/config.md). A layer may
        // still set it.
        for (name, value) in p.tracker_env(r) {
            let value = Sourced {
                value,
                from: Layer::Project,
            };
            config.env.insert(name.clone(), value);
        }
        config.addons = attached.iter().map(|(_, used)| used.clone()).collect();
        for (layer, settings) in &layers {
            config.apply(settings, layer);
        }
        config.contribute(&attached.iter().map(|(d, _)| *d).collect::<Vec<_>>());
        for (name, service) in &config.services {
            service.value.bind_address(name)?;
        }
        config.scope = a.scope;
        let adapter = self.adapters.iter().find(|a| a.name == config.harness);
        if let (Some(model), Some(adapter)) = (&mut config.model, adapter)
            && let Some((_, full)) = adapter.models.iter().find(|(short, _)| *short == model.value)
        {
            config.model_given = Some(std::mem::replace(&mut model.value, (*full).to_owned()));
        }
        Ok(config)
    }

    /// The harness and the layer that names it: the strongest layer that says (docs/design/config.md). Each layer's
    /// harness is one of the orchestrator's: loading checks it.
    fn launch(layers: &[(Layer, Settings)]) -> Result<(String, Layer), String> {
        (layers.iter().rev())
            .find_map(|(from, s)| s.harness.clone().map(|h| (h, from.clone())))
            .ok_or_else(|| "no layer sets `harness`".to_owned())
    }
}

/// The addons `layers` attach, weakest layer first, with the values of their params (docs/design/config.md "Addon"):
/// an attachment keeps the place of its first mention, and each mention gives params by key and says whether it is on.
fn attach<'a, 'b>(
    declared: &[&'a super::Addon],
    layers: impl Iterator<Item = &'b (Layer, Settings)>,
) -> Vec<(&'a super::Addon, Sourced<Attached>)> {
    let mut uses: Vec<(&super::Addon, Sourced<Attached>, bool)> = Vec::new();
    for (layer, settings) in layers {
        for used in &settings.addons {
            let i = match uses.iter().position(|(d, ..)| d.name == used.name) {
                Some(i) => i,
                // Loading refuses an attachment of no declared addon.
                None => match declared.iter().find(|d| d.name == used.name) {
                    None => continue,
                    Some(d) => {
                        let from = Layer::Addon(d.name.clone());
                        let params = (d.params.iter()).map(|(k, value)| (k.clone(), sourced(value.clone(), &from)));
                        let value = Attached {
                            name: d.name.clone(),
                            params: params.collect(),
                        };
                        uses.push((d, sourced(value, layer), false));
                        uses.len() - 1
                    }
                },
            };
            let (_, attached, on) = &mut uses[i];
            (attached.from, *on) = (layer.clone(), used.enabled);
            let params = (used.params.iter()).map(|(k, v)| (k.clone(), sourced(v.clone(), layer)));
            attached.value.params.extend(params);
        }
    }
    uses.into_iter().filter_map(|(d, s, on)| on.then_some((d, s))).collect()
}

fn agent_of<'a>(p: &'a Project, agent: &Name) -> Result<&'a Agent, String> {
    (p.packs.iter().find(|pack| pack.name == agent.pack))
        .and_then(|pack| pack.agents.get(&agent.name))
        .ok_or(format!("no agent `{agent}` in `{}`", p.name))
}

/// Every ai step of a project on every rig assembles: the harness of its runs is known. Said once the references
/// resolve, of the project file, with the layer of each value. The harnesses the steps run on are returned.
pub(super) fn check_runs(loader: &mut Loader, config: &Config) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    if !loader.errors.is_empty() {
        return used;
    }
    for p in &config.projects {
        let file = p.dir.join("loop-manager.toml");
        let formulas = (p.packs.iter()).flat_map(|pack| pack.formulas.iter().map(move |(f, x)| (pack, f, x)));
        for (pack, name, f) in formulas {
            let formula = format!("{}.{name}", pack.name);
            let steps = f.steps.iter().filter(|s| matches!(s.kind, StepKind::Ai { .. }));
            for (step, rig) in steps.flat_map(|s| p.places().map(move |(r, _)| (&s.name, r))) {
                match config.for_run(&p.name, rig, &formula, step) {
                    // The layers of the formula's pack and step may set another harness than its targets'.
                    Ok((agent, e)) => {
                        for why in super::subagents::unfit(config, p, p.place(rig), &agent, &e) {
                            loader.fail(&file, format!("formula `{formula}` on rig `{rig}`: {why}"));
                        }
                        used.insert(e.harness);
                    }
                    Err(e) => loader.fail(&file, format!("formula `{formula}` on rig `{rig}`: {e}")),
                }
            }
        }
    }
    used
}

fn sourced<T>(value: T, from: &Layer) -> Sourced<T> {
    Sourced {
        value,
        from: from.clone(),
    }
}
