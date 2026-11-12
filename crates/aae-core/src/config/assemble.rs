//! The effective config of one (project, rig, agent): the layers of spec §6.3 merged, every value with the layer it
//! came from, so that "explain config" can show it.

use super::{Config, Hook, Mcp, Name, PluginUse, Settings, Skill, check::resolve};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, fmt::Write as _};

/// The layer a value came from, weakest first. The host layer is not here: the harness merges it itself and the
/// core never sees its content (spec §6.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "layer", content = "name", rename_all = "snake_case")]
pub enum Layer {
    /// `[harness.<name>]` of the orchestrator: the default model of the harness.
    Harness,
    Orchestrator,
    Project,
    Rig,
    /// The contribution of an attached plugin.
    Plugin(String),
    Agent,
    OrchestratorOverride,
    ProjectOverride,
    RigOverride,
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Harness => f.write_str("orchestrator, harness default"),
            Self::Orchestrator => f.write_str("orchestrator"),
            Self::Project => f.write_str("project"),
            Self::Rig => f.write_str("rig"),
            Self::Plugin(name) => write!(f, "plugin {name}"),
            Self::Agent => f.write_str("agent"),
            Self::OrchestratorOverride => f.write_str("orchestrator override"),
            Self::ProjectOverride => f.write_str("project override"),
            Self::RigOverride => f.write_str("rig override"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sourced<T> {
    pub value: T,
    pub from: Layer,
}

/// The merged settings a session starts with (contracts 4.2); serializable, so a run keeps it as its snapshot.
/// `env` is the whole session environment besides what the adapter adds: nothing is inherited from the daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectiveConfig {
    pub harness: String,
    pub model: Option<Sourced<String>>,
    pub effort: Option<Sourced<String>>,
    pub system_prompt: Option<Sourced<String>>,
    pub permission_mode: Option<Sourced<String>>,
    /// `None`: no layer says, and host inheritance is on by default (spec §6.4).
    pub inherit_host: Option<Sourced<bool>>,
    pub env: BTreeMap<String, Sourced<String>>,
    pub mcp: BTreeMap<String, Sourced<Mcp>>,
    pub hooks: BTreeMap<String, Sourced<Hook>>,
    pub skills: BTreeMap<String, Sourced<Skill>>,
    /// Attached plugins in attachment order.
    pub plugins: Vec<Sourced<PluginUse>>,
    /// The native block of this harness.
    pub native: Option<Sourced<Value>>,
}

impl EffectiveConfig {
    pub fn inherits_host(&self) -> bool {
        self.inherit_host.as_ref().is_none_or(|s| s.value)
    }

    fn apply(&mut self, settings: &Settings, from: &Layer) {
        let sourced = |value: &String| Sourced {
            value: value.clone(),
            from: from.clone(),
        };
        for (slot, value) in [
            (&mut self.model, &settings.model),
            (&mut self.effort, &settings.effort),
            (&mut self.system_prompt, &settings.system_prompt),
            (&mut self.permission_mode, &settings.permission_mode),
        ] {
            if let Some(value) = value {
                *slot = Some(sourced(value));
            }
        }
        if let Some(value) = settings.inherit_host {
            self.inherit_host = Some(Sourced {
                value,
                from: from.clone(),
            });
        }
        if let Some(value) = settings.native.get(&self.harness) {
            self.native = Some(Sourced {
                value: value.clone(),
                from: from.clone(),
            });
        }
        by_key(&mut self.env, &settings.env, from, |_| true);
        by_key(&mut self.mcp, &settings.mcp, from, |_| true);
        by_key(&mut self.hooks, &settings.hooks, from, |h| h.enabled);
        by_key(&mut self.skills, &settings.skills, from, |s| s.enabled);
    }

    /// One line per value with its layer, and one line for the host layer (spec §6.3).
    pub fn explain(&self) -> String {
        let mut out = String::new();
        let mut line = |key: &str, value: String, from: &Layer| {
            let _ = writeln!(out, "{key} = {value}  # {from}");
        };
        line("harness", quoted(&self.harness), &Layer::Agent);
        for (key, slot) in [
            ("model", &self.model),
            ("effort", &self.effort),
            ("system_prompt", &self.system_prompt),
            ("permission_mode", &self.permission_mode),
        ] {
            if let Some(s) = slot {
                line(key, quoted(&s.value), &s.from);
            }
        }
        if let Some(s) = &self.inherit_host {
            line("inherit_host", s.value.to_string(), &s.from);
        }
        for (name, s) in &self.env {
            line(&format!("env.{name}"), quoted(&s.value), &s.from);
        }
        for (name, s) in &self.mcp {
            line(&format!("mcp.{name}"), json(&s.value), &s.from);
        }
        for (name, s) in &self.hooks {
            line(&format!("hooks.{name}"), json(&s.value), &s.from);
        }
        for (name, s) in &self.skills {
            line(&format!("skills.{name}"), json(&s.value), &s.from);
        }
        for s in &self.plugins {
            line(&format!("plugins.{}", s.value.name), json(&s.value.config), &s.from);
        }
        if let Some(s) = &self.native {
            line(&format!("native.{}", self.harness), s.value.to_string(), &s.from);
        }
        out.push_str(match self.inherits_host() {
            true => "host: inherited by the harness, its content is not visible to aae\n",
            false => "host: off\n",
        });
        out
    }
}

/// Dictionaries replace an entry whole; named lists also drop an entry that a stronger layer switches off.
fn by_key<T: Clone>(
    into: &mut BTreeMap<String, Sourced<T>>,
    from: &BTreeMap<String, T>,
    layer: &Layer,
    enabled: impl Fn(&T) -> bool,
) {
    for (key, value) in from {
        match enabled(value) {
            true => into.insert(
                key.clone(),
                Sourced {
                    value: value.clone(),
                    from: layer.clone(),
                },
            ),
            false => into.remove(key),
        };
    }
}

fn quoted(text: &str) -> String {
    format!("{text:?}")
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

impl Config {
    /// The effective config of `agent` for a run in `project`, on `rig` when the run has one (spec §6.3, §6.5).
    pub fn effective(&self, project: &str, rig: Option<&str>, agent: &Name) -> Result<EffectiveConfig, String> {
        let p = self
            .projects
            .iter()
            .find(|p| p.name == project)
            .ok_or(format!("no project `{project}`"))?;
        let r = rig
            .map(|r| p.rigs.get(r).ok_or(format!("no rig `{r}` in `{project}`")))
            .transpose()?;
        let a = p
            .packs
            .iter()
            .find(|pack| pack.name == agent.pack)
            .and_then(|pack| pack.agents.get(&agent.name))
            .ok_or(format!("no agent `{agent}` in `{project}`"))?;
        // An override names its agent as written in the project; one that names an agent of another project or
        // nobody here does not apply.
        let own = p.packs.first().map(|pack| pack.name.as_str()).unwrap_or_default();
        let has_agent = |pack: &super::Pack, n: &str| pack.agents.contains_key(n);
        let applies = |key: &String| {
            resolve(&p.packs, own, &super::load::reference(key), has_agent, "agent").as_ref() == Ok(agent)
        };
        let overrides = |map: &BTreeMap<String, Settings>| {
            map.iter()
                .filter(|(k, _)| applies(k))
                .map(|(_, s)| s.clone())
                .collect::<Vec<_>>()
        };
        let mut layers: Vec<(Layer, Settings)> = vec![
            (Layer::Orchestrator, self.settings.clone()),
            (Layer::Project, p.settings.clone()),
        ];
        layers.extend(r.map(|r| (Layer::Rig, r.settings.clone())));
        let mut stronger = vec![(Layer::Agent, a.settings.clone())];
        stronger.extend(
            overrides(&self.overrides)
                .into_iter()
                .map(|s| (Layer::OrchestratorOverride, s)),
        );
        stronger.extend(overrides(&p.overrides).into_iter().map(|s| (Layer::ProjectOverride, s)));
        stronger.extend(
            r.into_iter()
                .flat_map(|r| overrides(&r.overrides))
                .map(|s| (Layer::RigOverride, s)),
        );

        // Plugins attach first: their contributions are layer 5, between the rig and the agent.
        let mut plugins: Vec<Sourced<PluginUse>> = Vec::new();
        for (layer, settings) in layers.iter().chain(&stronger) {
            for used in &settings.plugins {
                let at = plugins.iter().position(|s| s.value.name == used.name);
                match (at, used.enabled) {
                    (Some(i), true) => {
                        plugins[i] = Sourced {
                            value: used.clone(),
                            from: layer.clone(),
                        }
                    }
                    (Some(i), false) => drop(plugins.remove(i)),
                    (None, true) => plugins.push(Sourced {
                        value: used.clone(),
                        from: layer.clone(),
                    }),
                    (None, false) => {}
                }
            }
        }
        for used in &plugins {
            let plugin = self
                .plugins
                .iter()
                .chain(&p.plugins)
                .find(|d| d.name == used.value.name);
            layers.extend(plugin.map(|d| (Layer::Plugin(d.name.clone()), d.agent.clone())));
        }

        let mut config = EffectiveConfig {
            harness: a.harness.clone(),
            model: self
                .harnesses
                .get(&a.harness)
                .and_then(|h| h.model.clone())
                .map(|value| Sourced {
                    value,
                    from: Layer::Harness,
                }),
            effort: None,
            system_prompt: None,
            permission_mode: None,
            inherit_host: None,
            // The project's tracker database, always by an explicit path (spec §10.2): br's own search would climb
            // to another tracker. A layer may still set it.
            env: Some(p.root.join(".beads"))
                .filter(|dir| dir.is_dir())
                .map(|dir| {
                    let value = dir.to_string_lossy().into_owned();
                    (
                        "BEADS_DIR".to_owned(),
                        Sourced {
                            value,
                            from: Layer::Project,
                        },
                    )
                })
                .into_iter()
                .collect(),
            mcp: BTreeMap::new(),
            hooks: BTreeMap::new(),
            skills: BTreeMap::new(),
            plugins,
            native: None,
        };
        for (layer, settings) in layers.iter().chain(&stronger) {
            config.apply(settings, layer);
        }
        Ok(config)
    }
}
