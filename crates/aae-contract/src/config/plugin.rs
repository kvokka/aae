//! A plugin (docs/design/config.md "Plugin"): its manifest as loaded, what its `[agent]` may give, and the strings of
//! its parts that `${AAE_PLUGIN_DIR}`, `${AAE_PLUGIN_DATA}` and `${params.<key>}` are filled in.

use super::{
    Hook, Mcp, OrchestratorHook, Plugin, Settings, Skill,
    files::{PluginDecl, PluginFile},
    load::{Loader, PARAMS, PLUGIN_DIR},
    schema::event_path,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// `[agent]` of a plugin: the settings a plugin may give the agents that attach it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginAgent {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub mcp: BTreeMap<String, Mcp>,
    #[serde(default)]
    pub hooks: BTreeMap<String, Hook>,
    #[serde(default)]
    pub skills: BTreeMap<String, Skill>,
    #[serde(default)]
    pub native: BTreeMap<String, Value>,
}

impl From<PluginAgent> for Settings {
    fn from(a: PluginAgent) -> Self {
        Settings {
            env: a.env,
            mcp: a.mcp,
            hooks: a.hooks,
            skills: a.skills,
            native: a.native,
            ..Settings::default()
        }
    }
}

impl Settings {
    /// Every string of this layer a plugin's text may reach (docs/design/config.md "Plugin"): harness hook commands,
    /// `env` values (`at` is told so: a value put there is a literal), MCP servers, the `native` tables and the
    /// services.
    pub(super) fn strings_mut(&mut self, at: &mut impl FnMut(&mut String, bool)) {
        (self.hooks.values_mut().filter_map(|h| h.command.as_mut())).for_each(|t| at(t, false));
        self.env.values_mut().for_each(|t| at(t, true));
        for mcp in self.mcp.values_mut() {
            let (command, url) = (mcp.command.iter_mut().flatten(), mcp.url.iter_mut());
            let texts = command
                .chain(url)
                .chain(mcp.env.values_mut())
                .chain(mcp.headers.values_mut());
            texts.for_each(|t| at(t, false));
        }
        self.native.values_mut().for_each(|v| strings(v, at));
        for s in self.services.values_mut() {
            let health = match &mut s.health {
                super::Health::Command(command) => Some(command),
                _ => None,
            };
            let texts = s
                .command
                .iter_mut()
                .chain(health.into_iter().flatten())
                .chain(s.env.values_mut());
            texts.for_each(|t| at(t, false));
        }
    }

    /// A string of a `native` table that is `pattern` and nothing else becomes `value` itself, with its type.
    pub(super) fn substitute_whole(&mut self, pattern: &str, value: &Value) {
        fn walk(at: &mut Value, pattern: &str, value: &Value) {
            match at {
                Value::String(text) if text == pattern => *at = value.clone(),
                Value::Array(list) => list.iter_mut().for_each(|v| walk(v, pattern, value)),
                Value::Object(table) => table.values_mut().for_each(|v| walk(v, pattern, value)),
                _ => {}
            }
        }
        self.native.values_mut().for_each(|v| walk(v, pattern, value));
    }

    /// `pattern` becomes `value` in every string of [`Self::strings_mut`]; in an `env` value as a literal, so a `$` in
    /// `value` is no reference.
    pub(super) fn substitute(&mut self, pattern: &str, value: &str) {
        let literal = super::env_literal(value);
        self.strings_mut(&mut |text, env| {
            *text = text.replace(pattern, if env { &literal } else { value });
        });
    }
}

fn strings(value: &mut Value, at: &mut impl FnMut(&mut String, bool)) {
    match value {
        Value::String(text) => at(text, false),
        Value::Array(list) => list.iter_mut().for_each(|v| strings(v, at)),
        Value::Object(table) => table.values_mut().for_each(|v| strings(v, at)),
        _ => {}
    }
}

impl Loader {
    /// Plugins declared by one file; `outer` are the plugins already declared above it.
    pub(super) fn plugins(
        &mut self,
        file: &Path,
        base: &Path,
        decls: Vec<PluginDecl>,
        outer: &[Plugin],
    ) -> Vec<Plugin> {
        let mut plugins: Vec<Plugin> = Vec::new();
        for decl in decls {
            let Some(root) = self.plugin_source(file, &decl).and_then(|s| self.root(file, base, &s)) else {
                continue;
            };
            let root = match &decl.dir {
                None => root,
                Some(dir) if dir.components().all(|c| matches!(c, std::path::Component::Normal(_))) => root.join(dir),
                Some(dir) => {
                    self.fail(
                        file,
                        format!("`dir = \"{}\"` is not a path inside the repository", dir.display()),
                    );
                    continue;
                }
            };
            let manifest = root.join("plugin.toml");
            let Some(raw) = self.read::<PluginFile>(&manifest) else {
                continue;
            };
            self.name(&manifest, "plugin", &raw.name);
            if outer.iter().chain(&plugins).any(|p| p.name == raw.name) {
                self.fail(file, format!("plugin `{}` is declared twice", raw.name));
            }
            let mut hooks = Vec::new();
            for hook in raw.hook {
                if event_path(&hook.on, "seq", false).is_none() {
                    self.fail(&manifest, format!("hook on unknown event type `{}`", hook.on));
                }
                if hook.command.is_empty() {
                    self.fail(&manifest, format!("hook on `{}` has an empty command", hook.on));
                }
                let timeout = self.timeout(&manifest, hook.timeout.as_deref());
                if timeout.is_some_and(|t| t.as_secs_f64() > 300.0) {
                    self.fail(&manifest, format!("hook on `{}`: the timeout ceiling is 300s", hook.on));
                }
                hooks.push(OrchestratorHook {
                    on: hook.on,
                    command: hook.command,
                    timeout,
                });
            }
            for (key, value) in &raw.params {
                if param_type(value).is_none() {
                    self.fail(
                        &manifest,
                        format!("param `{key}`: a default is a string, an integer or a boolean"),
                    );
                }
            }
            let mut agent: Settings = raw.agent.into();
            // `${params.<key>}` in an `env` value is replaced per agent; its form is checked as a variable's.
            let env = std::mem::take(&mut agent.env);
            let named = (env.iter()).map(|(k, v)| (k.clone(), v.replace(PARAMS, "${params_")));
            self.env_values(&manifest, "env", &named.collect());
            let mut agent = self.settings(&manifest, &root, agent, &[]);
            agent.env = env;
            let dir = root.to_string_lossy();
            agent.services = self.services(&manifest, raw.service);
            agent.substitute(PLUGIN_DIR, &dir);
            let mut unknown = BTreeSet::new();
            agent.clone().strings_mut(&mut |text, _| {
                let refs = text.split(PARAMS).skip(1).filter_map(|rest| rest.split_once('}'));
                unknown.extend(refs.map(|(k, _)| k.to_owned()).filter(|k| !raw.params.contains_key(k)));
            });
            for key in unknown {
                self.fail(
                    &manifest,
                    format!("`${{params.{key}}}`: no param `{key}` in `[params]`"),
                );
            }
            let prompt = raw.prompt.and_then(|p| self.text(&manifest, &root.join(p)));
            plugins.push(Plugin {
                name: raw.name,
                version: raw.version,
                harnesses: raw.harnesses,
                bin: Some(root.join("bin")).filter(|b| b.is_dir()),
                root,
                agent,
                prompt,
                hooks,
                params: raw.params,
            });
        }
        plugins
    }
}

/// The type of a plugin parameter's value, when it is one a parameter may have.
pub(super) fn param_type(value: &serde_json::Value) -> Option<&'static str> {
    match value {
        serde_json::Value::String(_) => Some("string"),
        serde_json::Value::Bool(_) => Some("boolean"),
        serde_json::Value::Number(n) if n.is_i64() => Some("integer"),
        _ => None,
    }
}
