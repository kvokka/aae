//! Reading the files: every file is parsed and checked on its own here; references between files are resolved by
//! `check`.

use super::{
    Config, ConfigError, Name, OrchestratorHook, Pack, Plugin, Project, Rig, Settings, Trigger, duration,
    files::{OrchestratorFile, PackFile, PluginDecl, PluginFile, ProjectFile, TriggerFile},
    schema::event_path,
};
use serde::de::DeserializeOwned;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

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
        let settings = self.settings(file, &base, raw.settings, &names);
        let overrides = self.overrides(file, &base, raw.overrides, &names);
        let mut projects: Vec<Project> = Vec::new();
        for dir in &raw.projects {
            let Some(project) = self.project(&resolve(&base, dir), &plugins) else {
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
        let config = Config {
            harnesses: raw.harness,
            settings,
            overrides,
            plugins,
            projects,
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
            let root = resolve(base, &decl.path);
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
                if event_path(&hook.on, "seq").is_none() {
                    self.fail(&manifest, format!("hook on unknown event type `{}`", hook.on));
                }
                if hook.command.is_empty() {
                    self.fail(&manifest, format!("hook on `{}` has an empty command", hook.on));
                }
                let timeout = self.timeout(&manifest, hook.timeout.as_deref());
                hooks.push(OrchestratorHook {
                    on: hook.on,
                    command: hook.command,
                    timeout,
                });
            }
            // A plugin attaches no other plugin to agents.
            let agent = self.settings(&manifest, &root, raw.agent, &[]);
            plugins.push(Plugin {
                name: raw.name,
                version: raw.version,
                root,
                agent,
                hooks,
            });
        }
        plugins
    }

    /// Checks one layer of settings and makes its paths absolute; `plugins` are the names it may attach.
    pub(super) fn settings(&mut self, file: &Path, base: &Path, mut settings: Settings, plugins: &[&str]) -> Settings {
        for (name, mcp) in &settings.mcp {
            if mcp.command.as_ref().is_some_and(Vec::is_empty) || mcp.command.is_some() == mcp.url.is_some() {
                self.fail(
                    file,
                    format!("mcp `{name}` needs exactly one of a non-empty `command` or `url`"),
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

    fn project(&mut self, dir: &Path, outer: &[Plugin]) -> Option<Project> {
        let file = dir.join("aae.toml");
        let raw: ProjectFile = self.read(&file)?;
        let name = raw
            .name
            .unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().into_owned());
        self.name(&file, "project", &name);
        let plugins = self.plugins(&file, dir, raw.plugin, outer);
        let all: Vec<&Plugin> = outer.iter().chain(&plugins).collect();
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        let settings = self.settings(&file, dir, raw.settings, &names);
        let overrides = self.overrides(&file, dir, raw.overrides, &names);
        let mut rigs = BTreeMap::new();
        for (rig, r) in raw.rigs {
            self.name(&file, "rig", &rig);
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
        for pack_dir in &raw.packs {
            let root = resolve(dir, pack_dir);
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
        Some(Project {
            name,
            root: dir.to_owned(),
            settings,
            overrides,
            plugins,
            rigs,
            packs,
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

    /// The entries of a directory in name order; a missing directory has none.
    fn entries(&mut self, dir: &Path) -> Vec<PathBuf> {
        if !dir.exists() {
            return Vec::new();
        }
        match fs::read_dir(dir).and_then(|d| d.map(|e| e.map(|e| e.path())).collect::<Result<Vec<_>, _>>()) {
            Ok(mut paths) => {
                paths.sort();
                paths
            }
            Err(e) => {
                self.fail(dir, e.to_string());
                Vec::new()
            }
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

pub(super) fn parent(file: &Path) -> PathBuf {
    file.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// `path` relative to `base`, with a leading `~/` meaning the home directory.
pub(super) fn resolve(base: &Path, path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => base.join(path),
    }
}
