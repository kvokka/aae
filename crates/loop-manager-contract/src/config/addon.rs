//! An addon (docs/design/config.md "Addon"): its manifest as loaded, what its `[agent]` may give, and the strings of
//! its parts that `${LOOP_MANAGER_ADDON_DIR}`, `${LOOP_MANAGER_ADDON_DATA}` and `${params.<key>}` are filled in.

use super::{
    Addon, Hook, Mcp, OrchestratorHook, Settings, Skill,
    files::{AddonDecl, AddonFile},
    load::{ADDON_DIR, Loader, PARAMS},
    schema::event_path,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// `[agent]` of an addon: the settings an addon may give the agents that attach it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddonAgent {
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

impl From<AddonAgent> for Settings {
    fn from(a: AddonAgent) -> Self {
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
    /// Every string of this layer an addon's text may reach (docs/design/config.md "Addon"): harness hook commands,
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
                .chain(std::iter::once(&mut s.bind))
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
    /// Addons declared by one file; `outer` are the addons already declared above it.
    pub(super) fn addons(&mut self, file: &Path, base: &Path, decls: Vec<AddonDecl>, outer: &[Addon]) -> Vec<Addon> {
        let mut addons: Vec<Addon> = Vec::new();
        for decl in decls {
            let Some(root) = self.addon_source(file, &decl).and_then(|s| self.root(file, base, &s)) else {
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
            let manifest = root.join("addon.toml");
            let Some(raw) = self.read::<AddonFile>(&manifest) else {
                continue;
            };
            self.name(&manifest, "addon", &raw.name);
            if outer.iter().chain(&addons).any(|p| p.name == raw.name) {
                self.fail(file, format!("addon `{}` is declared twice", raw.name));
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
            agent.services = self.services(&manifest, raw.service, true);
            agent.substitute(ADDON_DIR, &dir);
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
            addons.push(Addon {
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
        addons
    }
}

/// The type of an addon parameter's value, when it is one a parameter may have.
pub(super) fn param_type(value: &serde_json::Value) -> Option<&'static str> {
    match value {
        serde_json::Value::String(_) => Some("string"),
        serde_json::Value::Bool(_) => Some("boolean"),
        serde_json::Value::Number(n) if n.is_i64() => Some("integer"),
        _ => None,
    }
}
