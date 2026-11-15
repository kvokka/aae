//! The layers of one agent put together (spec §6.3, docs/design/config.md "Layers"): the agent or its variant, the
//! pack and step of the formula of a run, the overrides; the harness they come to, and the model name the adapter
//! expands.

use super::{
    Agent, Config, EffectiveConfig, Layer, Name, Pack, PluginUse, Project, Rig, Settings, Sourced, StepKind,
    check::resolve, load::Loader, load::reference,
};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The three launch settings, all that a pack's `[settings]` and a prompt step may set.
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
    /// The effective config of `agent` alone, with no formula, in `project`, on `rig` when given (spec §6.3, §6.5).
    pub fn effective(&self, project: &str, rig: Option<&str>, agent: &Name) -> Result<EffectiveConfig, String> {
        let (p, r) = self.place(project, rig)?;
        let a = agent_of(p, agent)?;
        let own = p.packs.first().map(|pack| pack.name.as_str()).unwrap_or_default();
        // An override names its agent as written in the project; one that names an agent of another project or
        // nobody here does not apply.
        let has_agent = |pack: &Pack, n: &str| pack.agents.contains_key(n);
        let named = |key: &String| resolve(&p.packs, own, &reference(key), has_agent, "agent").as_ref() == Ok(agent);
        self.assemble(p, r, a, Vec::new(), &named)
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
        let (p, r) = self.place(project, Some(rig))?;
        let (name, f) = p.formula(formula)?;
        let kind = f.step(step).map(|s| &s.kind);
        let Some(StepKind::Prompt { agent, settings, .. }) = kind else {
            return Err(format!(
                "step `{step}` of `{name}` is not a prompt step: it has no agent"
            ));
        };
        let pack = p.packs.iter().find(|pack| pack.name == name.pack);
        let mut middle: Vec<(Layer, Settings)> = (pack.into_iter())
            .map(|pack| (Layer::Pack(pack.name.clone()), pack.settings.clone()))
            .collect();
        middle.push((Layer::Formula(format!("{name} step {step}")), (**settings).clone()));
        let Some(variant) = r.and_then(|r| r.agents.get(agent)) else {
            let a = agent_of(p, agent)?;
            let own = p.packs.first().map(|pack| pack.name.as_str()).unwrap_or_default();
            let has_agent = |pack: &Pack, n: &str| pack.agents.contains_key(n);
            let named =
                |key: &String| resolve(&p.packs, own, &reference(key), has_agent, "agent").as_ref() == Ok(agent);
            return Ok((agent.to_string(), self.assemble(p, r, a, middle, &named)?));
        };
        let v = (p.variants.get(variant).or_else(|| self.variants.get(variant)))
            .ok_or_else(|| format!("no agent variant `{variant}`"))?;
        let a = agent_of(p, &super::variants::base(p, v)?)?;
        middle.push((Layer::Variant(variant.clone()), v.settings.clone()));
        // The overrides of a variant are those that name it: the base agent's are for the base agent.
        let config = self.assemble(p, r, a, middle, &|key| key == variant)?;
        Ok((variant.clone(), config))
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
            .map(|r| p.rigs.get(r).ok_or(format!("no rig `{r}` in `{project}`")))
            .transpose()?;
        Ok((p, r))
    }

    /// The layers of spec §6.3 merged, weakest first; `middle` goes between the agent and the overrides, of which
    /// those whose key `named` accepts apply.
    fn assemble(
        &self,
        p: &Project,
        r: Option<&Rig>,
        a: &Agent,
        middle: Vec<(Layer, Settings)>,
        named: &dyn Fn(&String) -> bool,
    ) -> Result<EffectiveConfig, String> {
        let overrides = |map: &BTreeMap<String, Settings>| {
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
        stronger.extend(
            overrides(&self.overrides)
                .into_iter()
                .map(|s| (Layer::OrchestratorOverride, s)),
        );
        stronger.extend(overrides(&p.overrides).into_iter().map(|s| (Layer::ProjectOverride, s)));
        stronger.extend((r.into_iter().flat_map(|r| overrides(&r.overrides))).map(|s| (Layer::RigOverride, s)));

        // Plugins attach first: their contributions are a layer between the rig and the agent.
        let mut plugins: Vec<Sourced<PluginUse>> = Vec::new();
        for (layer, settings) in layers.iter().chain(&stronger) {
            for used in &settings.plugins {
                let at = plugins.iter().position(|s| s.value.name == used.name);
                let sourced = || Sourced {
                    value: used.clone(),
                    from: layer.clone(),
                };
                match (at, used.enabled) {
                    (Some(i), true) => plugins[i] = sourced(),
                    (Some(i), false) => drop(plugins.remove(i)),
                    (None, true) => plugins.push(sourced()),
                    (None, false) => {}
                }
            }
        }
        let attached: Vec<&super::Plugin> = (plugins.iter())
            .filter_map(|used| (self.plugins.iter().chain(&p.plugins)).find(|d| d.name == used.value.name))
            .collect();
        layers.extend((attached.iter()).map(|d| (Layer::Plugin(d.name.clone()), d.agent.clone())));
        layers.extend(stronger);

        let (harness, harness_from) = Self::launch(&layers)?;
        let model = (self.harnesses.get(&harness).and_then(|h| h.model.clone())).map(|value| Sourced {
            value,
            from: Layer::Harness,
        });
        let mut config = EffectiveConfig::new(harness, harness_from, model);
        // The project's tracker, by name, even where the project has none (spec §10.2). A layer may still set it.
        for (name, value) in &p.tracker.env {
            let value = Sourced {
                value: value.clone(),
                from: Layer::Project,
            };
            config.env.insert(name.clone(), value);
        }
        config.plugins = plugins;
        for (layer, settings) in &layers {
            config.apply(settings, layer);
        }
        config.contribute(&attached);
        let adapter = self.adapters.iter().find(|a| a.name == config.harness);
        if let (Some(model), Some(adapter)) = (&mut config.model, adapter)
            && let Some((_, full)) = adapter.models.iter().find(|(short, _)| *short == model.value)
        {
            config.model_given = Some(std::mem::replace(&mut model.value, (*full).to_owned()));
        }
        Ok(config)
    }

    /// The harness and the layer that names it: the strongest layer that says (spec §6.2). Each layer's harness is
    /// one of the orchestrator's: loading checks it.
    fn launch(layers: &[(Layer, Settings)]) -> Result<(String, Layer), String> {
        (layers.iter().rev())
            .find_map(|(from, s)| s.harness.clone().map(|h| (h, from.clone())))
            .ok_or_else(|| "no layer sets `harness`".to_owned())
    }
}

fn agent_of<'a>(p: &'a Project, agent: &Name) -> Result<&'a Agent, String> {
    (p.packs.iter().find(|pack| pack.name == agent.pack))
        .and_then(|pack| pack.agents.get(&agent.name))
        .ok_or(format!("no agent `{agent}` in `{}`", p.name))
}

/// Every prompt formula of a project on every rig assembles: the harness of its runs is known. Said once the
/// references resolve, of the project file, with the layer of each value.
pub(super) fn check_runs(loader: &mut Loader, config: &Config) {
    if !loader.errors.is_empty() {
        return;
    }
    for p in &config.projects {
        let file = p.root.join("aae.toml");
        let formulas = (p.packs.iter()).flat_map(|pack| pack.formulas.iter().map(move |(f, x)| (pack, f, x)));
        for (pack, name, f) in formulas {
            let formula = format!("{}.{name}", pack.name);
            let prompts = f.steps.iter().filter(|s| matches!(s.kind, StepKind::Prompt { .. }));
            for (step, rig) in prompts.flat_map(|s| p.rigs.keys().map(move |r| (&s.name, r))) {
                if let Err(e) = config.for_run(&p.name, rig, &formula, step) {
                    loader.fail(&file, format!("formula `{formula}` on rig `{rig}`: {e}"));
                }
            }
        }
    }
}
