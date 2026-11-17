//! Reading the files: every file is parsed and checked on its own here; references between files are resolved by
//! `check`.

use super::{
    Config, ConfigError, Name, Pack, Plugin, Project, Settings, Trigger,
    defaults::Given,
    duration,
    files::{OrchestratorFile, PackFile, ProjectFile, TriggerFile},
    items::{parent, resolve},
    plugin::param_type,
};
use serde::de::DeserializeOwned;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    num::NonZeroU32,
    path::Path,
};

/// Runs started at once when the orchestrator config does not say (docs/design/contracts.md 6).
const DEFAULT_MAX_RUNS: u32 = 8;
/// What the harness hooks and MCP commands of a plugin's contribution say for the plugin's directory.
pub(super) const PLUGIN_DIR: &str = "${AAE_PLUGIN_DIR}";
/// How a plugin's strings name one of its parameters: `${params.<key>}`.
pub(super) const PARAMS: &str = "${params.";
/// How a plugin's strings name the directory the daemon keeps for its data.
pub(super) const PLUGIN_DATA: &str = "${AAE_PLUGIN_DATA}";
/// Failed runs of a bead before the core closes its gate (docs/design/config.md).
const DEFAULT_BEAD_FAILURES: u32 = 3;

#[derive(Default)]
pub(super) struct Loader {
    pub(super) errors: Vec<ConfigError>,
    pub(super) harnesses: BTreeSet<String>,
    pub(super) adapters: &'static [&'static crate::driver::Adapter],
    /// Where the git sources were downloaded to, by whoever loads the config.
    pub(super) checkouts: super::Checkouts,
}

impl Loader {
    pub(super) fn run(mut self, file: &Path) -> Result<Config, Vec<ConfigError>> {
        let base = parent(file);
        let raw = match file.exists() {
            true => self.read::<OrchestratorFile>(file).unwrap_or_default(),
            false => OrchestratorFile::default(),
        };
        // A harness of the build needs no section (docs/design/config.md); a section may name one the build lacks, as
        // the scripted harness of the tests.
        let built = self.adapters.iter().map(|a| a.name.to_owned());
        self.harnesses = raw.harness.keys().cloned().chain(built).collect();
        let plugins = self.plugins(file, &base, raw.plugin, &[]);
        let all: Vec<&Plugin> = plugins.iter().collect();
        let mut settings = self.settings(file, &base, raw.settings, &all);
        settings.services = self.services(file, raw.service);
        let (edits, variants) = self.agents(file, &base, raw.agents, &all);
        let bundled = super::check::bundled(&mut self, file, raw.builtin);
        let watchdog = super::check::watchdog(&mut self, file, raw.watchdog);
        let mut projects: Vec<Project> = Vec::new();
        let top = Given {
            file,
            layer: super::Layer::Orchestrator,
            rounds: raw.rounds,
            inputs: &raw.inputs,
            exec: &raw.exec,
        };
        self.env_values(file, "exec.env", &raw.exec.env);
        for dir in &raw.projects {
            let Some(project) = self.project(&resolve(&base, dir), &plugins, &bundled, &top) else {
                continue;
            };
            if projects.iter().any(|p| p.name == project.name) {
                self.fail(file, format!("two projects are named `{}`", project.name));
            }
            projects.push(project);
        }
        for project in &mut projects {
            super::check::project(&mut self, project, &variants);
            super::prompt::project(&mut self, project);
            super::variants::check_project(&mut self, project, &variants);
        }
        super::check::orchestrator_edits(&mut self, file, edits.keys(), &projects, &variants);
        super::variants::check_orchestrator(&mut self, file, &variants, &projects);
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
        let mut config = Config {
            api_tcp: raw.api_tcp,
            api_token: String::new(),
            timezone,
            max_runs: raw.max_runs.map_or(DEFAULT_MAX_RUNS, NonZeroU32::get),
            bead_failures: raw.bead_failures.map_or(DEFAULT_BEAD_FAILURES, NonZeroU32::get),
            harnesses: raw.harness,
            settings,
            edits,
            variants,
            adapters: self.adapters,
            plugins,
            bundled,
            watchdog,
            projects,
            bridge: Vec::new(),
            plugin_data: None,
            service_wait: std::time::Duration::from_secs(30),
        };
        super::subagents::check(&mut self, &config);
        // A harness of the build the steps run on gets its defaults, none without its section.
        for harness in super::compose::check_runs(&mut self, &config) {
            config.harnesses.entry(harness).or_default();
        }
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
            .and_then(|text| toml::from_str(&text).map_err(|e| super::retired::retired(e.to_string(), &text)));
        parsed.map_err(|e| self.fail(file, e)).ok()
    }

    pub(super) fn name(&mut self, file: &Path, what: &str, name: &str) {
        if name.is_empty() || name.contains('.') {
            self.fail(file, format!("{what} name `{name}` must be non-empty and have no `.`"));
        }
    }

    /// Checks one layer of settings and makes its paths absolute; `plugins` are the names it may attach.
    pub(super) fn settings(
        &mut self,
        file: &Path,
        base: &Path,
        mut settings: Settings,
        plugins: &[&Plugin],
    ) -> Settings {
        self.env_values(file, "env", &settings.env);
        for (name, mcp) in &settings.mcp {
            if let Some(why) = mcp.malformed() {
                self.fail(file, format!("mcp `{name}` {why}"));
            }
        }
        for (name, hook) in &settings.hooks {
            if let Some(why) = hook.malformed() {
                self.fail(file, format!("hook `{name}` {why}"));
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
            let Some(plugin) = plugins.iter().find(|p| p.name == used.name) else {
                self.fail(file, format!("plugin `{}` is not declared", used.name));
                continue;
            };
            for (key, value) in &used.params {
                let at = format!("plugin `{}`: param `{key}`", used.name);
                match plugin.params.get(key).and_then(param_type) {
                    None => self.fail(file, format!("{at} is not in the plugin's `[params]`")),
                    Some(want) if param_type(value) != Some(want) => self.fail(file, format!("{at} is of type {want}")),
                    Some(_) => {}
                }
            }
        }
        if let Some(harness) = settings.harness.as_ref().filter(|h| !self.harnesses.contains(*h)) {
            self.fail(
                file,
                format!("harness `{harness}` is neither of this build nor in the orchestrator config"),
            );
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
                format!("`native.{harness}`: no harness `{harness}` in this build or the orchestrator config"),
            );
        }
        settings
    }

    pub(super) fn timeout(&mut self, file: &Path, text: Option<&str>) -> Option<std::time::Duration> {
        text.and_then(|t| duration(t).map_err(|e| self.fail(file, e)).ok())
    }

    fn project(
        &mut self,
        dir: &Path,
        outer: &[Plugin],
        bundled: &[super::Bundled],
        top: &Given<'_>,
    ) -> Option<Project> {
        let file = dir.join("aae.toml");
        let raw: ProjectFile = self.read(&file)?;
        let name = raw
            .name
            .unwrap_or_else(|| dir.file_name().unwrap_or_default().to_string_lossy().into_owned());
        self.name(&file, "project", &name);
        let plugins = self.plugins(&file, dir, raw.plugin, outer);
        let all: Vec<&Plugin> = outer.iter().chain(&plugins).collect();
        let names = all.as_slice();
        let mut settings = self.settings(&file, dir, raw.settings, names);
        settings.services = self.services(&file, raw.service);
        let (edits, variants) = self.agents(&file, dir, raw.agents, names);
        let (root, rigs) = self.places(&file, dir, raw.root, raw.rigs, names);
        let mut packs = vec![self.pack(dir, &file, name.clone(), raw.trigger, true, names)];
        for source in &raw.packs {
            let Some(root) = self.root(&file, dir, source) else {
                continue;
            };
            let manifest = root.join("pack.toml");
            let Some(raw) = self.read::<PackFile>(&manifest) else {
                continue;
            };
            self.name(&manifest, "pack", &raw.name);
            for required in raw
                .requires_plugins
                .iter()
                .filter(|r| !names.iter().any(|p| &p.name == *r))
            {
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
            let mut pack = self.pack(&root, &manifest, raw.name, raw.trigger, false, names);
            if packs.iter().any(|p| p.name == pack.name) {
                self.fail(&file, format!("two packs are named `{}`", pack.name));
            }
            (pack.version, pack.requires_plugins) = (Some(raw.version), raw.requires_plugins);
            pack.settings = self.settings(&manifest, &root, raw.settings.into(), &[]);
            packs.push(pack);
        }
        let own = Given {
            file: &file,
            layer: super::Layer::Project,
            rounds: raw.rounds,
            inputs: &raw.inputs,
            exec: &raw.exec,
        };
        self.env_values(&file, "exec.env", &raw.exec.env);
        self.formula_defaults(&mut packs, [top, &own]);
        let mut exec = super::ExecEnv::default();
        exec.under(&raw.exec, &super::Layer::Project);
        exec.under(top.exec, &super::Layer::Orchestrator);
        let bundled = super::check::project_bundled(self, &file, bundled, raw.builtin, &packs);
        Some(Project {
            name,
            dir: dir.to_owned(),
            root,
            settings,
            edits,
            variants,
            plugins,
            bundled,
            rigs,
            packs,
            prompts: BTreeMap::new(),
            tracker: super::TrackerLink::default(),
            exec,
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
        plugins: &[&super::Plugin],
    ) -> Pack {
        let mut agents = BTreeMap::new();
        for dir in self.entries(&root.join("agents")).into_iter().filter(|p| p.is_dir()) {
            let agent = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if let Some(a) = self.agent(&dir, plugins) {
                self.name(&dir.join("agent.toml"), "agent", &agent);
                agents.insert(agent, a);
            }
        }
        let formulas = self.formulas(&root.join("formulas"));
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
            settings: Settings::default(),
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
