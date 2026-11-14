//! The layers of one agent put together (spec §6.3, docs/design/config.md "Layers"): the agent or its variant, the
//! pack and step of the formula of a run, the overrides; the harness and provider they come to, and the model name
//! the adapter expands.

use super::{
    Agent, Config, EffectiveConfig, Layer, Name, Pack, PluginUse, Project, Rig, Settings, Sourced, Step,
    check::resolve, load::Loader, load::reference,
};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The four launch settings, all that a pack's `[settings]` and a prompt step may set.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Launch {
    pub harness: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl From<Launch> for Settings {
    fn from(l: Launch) -> Self {
        Settings {
            harness: l.harness,
            provider: l.provider,
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

    /// The agent a run of `formula` on `rig` is of — the rig's variant of the step's agent, else that agent — and its
    /// effective config, with the layers of the formula's pack and step and of the variant.
    pub fn for_run(&self, project: &str, rig: &str, formula: &str) -> Result<(String, EffectiveConfig), String> {
        let (p, r) = self.place(project, Some(rig))?;
        let (name, f) = p.formula(formula)?;
        let Step::Prompt { agent, settings, .. } = &f.step else {
            return Err(format!("`{name}` is an exec step: it has no agent"));
        };
        let pack = p.packs.iter().find(|pack| pack.name == name.pack);
        let mut middle: Vec<(Layer, Settings)> = (pack.into_iter())
            .map(|pack| (Layer::Pack(pack.name.clone()), pack.settings.clone()))
            .collect();
        middle.push((Layer::Formula(name.to_string()), (**settings).clone()));
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

        let (harness, harness_from, provider) = self.launch(&layers)?;
        let model = (self.harnesses.get(&harness).and_then(|h| h.model.clone())).map(|value| Sourced {
            value,
            from: Layer::Harness,
        });
        let mut config = EffectiveConfig::new(harness, harness_from, provider, model);
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

    /// The harness, the layer that names it, and the provider of `layers`: each as the strongest layer says; the
    /// one not said follows from the other through the adapters (docs/design/adapters.md).
    fn launch(&self, layers: &[(Layer, Settings)]) -> Result<(String, Layer, Option<Sourced<String>>), String> {
        let last = |key: fn(&Settings) -> Option<&String>| {
            (layers.iter().enumerate().rev()).find_map(|(at, (from, s))| {
                key(s).map(|value| {
                    let value = Sourced {
                        value: value.clone(),
                        from: from.clone(),
                    };
                    (at, value)
                })
            })
        };
        let adapter = |h: &str| self.adapters.iter().find(|a| a.name == h);
        let (harness, provider) = (last(|s| s.harness.as_ref()), last(|s| s.provider.as_ref()));
        // Of a harness and a provider that do not go together, the stronger layer's stands and the other follows.
        let (harness, provider) = match (harness, provider) {
            (Some((h_at, h)), Some((p_at, p)))
                if adapter(&h.value).is_some_and(|a| !a.providers.contains(&p.value.as_str())) =>
            {
                match h_at.cmp(&p_at) {
                    std::cmp::Ordering::Less => (None, Some(p)),
                    std::cmp::Ordering::Greater => (Some(h), None),
                    std::cmp::Ordering::Equal => {
                        let works = adapter(&h.value).map(|a| a.providers.join(", ")).unwrap_or_default();
                        return Err(format!(
                            "`provider = {:?}` ({}) is not one that harness `{}` works with: {works}",
                            p.value, p.from, h.value
                        ));
                    }
                }
            }
            (h, p) => (h.map(|(_, h)| h), p.map(|(_, p)| p)),
        };
        let (harness, from, provider) = match (harness, provider) {
            (Some(h), Some(p)) => (h.value, h.from, Some(p)),
            (None, None) => return Err("neither `harness` nor `provider` is set".to_owned()),
            (Some(h), None) => {
                let own = adapter(&h.value).and_then(|a| a.providers.first());
                let own = own.map(|p| Sourced {
                    value: (*p).to_owned(),
                    from: Layer::Derived,
                });
                (h.value, h.from, own)
            }
            (None, Some(p)) => {
                let serving = self.adapters.iter().filter(|a| a.providers.contains(&p.value.as_str()));
                match serving.map(|a| a.name).collect::<Vec<_>>().as_slice() {
                    [h] => ((*h).to_owned(), Layer::Derived, Some(p)),
                    [] => {
                        return Err(format!(
                            "`provider = {:?}` ({}): no harness works with it",
                            p.value, p.from
                        ));
                    }
                    many => {
                        let many = many.join(", ");
                        let why = format!("harnesses {many} all work with it; set `harness`");
                        return Err(format!("`provider = {:?}` ({}): {why}", p.value, p.from));
                    }
                }
            }
        };
        if !self.harnesses.contains_key(&harness) {
            return Err(format!(
                "harness `{harness}` ({from}) is not in the orchestrator config: add `[harness.{harness}]`"
            ));
        }
        Ok((harness, from, provider))
    }
}

fn agent_of<'a>(p: &'a Project, agent: &Name) -> Result<&'a Agent, String> {
    (p.packs.iter().find(|pack| pack.name == agent.pack))
        .and_then(|pack| pack.agents.get(&agent.name))
        .ok_or(format!("no agent `{agent}` in `{}`", p.name))
}

/// Every prompt formula of a project on every rig assembles: the harness and provider of its runs are known and go
/// together. Said once the references resolve, of the project file, with the layer of each value.
pub(super) fn check_runs(loader: &mut Loader, config: &Config) {
    if !loader.errors.is_empty() {
        return;
    }
    for p in &config.projects {
        let file = p.root.join("aae.toml");
        let formulas = (p.packs.iter()).flat_map(|pack| pack.formulas.iter().map(move |(f, x)| (pack, f, x)));
        for (pack, name, _) in formulas.filter(|(.., f)| matches!(f.step, Step::Prompt { .. })) {
            let formula = format!("{}.{name}", pack.name);
            for rig in p.rigs.keys() {
                if let Err(e) = config.for_run(&p.name, rig, &formula) {
                    loader.fail(&file, format!("formula `{formula}` on rig `{rig}`: {e}"));
                }
            }
        }
    }
}
