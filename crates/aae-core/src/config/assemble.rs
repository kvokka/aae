//! The effective config of one (project, rig, agent): the layers of spec §6.3 merged, every value with the layer it
//! came from, so that "explain config" can show it.

use super::{Hook, Mcp, PluginUse, Service, Settings, Skill};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, fmt::Write as _, path::PathBuf};

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
    /// `[settings]` of the pack of the run's formula.
    Pack(String),
    /// The `[[step]]` of the run's formula.
    Formula(String),
    /// The agent variant of the run (`[agents.<name>]`).
    Variant(String),
    /// `[agents.<name>]` of the orchestrator, the project and the rig: the user's settings for the agent.
    OrchestratorAgent,
    ProjectAgent,
    RigAgent,
    /// A value several layers make together, in the order they apply: tables merged by key, a prompt with fragments.
    Merged(Vec<Layer>),
}

impl Layer {
    /// The layer of a value `self` gave and `next` added to.
    fn and(&self, next: &Layer) -> Layer {
        let mut layers = match self {
            Self::Merged(layers) => layers.clone(),
            one => vec![one.clone()],
        };
        if !layers.contains(next) {
            layers.push(next.clone());
        }
        match <[Layer; 1]>::try_from(layers) {
            Ok([one]) => one,
            Err(layers) => Self::Merged(layers),
        }
    }
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
            Self::Pack(name) => write!(f, "pack {name}"),
            Self::Formula(name) => write!(f, "formula {name}"),
            Self::Variant(name) => write!(f, "variant {name}"),
            Self::OrchestratorAgent => f.write_str("orchestrator [agents]"),
            Self::ProjectAgent => f.write_str("project [agents]"),
            Self::RigAgent => f.write_str("rig [agents]"),
            Self::Merged(layers) => {
                let names: Vec<String> = layers.iter().map(ToString::to_string).collect();
                f.write_str(&names.join(" + "))
            }
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
    /// The layer that names the harness. A snapshot taken before the field has the agent's.
    #[serde(default = "agent")]
    pub harness_from: Layer,
    /// The full model name the harness gets.
    pub model: Option<Sourced<String>>,
    /// The short name a layer gave for `model`, when the adapter expanded it.
    #[serde(default)]
    pub model_given: Option<String>,
    pub effort: Option<Sourced<String>>,
    pub system_prompt: Option<Sourced<String>>,
    /// `None`: no layer says, and host inheritance is on by default (spec §6.4).
    pub inherit_host: Option<Sourced<bool>>,
    pub env: BTreeMap<String, Sourced<String>>,
    pub mcp: BTreeMap<String, Sourced<Mcp>>,
    pub hooks: BTreeMap<String, Sourced<Hook>>,
    pub skills: BTreeMap<String, Sourced<Skill>>,
    /// Attached plugins in attachment order.
    pub plugins: Vec<Sourced<PluginUse>>,
    /// The native block of this harness: tables of several layers merged by key.
    pub native: Option<Sourced<Value>>,
    /// The services its session uses; the core binds them to MCP servers or environment before the adapter.
    #[serde(default)]
    pub services: BTreeMap<String, Sourced<Service>>,
}

fn agent() -> Layer {
    Layer::Agent
}

impl EffectiveConfig {
    pub(super) fn new(harness: String, harness_from: Layer, model: Option<Sourced<String>>) -> Self {
        Self {
            harness,
            harness_from,
            model,
            model_given: None,
            effort: None,
            system_prompt: None,
            inherit_host: None,
            env: BTreeMap::new(),
            mcp: BTreeMap::new(),
            hooks: BTreeMap::new(),
            skills: BTreeMap::new(),
            plugins: Vec::new(),
            native: None,
            services: BTreeMap::new(),
        }
    }

    /// The model gate key of its sessions, `model:<harness>/<model>` (contracts 7), when a model is set.
    pub fn model_key(&self) -> Option<String> {
        (self.model.as_ref()).map(|model| format!("model:{}/{}", self.harness, model.value))
    }

    pub fn inherits_host(&self) -> bool {
        self.inherit_host.as_ref().is_none_or(|s| s.value)
    }

    pub(super) fn apply(&mut self, settings: &Settings, from: &Layer) {
        let sourced = |value: &String| Sourced {
            value: value.clone(),
            from: from.clone(),
        };
        for (slot, value) in [
            (&mut self.model, &settings.model),
            (&mut self.effort, &settings.effort),
            (&mut self.system_prompt, &settings.system_prompt),
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
            self.native = Some(match (self.native.take(), value.as_object()) {
                // Two tables merge by key; anything else replaces.
                (
                    Some(Sourced {
                        value: Value::Object(mut table),
                        from: old,
                    }),
                    Some(new),
                ) => {
                    table.extend(new.iter().map(|(k, v)| (k.clone(), v.clone())));
                    Sourced {
                        value: Value::Object(table),
                        from: old.and(from),
                    }
                }
                _ => Sourced {
                    value: value.clone(),
                    from: from.clone(),
                },
            });
        }
        by_key(&mut self.env, &settings.env, from, |_| true);
        by_key(&mut self.mcp, &settings.mcp, from, |_| true);
        by_key(&mut self.hooks, &settings.hooks, from, |h| h.enabled);
        by_key(&mut self.skills, &settings.skills, from, |s| s.enabled);
        by_key(&mut self.services, &settings.services, from, |_| true);
    }

    /// What attached plugins add beyond a settings layer (spec §11.2): their prompt fragments after the system
    /// prompt and their `bin/` directories ahead of `PATH` (the daemon's when no layer sets it), in attachment order.
    pub(super) fn contribute(&mut self, attached: &[&super::Plugin]) {
        for plugin in attached {
            let Some(fragment) = &plugin.prompt else { continue };
            let prompt = self.system_prompt.get_or_insert_with(|| Sourced {
                value: String::new(),
                from: Layer::Plugin(plugin.name.clone()),
            });
            let gap = if prompt.value.is_empty() { "" } else { "\n\n" };
            prompt.value = format!("{}{gap}{}", prompt.value, fragment.trim_end());
            prompt.from = prompt.from.and(&Layer::Plugin(plugin.name.clone()));
        }
        let bins: Vec<(&String, &PathBuf)> = (attached.iter())
            .filter_map(|p| Some((&p.name, p.bin.as_ref()?)))
            .collect();
        let Some(((first, _), rest)) = bins.split_first() else {
            return;
        };
        let (path, from) = match self.env.get("PATH") {
            Some(s) => (s.value.clone(), s.from.and(&Layer::Plugin((*first).clone()))),
            None => (
                std::env::var("PATH").unwrap_or_default(),
                Layer::Plugin((*first).clone()),
            ),
        };
        let from = (rest.iter()).fold(from, |from, (name, _)| from.and(&Layer::Plugin((*name).clone())));
        let dirs = bins
            .iter()
            .map(|(_, bin)| (*bin).clone())
            .chain(std::env::split_paths(&path));
        let value = std::env::join_paths(dirs).map_or(path, |p| p.to_string_lossy().into_owned());
        self.env.insert("PATH".to_owned(), Sourced { value, from });
    }

    /// One line per value with its layer, and one line for the host layer (spec §6.3).
    pub fn explain(&self) -> String {
        let mut out = String::new();
        let given = (self.model_given.as_ref()).map_or_else(String::new, |g| format!(", given as {}", quoted(g)));
        if let Some(s) = &self.model {
            let _ = writeln!(out, "model = {}  # {}{given}", quoted(&s.value), s.from);
        }
        let mut line = |key: &str, value: String, from: &Layer| {
            let _ = writeln!(out, "{key} = {value}  # {from}");
        };
        line("harness", quoted(&self.harness), &self.harness_from);
        for (key, slot) in [("effort", &self.effort), ("system_prompt", &self.system_prompt)] {
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
        for (name, s) in &self.services {
            line(&format!("service.{name}"), json(&s.value), &s.from);
        }
        for (name, s) in &self.hooks {
            line(&format!("hooks.{name}"), json(&s.value), &s.from);
        }
        for (name, s) in &self.skills {
            line(&format!("skills.{name}"), json(&s.value), &s.from);
        }
        for s in &self.plugins {
            line(&format!("plugins.{}", s.value.name), "attached".to_owned(), &s.from);
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
