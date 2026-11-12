//! The world the run tests share: a project on the scripted driver, the run machine on the managed clock.
// Each test crate uses a part of this module.
#![allow(dead_code)]

use aae_core::{
    BoxFuture, Clock, CoreHandle, Journal, config,
    driver::{Normalized, SessionState, StopReason},
    open,
    run::{self, Create, HookRunner, Msg},
};
use aae_testkit::{ScriptedDriver, Step};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

pub(crate) const FILES: &[(&str, &str)] = &[
    ("config.toml", "projects = [\"shop\"]\n[harness.scripted]\n"),
    ("shop/aae.toml", "[rigs.web]\npath = \"web\"\n"),
    ("shop/web/.keep", ""),
    ("shop/agents/dev/agent.toml", "harness = \"scripted\"\n"),
    (
        "shop/formulas/build.toml",
        "name = \"build\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n",
    ),
    ("shop/formulas/build.md", "Build {{ rig.name }}."),
];

/// A plugin with a hook on `run.attempt_failed`: decisions on it wait for its deadline instead of applying at once.
pub(crate) const POLICY: &[(&str, &str)] = &[
    (
        "config.toml",
        "projects = [\"shop\"]\n[harness.scripted]\n[[plugin]]\npath = \"policy\"\n",
    ),
    (
        "policy/plugin.toml",
        "name = \"policy\"\nversion = \"0.1.0\"\n[[hook]]\non = \"run.attempt_failed\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n",
    ),
];

pub(crate) struct World {
    pub(crate) core: CoreHandle<Msg>,
    pub(crate) driver: ScriptedDriver,
    pub(crate) journal: PathBuf,
    _dir: tempfile::TempDir,
}

/// A hook runner lost after the dispatch: decisions come from the test or from the deadline (contracts 3.3).
struct Lost;

impl HookRunner for Lost {
    fn run<'a>(&'a self, _: &'a CoreHandle<Msg>, _: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

/// The world with a lost hook runner: the test decides.
pub(crate) fn world(extra: &[(&str, &str)]) -> World {
    world_with(extra, Arc::new(Lost))
}

pub(crate) fn world_with(extra: &[(&str, &str)], hooks: Arc<dyn HookRunner>) -> World {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in FILES
        .iter()
        .filter(|(p, _)| !extra.iter().any(|(e, _)| e == p))
        .chain(extra)
    {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let config = config::load(&dir.path().join("config.toml")).unwrap();
    let driver = ScriptedDriver::new(1);
    let journal = dir.path().join("journal");
    let (core, _task) = run::start(
        open(&dir.path().join("state.db")).unwrap(),
        Clock::managed(0),
        Arc::new(config),
        vec![Arc::new(driver.clone())],
        hooks,
        Journal::new(&journal),
    );
    World {
        core,
        driver,
        journal,
        _dir: dir,
    }
}

impl World {
    pub(crate) async fn create(&self) -> String {
        self.create_on("web").await
    }

    pub(crate) async fn create_on(&self, rig: &str) -> String {
        let create = Create {
            project: "shop".into(),
            rig: rig.into(),
            formula: "build".into(),
            inputs: serde_json::Map::new(),
            correlation: None,
            depth: 0,
            parent_run: None,
            continue_from: None,
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// The journal events of `run`, `(type, data)`, after letting the system run for `wait`.
    pub(crate) async fn events(&self, run: &str, wait: Duration) -> Vec<(String, Value)> {
        tokio::time::sleep(wait).await;
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        all.into_iter()
            .filter(|e| e["run"] == run)
            .map(|e| (e["type"].as_str().unwrap().to_owned(), e["data"].clone()))
            .collect()
    }

    pub(crate) async fn states(&self, run: &str) -> Vec<String> {
        let events = self.events(run, Duration::from_millis(200)).await;
        let changes = events.into_iter().filter(|(t, _)| t == "run.state_changed");
        changes.map(|(_, d)| d["to"].as_str().unwrap().to_owned()).collect()
    }

    pub(crate) async fn seq_of(&self, run: &str, kind: &str, nth: usize) -> i64 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        let found = all
            .into_iter()
            .filter(|e| e["run"] == run && e["type"] == kind)
            .nth(nth);
        found.unwrap_or_else(|| panic!("no {kind} #{nth}"))["seq"]
            .as_i64()
            .unwrap()
    }
}

pub(crate) fn last<'a>(events: &'a [(String, Value)], kind: &str) -> &'a Value {
    &events
        .iter()
        .rev()
        .find(|(t, _)| t == kind)
        .unwrap_or_else(|| panic!("no {kind}"))
        .1
}

pub(crate) fn hollow_turn() -> Vec<Step> {
    vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Emit(Normalized::TurnEnded {
            stop_reason: StopReason::Completed,
        }),
        Step::Emit(Normalized::State {
            state: SessionState::Idle,
            background: false,
        }),
    ]
}
