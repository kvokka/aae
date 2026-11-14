//! Reading the files: every file is parsed and checked on its own here; references between files are resolved by
//! `check`.

use super::{
    Config, ConfigError, Name, OrchestratorHook, Pack, Plugin, Project, Rig, Settings, Trigger, duration,
    files::{OrchestratorFile, PackFile, PluginDecl, PluginFile, ProjectFile, TriggerFile},
    items::{parent, resolve},
    schema::event_path,
};
use serde::de::DeserializeOwned;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    num::NonZeroU32,
    path::Path,
};

/// Runs started at once when the orchestrator config does not say (spec §9.3).
const DEFAULT_MAX_RUNS: u32 = 8;
/// What the harness hooks and MCP commands of a plugin's contribution say for the plugin's directory.
pub(super) const PLUGIN_DIR: &str = "${AAE_PLUGIN_DIR}";
/// Failed runs of a bead before the core closes its gate (spec §10.3).
const DEFAULT_BEAD_FAILURES: u32 = 3;

#[derive(Default)]
pub(super) struct Loader {
    pub(super) errors: Vec<ConfigError>,
    pub(super) harnesses: BTreeSet<String>,
}

impl Loader {
    pub(super) fn run(mut self, file: &Path) -> Result<Config, Vec<ConfigError>> {
        let base = parent(file);
        let raw = match file.exists() {
            true => self.read::<OrchestratorFile>(file).unwrap_or_default(),
            false => OrchestratorFile::default(),
        };
        self.harnesses = raw.harness.keys().cloned().collect();
        let plugins = self.plugins(file, &base, raw.plugin, &[]);
        let names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
        let mut settings = self.settings(file, &base, raw.settings, &names);
        settings.services = self.services(file, raw.service, None);
        let overrides = self.overrides(file, &base, raw.overrides, &names);
        let bundled = super::check::bundled(&mut self, file, raw.builtin);
        let watchdog = super::check::watchdog(&mut self, file, raw.watchdog);
        let mut projects: Vec<Project> = Vec::new();
        for dir in &raw.projects {
            let Some(project) = self.project(&resolve(&base, dir), &plugins, &bundled) else {
                continue;
            };
            if projects.iter().any(|p| p.name == project.name) {
                self.fail(file, format!("two projects are named `{}`", project.name));
            }
            projects.push(project);
        }
        for project in &mut projects {
            super::check::project(&mut self, project);
        }
        super::check::orchestrator_overrides(&mut self, file, overrides.keys(), &projects);
        if raw.api_tcp.is_some_and(|a| !a.ip().is_loopback()) {
            self.fail(file, "`api_tcp` must be a loopback address");
        }
        let timezone = match raw.timezone.as_deref().map(jiff::tz::TimeZone::get) {
            None => jiff::tz::TimeZone::system(),
            Some(Ok(tz)) => tz,
            Some(Err(e)) => self
                .none(file, format!("`timezone`: {e}"))
                .unwrap_or(jiff::tz::TimeZone::UTC),
        };
        let config = Config {
            api_tcp: raw.api_tcp,
            api_token: String::new(),
            timezone,
            max_runs: raw.max_runs.map_or(DEFAULT_MAX_RUNS, NonZeroU32::get),
            bead_failures: raw.bead_failures.map_or(DEFAULT_BEAD_FAILURES, NonZeroU32::get),
            harnesses: raw.harness,
            settings,
            overrides,
            plugins,
            bundled,
            watchdog,
            projects,
            bridge: Vec::new(),
        };
        if self.errors.is_empty() {
            Ok(config)
        } else {
            Err(self.errors)
        }
    }

    pub(super) fn fail(&mut self, file: &Path, message: impl Into<String>) {
        self.errors.push(ConfigError {
            file: file.to_owned(),
            message: message.into(),
        });
    }

    pub(super) fn none<T>(&mut self, file: &Path, message: impl Into<String>) -> Option<T> {
        self.fail(file, message);
        None
    }

    pub(super) fn read<T: DeserializeOwned>(&mut self, file: &Path) -> Option<T> {
        let parsed = fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|text| toml::from_str(&text).map_err(|e| e.to_string().trim_end().to_owned()));
        parsed.map_err(|e| self.fail(file, e)).ok()
    }

    pub(super) fn name(&mut self, file: &Path, what: &str, name: &str) {
        if name.is_empty() || name.contains('.') {
            self.fail(file, format!("{what} name `{name}` must be non-empty and have no `.`"));
        }
    }

    /// Plugins declared by one file; `outer` are the plugins already declared above it.
    fn plugins(&mut self, file: &Path, base: &Path, decls: Vec<PluginDecl>, outer: &[Plugin]) -> Vec<Plugin> {
        let mut plugins: Vec<Plugin> = Vec::new();
        for decl in decls {
            let Some(root) = self.plugin_source(file, &decl).and_then(|s| self.root(file, base, &s)) else {
                continue;
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
            // A plugin attaches no other plugin to agents.
            let mut agent = self.settings(&manifest, &root, raw.agent, &[]);
            // Its harness hooks and MCP servers reach its own files as `${AAE_PLUGIN_DIR}`.
            let dir = root.to_string_lossy();
            agent.services = self.services(&manifest, raw.service, Some(&dir));
            let at = |text: &mut String| *text = text.replace(PLUGIN_DIR, &dir);
            agent.hooks.values_mut().filter_map(|h| h.command.as_mut()).for_each(at);
            agent
                .mcp
                .values_mut()
                .filter_map(|m| m.command.as_mut())
                .flatten()
                .for_each(at);
            let prompt = raw.prompt.and_then(|p| self.text(&manifest, &root.join(p)));
            plugins.push(Plugin {
                name: raw.name,
                version: raw.version,
                bin: Some(root.join("bin")).filter(|b| b.is_dir()),
                root,
                agent,
                prompt,
                hooks,
                params: decl.config,
            });
        }
        plugins
    }

    /// Checks one layer of settings and makes its paths absolute; `plugins` are the names it may attach.
    pub(super) fn settings(&mut self, file: &Path, base: &Path, mut settings: Settings, plugins: &[&str]) -> Settings {
        for (name, mcp) in &settings.mcp {
            if !mcp.well_formed() {
                self.fail(
                    file,
                    format!("mcp `{name}` needs a `command` or a `url`; with both it is a `[[service]]` with `mcp`"),
                );
            }
        }
        for (name, hook) in &settings.hooks {
            if hook.enabled && (hook.event.is_none() || hook.command.is_none()) {
                self.fail(file, format!("hook `{name}` needs `event` and `command`"));
            }
            self.timeout(file, hook.timeout.as_deref());
        }
        for (name, skill) in &mut settings.skills {
            match (&skill.path, skill.enabled) {
                (Some(path), _) => {
                    let path = resolve(base, path);
                    if !path.exists() {
                        self.fail(file, format!("skill `{name}`: `{}` does not exist", path.display()));
                    }
                    skill.path = Some(path);
                }
                (None, true) => self.fail(file, format!("skill `{name}` needs `path`")),
                (None, false) => {}
            }
        }
        for used in &settings.plugins {
            if !plugins.contains(&used.name.as_str()) {
                self.fail(file, format!("plugin `{}` is not declared", used.name));
            }
        }
        for harness in settings
            .native
            .keys()
            .filter(|h| !self.harnesses.contains(*h))
            .cloned()
            .collect::<Vec<_>>()
        {
            self.fail(
                file,
                format!("`native.{harness}`: no such harness in the orchestrator config"),
            );
        }
        settings
    }

    fn overrides(
        &mut self,
        file: &Path,
        base: &Path,
        overrides: BTreeMap<String, Settings>,
        plugins: &[&str],
    ) -> BTreeMap<String, Settings> {
        overrides
            .into_iter()
            .map(|(agent, s)| (agent, self.settings(file, base, s, plugins)))
            .collect()
    }

    pub(super) fn timeout(&mut self, file: &Path, text: Option<&str>) -> Option<std::time::Duration> {
        text.and_then(|t| duration(t).map_err(|e| self.fail(file, e)).ok())
    }

    fn project(&mut self, dir: &Path, outer: &[Plugin], bundled: &[super::Bundled]) -> Option<Project> {
        let file = dir.join("aae.toml");
        let raw: ProjectFile = self.read(&file)?;
        let name = raw
            .name
            .unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().into_owned());
        self.name(&file, "project", &name);
        let plugins = self.plugins(&file, dir, raw.plugin, outer);
        let all: Vec<&Plugin> = outer.iter().chain(&plugins).collect();
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        let mut settings = self.settings(&file, dir, raw.settings, &names);
        settings.services = self.services(&file, raw.service, None);
        let overrides = self.overrides(&file, dir, raw.overrides, &names);
        let mut rigs = BTreeMap::new();
        for (rig, r) in raw.rigs {
            self.name(&file, "rig", &rig);
            // The rig is the tracker label `rig:<name>`; the tracker takes labels of `[A-Za-z0-9_:-]`, 50 at most (§10.2).
            if rig.len() > 46 || !rig.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                self.fail(
                    &file,
                    format!("rig name `{rig}`: ASCII letters, digits, `-` and `_`, 46 at most"),
                );
            }
            let path = resolve(dir, &r.path);
            if !path.is_dir() {
                self.fail(&file, format!("rig `{rig}`: `{}` is not a directory", path.display()));
            }
            let triggers = r.triggers.iter().map(|t| reference(t)).collect();
            let settings = self.settings(&file, dir, r.settings, &names);
            let overrides = self.overrides(&file, dir, r.overrides, &names);
            rigs.insert(
                rig,
                Rig {
                    path,
                    vars: r.vars,
                    triggers,
                    settings,
                    overrides,
                },
            );
        }
        let mut packs = vec![self.pack(dir, &file, name.clone(), raw.trigger, true, &names)];
        for source in &raw.packs {
            let Some(root) = self.root(&file, dir, source) else {
                continue;
            };
            let manifest = root.join("pack.toml");
            let Some(raw) = self.read::<PackFile>(&manifest) else {
                continue;
            };
            self.name(&manifest, "pack", &raw.name);
            for required in raw.requires_plugins.iter().filter(|r| !names.contains(&r.as_str())) {
                self.fail(&manifest, format!("required plugin `{required}` is not declared"));
            }
            if let Some(min) = &raw.min_aae
                && version(min).is_none_or(|min| version(env!("CARGO_PKG_VERSION")) < Some(min))
            {
                self.fail(
                    &manifest,
                    format!("needs aae {min} or later, this is {}", env!("CARGO_PKG_VERSION")),
                );
            }
            let mut pack = self.pack(&root, &manifest, raw.name, raw.trigger, false, &names);
            if packs.iter().any(|p| p.name == pack.name) {
                self.fail(&file, format!("two packs are named `{}`", pack.name));
            }
            (pack.version, pack.requires_plugins) = (Some(raw.version), raw.requires_plugins);
            packs.push(pack);
        }
        let bundled = super::check::project_bundled(self, &file, bundled, raw.builtin, &packs);
        Some(Project {
            name,
            root: dir.to_owned(),
            settings,
            overrides,
            plugins,
            bundled,
            rigs,
            packs,
            tracker: super::TrackerLink::default(),
        })
    }

    /// The agents, formulas and triggers of a pack; `manifest` is the file that holds `triggers`.
    fn pack(
        &mut self,
        root: &Path,
        manifest: &Path,
        name: String,
        triggers: Vec<TriggerFile>,
        project: bool,
        plugins: &[&str],
    ) -> Pack {
        let mut agents = BTreeMap::new();
        for dir in self.entries(&root.join("agents")).into_iter().filter(|p| p.is_dir()) {
            let agent = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if let Some(a) = self.agent(&dir, plugins) {
                self.name(&dir.join("agent.toml"), "agent", &agent);
                agents.insert(agent, a);
            }
        }
        let mut formulas = BTreeMap::new();
        for file in self.entries(&root.join("formulas")) {
            if file.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            let Some((formula, f)) = self.formula(&file) else {
                continue;
            };
            if formulas.insert(formula.clone(), f).is_some() {
                self.fail(&file, format!("two formulas of the pack are named `{formula}`"));
            }
        }
        let mut loaded: Vec<Trigger> = Vec::new();
        for raw in triggers {
            let Some(trigger) = self.trigger(manifest, raw, project) else {
                continue;
            };
            if loaded.iter().any(|t| t.name == trigger.name) {
                self.fail(manifest, format!("two triggers are named `{}`", trigger.name));
            }
            loaded.push(trigger);
        }
        let root = root.to_owned();
        Pack {
            name,
            root,
            version: None,
            requires_plugins: Vec::new(),
            agents,
            formulas,
            triggers: loaded,
        }
    }
}

/// A reference as written: `pack.name` or a bare `name` (empty `pack` until `check` resolves it).
pub(super) fn reference(text: &str) -> Name {
    let (pack, name) = text.split_once('.').unwrap_or(("", text));
    Name {
        pack: pack.to_owned(),
        name: name.to_owned(),
    }
}

fn version(text: &str) -> Option<Vec<u64>> {
    text.split('.').map(|n| n.parse().ok()).collect()
}
