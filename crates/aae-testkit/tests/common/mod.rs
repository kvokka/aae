//! The world the run tests share: a project on the scripted driver, the run machine on the managed clock.
// Each test crate uses a part of this module.
#![allow(dead_code)]

use aae_core::{
    BoxFuture, Clock, CoreHandle, Error, Journal, config,
    driver::{Normalized, SessionState, StopReason},
    open,
    run::{self, Create, Decision, HookRunner, Msg},
};
use aae_testkit::{MemoryTracker, ScriptedDriver, Step};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::task::JoinHandle;

pub(crate) const FILES: &[(&str, &str)] = &[
    (
        "config.toml",
        "projects = [\"shop\"]\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n",
    ),
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
        "projects = [\"shop\"]\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n[[plugin]]\npath = \"policy\"\n",
    ),
    (
        "policy/plugin.toml",
        "name = \"policy\"\nversion = \"0.1.0\"\n[[hook]]\non = \"run.attempt_failed\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n",
    ),
];

pub(crate) struct World {
    pub(crate) core: CoreHandle<Msg>,
    pub(crate) driver: ScriptedDriver,
    /// The tracker of project `shop`; it outlives restarts, as a tracker database would.
    pub(crate) tracker: MemoryTracker,
    pub(crate) journal: PathBuf,
    /// The managed clock of every daemon of the world: a restart does not turn time back.
    pub(crate) clock: Clock,
    hooks: Arc<dyn HookRunner>,
    task: JoinHandle<Result<(), Error>>,
    pub(crate) dir: tempfile::TempDir,
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

/// The world with a lost hook runner on `driver`.
pub(crate) fn world_driven(extra: &[(&str, &str)], driver: ScriptedDriver) -> World {
    world_on(extra, Arc::new(Lost), driver)
}

pub(crate) fn world_with(extra: &[(&str, &str)], hooks: Arc<dyn HookRunner>) -> World {
    world_on(extra, hooks, ScriptedDriver::new(1))
}

/// The world on `driver`.
pub(crate) fn world_on(extra: &[(&str, &str)], hooks: Arc<dyn HookRunner>, driver: ScriptedDriver) -> World {
    // The state and the journal sync every write: in memory, where the machine has it, that costs nothing.
    let shm = Path::new("/dev/shm");
    let dir = if shm.is_dir() {
        tempfile::tempdir_in(shm)
    } else {
        tempfile::tempdir()
    };
    let dir = dir.unwrap();
    for (path, text) in FILES
        .iter()
        .filter(|(p, _)| !extra.iter().any(|(e, _)| e == p))
        .chain(extra)
    {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let tracker = MemoryTracker::new();
    let clock = Clock::managed(0);
    let (core, task) = core_on(dir.path(), clock, hooks.clone(), &driver, &tracker);
    World {
        core,
        driver,
        tracker,
        journal: dir.path().join("journal"),
        clock,
        hooks,
        task,
        dir,
    }
}

fn core_on(
    dir: &Path,
    clock: Clock,
    hooks: Arc<dyn HookRunner>,
    driver: &ScriptedDriver,
    tracker: &MemoryTracker,
) -> (CoreHandle<Msg>, JoinHandle<Result<(), Error>>) {
    let mut config = config::load(&dir.join("config.toml")).unwrap();
    // `env <command>` runs the command itself: the HTTP probe stands in for a stdio server behind the bridge.
    config.bridge = vec!["env".to_owned()];
    // As the daemon does for a tracker it makes: `shop` has the memory tracker.
    for p in config.projects.iter_mut().filter(|p| p.name == "shop") {
        p.tracker.present = true;
        p.tracker.actor = Some("TRACKER_ACTOR".to_owned());
    }
    run::start(
        open(&dir.join("state.db")).unwrap(),
        clock,
        Arc::new(config),
        vec![Arc::new(driver.clone())],
        [(
            "shop".to_owned(),
            Arc::new(tracker.clone()) as Arc<dyn aae_core::tracker::Tracker>,
        )]
        .into(),
        hooks,
        Journal::new(&dir.join("journal")),
    )
}

impl World {
    /// The daemon dies without a word (its writer is dropped mid-flight) and a new one starts on its state, with a
    /// new driver: the old one's processes are unknown to it, the transcripts of its sessions survive.
    pub(crate) async fn restart(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
        self.driver = self.driver.after_crash();
        (self.core, self.task) = core_on(
            self.dir.path(),
            self.clock,
            self.hooks.clone(),
            &self.driver,
            &self.tracker,
        );
    }

    /// Kills the daemon's writer from any task: `kill -9` lands wherever the daemon is. [`Self::restart`] follows.
    pub(crate) fn killer(&self) -> tokio::task::AbortHandle {
        self.task.abort_handle()
    }

    /// The `n`-th event `kind` of `run` (`None`: of any run), waiting for it up to 10 s.
    pub(crate) async fn until(&self, run: Option<&str>, kind: &str, n: usize) -> Value {
        let started = std::time::Instant::now();
        loop {
            let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap_or_default();
            let found = all
                .into_iter()
                .filter(|e| e["type"] == kind && run.is_none_or(|r| e["run"] == r))
                .nth(n);
            if let Some(event) = found {
                return event["data"].clone();
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no {kind} #{n}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub(crate) async fn create(&self) -> String {
        self.create_on("web").await
    }

    pub(crate) async fn create_on(&self, rig: &str) -> String {
        self.create_in("shop", rig).await
    }

    pub(crate) async fn create_in(&self, project: &str, rig: &str) -> String {
        let create = Create {
            project: project.into(),
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

    /// Decides the `nth` deciding event `kind` of `run`.
    pub(crate) async fn decide(&self, run: &str, kind: &str, nth: usize, decision: Decision) {
        let seq = self.seq_of(run, kind, nth).await;
        let msg = Msg::Decide {
            run: run.into(),
            seq,
            decision,
        };
        self.core.send(None, msg).await.unwrap();
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

/// A real process marked with the instance `id`, as a tool command of the harness would be; `/proc` shows its marker.
pub(crate) fn marked(id: &str) -> std::process::Child {
    let child = std::process::Command::new("sleep")
        .arg("300")
        .env(aae_core::orphans::INSTANCE_ENV, id)
        .spawn()
        .unwrap();
    // `spawn` returns while the child is still inside `execve`, where the kernel shows an empty environment: a sweep
    // that walks `/proc` right then misses the marker. Without `/proc` the read fails and nothing is waited for.
    let environ = format!("/proc/{}/environ", child.id());
    let marker = format!("{}={id}", aae_core::orphans::INSTANCE_ENV);
    let start = std::time::Instant::now();
    while fs::read(&environ).is_ok_and(|e| !e.split(|b| *b == 0).any(|var| var == marker.as_bytes())) {
        assert!(start.elapsed() < Duration::from_secs(10), "the marker never showed");
        std::thread::sleep(Duration::from_millis(1));
    }
    child
}

/// Whether `child` is killed within a few real seconds; the sweep walks `/proc` on a blocking thread.
pub(crate) async fn killed(mut child: std::process::Child) -> bool {
    for _ in 0..300 {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}
