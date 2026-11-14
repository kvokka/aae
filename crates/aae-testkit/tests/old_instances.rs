//! Invariant 6 (spec §5.5, docs/design/contracts.md 8): a new attempt starts only once nothing is left of the
//! instance it replaces, its marked descendants included, and never touches a live shared instance.
#![cfg(target_os = "linux")] // the sweep reads `/proc`

mod common;

use aae_core::{
    driver::{Failure, FailureKind, Normalized, Source},
    run::{Decision, Msg},
};
use aae_testkit::{ScriptedDriver, Step};
use common::{POLICY, World, killed, marked, world_driven};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// The decision hook of [`POLICY`] and a second rig.
fn files() -> Vec<(&'static str, &'static str)> {
    let rigs = [
        (
            "shop/aae.toml",
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        ),
        ("shop/api/.keep", ""),
    ];
    [POLICY, &rigs].concat()
}

impl World {
    /// Decides a new session on the first failed attempt of `run` and waits for it to run.
    async fn retry(&self, run: &str) {
        let decision = Decision::NewSession {
            after: Duration::ZERO,
            gate: None,
        };
        self.until(Some(run), "run.attempt_failed", 0).await;
        self.decide(run, "run.attempt_failed", 0, decision).await;
        let start = Instant::now();
        loop {
            let states = self.states(run).await;
            if states[states.len() - 2..] == ["starting", "running"] {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "{states:?}");
        }
    }

    /// Waits until `run` runs.
    async fn running(&self, run: &str) {
        let start = Instant::now();
        while !self.states(run).await.iter().any(|s| s == "running") {
            assert!(start.elapsed() < Duration::from_secs(10), "{run} never ran");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_new_attempt_starts_only_after_every_process_of_the_old_instance_is_gone() {
    let driver = ScriptedDriver::new(1);
    driver.script(vec![Step::Exit {
        code: Some(1),
        signal: None,
    }]);
    let w = world_driven(&files(), driver);
    let run = w.create().await;
    let failed = w.until(None, "instance.failed", 0).await;
    let id = failed["instance"]["id"].as_str().unwrap().to_owned();
    // The sweep of its death is done: what carries its marker now is a descendant that outran it.
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let start = Instant::now();
    let sweeps = "SELECT count(*) FROM effects WHERE payload LIKE '%\"op\":\"sweep\"%'";
    while db.query_row(sweeps, [], |r| r.get::<_, i64>(0)).unwrap() > 0 {
        assert!(start.elapsed() < Duration::from_secs(10), "the sweep never ran");
        tokio::time::sleep(Duration::from_millis(10)).await;
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut orphan = marked(&id);
    w.retry(&run).await;
    // Dead is a zombie at least (some kernels show it before `waitpid` sees it).
    let pid = i32::try_from(orphan.id()).unwrap();
    assert_eq!(
        aae_core::orphans::start_time(pid),
        None,
        "killed before the new attempt ran"
    );
    let _ = orphan.wait();
    w.until(None, "instance.ready", 1).await;
}

#[tokio::test(start_paused = true)]
async fn a_new_attempt_on_a_shared_instance_leaves_its_processes_alone() {
    let driver = ScriptedDriver::new(2);
    let lost = Failure::new(FailureKind::Unknown, Source::Adapter);
    // The first session fails once the second one runs on the same instance and the orphan runs.
    let failure = Arc::new(Notify::new());
    driver.script(vec![
        Step::Await(failure.clone()),
        Step::Emit(Normalized::Failure(lost)),
    ]);
    let w = world_driven(&files(), driver);
    let (one, two) = (w.create().await, w.create_on("api").await);
    let started = w.until(None, "instance.starting", 0).await;
    let mut orphan = marked(started["instance"]["id"].as_str().unwrap());
    w.running(&two).await;
    failure.notify_one();
    w.retry(&one).await;
    assert!(
        orphan.try_wait().unwrap().is_none(),
        "the live instance keeps its processes"
    );
    assert_eq!(
        w.until(None, "instance.starting", 0).await,
        started,
        "and serves the new attempt"
    );
    let _ = orphan.kill();
    let _ = orphan.wait();
}

/// A descendant that outran the sweep of its stopped instance (invariants 7, 20): the periodic resync and the resync
/// at a start kill it; a marker this daemon never gave is another daemon's and stays.
#[tokio::test(start_paused = true)]
async fn the_resync_sweeps_what_carries_the_marker_of_a_gone_instance() {
    let mut w = world_driven(&files(), ScriptedDriver::new(1));
    let run = w.create().await;
    let started = w.until(None, "instance.starting", 0).await;
    let id = started["instance"]["id"].as_str().unwrap().to_owned();
    let cancel = Msg::Cancel {
        run: run.clone(),
        reason: "done".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    w.until(None, "instance.stopped", 0).await;
    let late = marked(&id);
    let mut foreign = marked(&format!("foreign-{}", std::process::id()));
    tokio::time::sleep(Duration::from_secs(300)).await;
    assert!(killed(late).await, "the periodic resync sweeps it");
    assert!(foreign.try_wait().unwrap().is_none(), "another daemon's process");
    let _ = foreign.kill();
    let _ = foreign.wait();

    let late = marked(&id);
    w.restart().await;
    assert!(killed(late).await, "the resync at the start sweeps it");
}

/// The resync sweeps only what is no longer recorded: a live instance keeps its marked processes (invariant 20).
#[tokio::test(start_paused = true)]
async fn the_resync_leaves_the_processes_of_a_live_instance_alone() {
    let w = world_driven(&files(), ScriptedDriver::new(1));
    let gone = w.create().await;
    let first = w.until(None, "instance.starting", 0).await;
    w.running(&gone).await;
    // Its activity keeps the silence watchdog off the live session past the resync.
    let beat = [
        Step::Wait(Duration::from_secs(60)),
        Step::Emit(Normalized::Activity { tool_running: false }),
    ];
    w.driver.script(beat.iter().cycle().take(12).cloned().collect());
    let live = w.create_on("api").await;
    let second = w.until(None, "instance.starting", 1).await;
    let mut kept = marked(second["instance"]["id"].as_str().unwrap());
    let cancel = Msg::Cancel {
        run: gone,
        reason: "done".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    w.until(None, "instance.stopped", 0).await;
    // Spawned after `kept`, so the one walk of `/proc` reaches it later.
    let late = marked(first["instance"]["id"].as_str().unwrap());
    tokio::time::sleep(Duration::from_secs(300)).await;
    assert!(killed(late).await, "the resync ran");
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let start = Instant::now();
    let pending = "SELECT count(*) FROM effects WHERE key = 'daemon'";
    while db.query_row(pending, [], |r| r.get::<_, i64>(0)).unwrap() > 0 {
        assert!(start.elapsed() < Duration::from_secs(10), "the resync never ended");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        kept.try_wait().unwrap().is_none(),
        "a live instance keeps its processes"
    );
    assert_eq!(w.states(&live).await.last().unwrap(), "running");
    let _ = kept.kill();
    let _ = kept.wait();
}
