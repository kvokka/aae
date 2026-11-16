//! Edges of a new attempt (docs/design/contracts.md 3.3, 3.4, 6, 6.4): which session it takes after a `new_session`,
//! the remark a new session inherits from a daemon crash, no prompt past a stopped writer, and a hook on a failed
//! attempt that runs only once the stopped process is dead, within the deadline of its decision.

use crate::common;

use aae_core::{
    BoxFuture, CoreHandle,
    driver::{Failure, FailureKind, Normalized, Source},
    orphans,
    run::{Create, Decision, HookRunner, Msg},
};
use aae_testkit::{ScriptedDriver, Step};
use common::{POLICY, hollow_turn, last, world, world_on, world_with};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Polls `ready` for up to 10 s of real time.
async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < Duration::from_secs(10), "{what} never happened");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The session of attempt `n` of `run`, as the state records it.
fn session(dir: &Path, run: &str, n: u32) -> Option<String> {
    let db = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let sql = "SELECT session FROM attempts WHERE run = ?1 AND n = ?2";
    db.query_row(sql, rusqlite::params![run, n], |r| r.get(0))
        .ok()
        .flatten()
}

fn new_session() -> Decision {
    Decision::NewSession {
        after: Duration::ZERO,
        gate: None,
    }
}

/// A `new_session` attempt that died before its session was recorded leaves the old session given up: a `resume`
/// after it starts a new one.
#[tokio::test(start_paused = true)]
async fn a_resume_after_a_new_session_that_never_started_starts_a_new_session() {
    let mut w = world(POLICY);
    w.driver.script(vec![Step::Exit {
        code: None,
        signal: Some(9),
    }]);
    let run = w.create().await;
    w.until(Some(&run), "run.attempt_failed", 0).await;
    w.driver.stall_sessions();
    w.decide(&run, "run.attempt_failed", 0, new_session()).await;
    wait_for("the start of attempt 2's session", || {
        !w.driver.workdir(&run, 2).is_null()
    })
    .await;
    w.restart().await;
    w.until(Some(&run), "run.attempt_failed", 1).await;
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 1, resume).await;
    let dir = w.dir.path();
    wait_for("attempt 3's session", || session(dir, &run, 3).is_some()).await;
    assert_ne!(
        session(dir, &run, 3),
        session(dir, &run, 1),
        "the given-up session resumed"
    );
}

/// A run that continued another run's session and gave it up for a new session that never started does not resume
/// the other run's session.
#[tokio::test(start_paused = true)]
async fn a_resume_after_a_new_session_that_never_started_leaves_the_continued_session() {
    let mut w = world(POLICY);
    w.driver.script(hollow_turn());
    let first = w.create().await;
    w.until(Some(&first), "run.failed", 0).await;
    w.driver.script(vec![Step::Exit {
        code: None,
        signal: Some(9),
    }]);
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "build".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: Some(first.clone()),
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    w.until(Some(&run), "run.attempt_failed", 0).await;
    let dir = w.dir.path().to_owned();
    assert_eq!(session(&dir, &run, 1), session(&dir, &first, 1), "not continued");
    w.driver.stall_sessions();
    w.decide(&run, "run.attempt_failed", 0, new_session()).await;
    wait_for("the start of attempt 2's session", || {
        !w.driver.workdir(&run, 2).is_null()
    })
    .await;
    w.restart().await;
    w.until(Some(&run), "run.attempt_failed", 1).await;
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 1, resume).await;
    wait_for("attempt 3's session", || session(&dir, &run, 3).is_some()).await;
    assert_ne!(
        session(&dir, &run, 3),
        session(&dir, &first, 1),
        "the given-up session resumed"
    );
}

/// The step's prompt the daemon died under goes into the new session once, as its prompt.
#[tokio::test(start_paused = true)]
async fn a_new_session_after_a_daemon_crash_gets_the_step_prompt_once() {
    let mut w = world(POLICY);
    w.driver.lose_prompts(true);
    let run = w.create().await;
    wait_for("the prompt", || !w.driver.keys().is_empty()).await;
    w.restart().await;
    w.driver.lose_prompts(false);
    w.until(Some(&run), "run.attempt_failed", 0).await;
    w.decide(&run, "run.attempt_failed", 0, new_session()).await;
    wait_for("attempt 2's prompt", || w.driver.keys().len() == 2).await;
    assert_eq!(w.driver.sent(&run, 2).1, ["Build web.", "Build web."]);
}

/// A `continue` the daemon died under follows the step's prompt into the new session that replaces the old one.
#[tokio::test(start_paused = true)]
async fn a_new_session_after_a_daemon_crash_gets_the_pending_continue() {
    let plugin = format!(
        "{}[[hook]]\non = \"run.turn_ended_without_outcome\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n",
        POLICY[1].1
    );
    let mut w = world_with(&[POLICY[0], ("policy/plugin.toml", &plugin)], Arc::new(common::Lost));
    w.driver.script(hollow_turn());
    let run = w.create().await;
    w.until(Some(&run), "run.turn_ended_without_outcome", 0).await;
    w.restart().await;
    w.until(None, "daemon.started", 1).await;
    let go_on = Decision::Continue {
        message: "go on".into(),
    };
    w.decide(&run, "run.turn_ended_without_outcome", 0, go_on).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "daemon_crash");
    w.decide(&run, "run.attempt_failed", 0, new_session()).await;
    wait_for("attempt 2's prompt", || w.driver.keys().len() == 2).await;
    let (_, texts) = w.driver.sent(&run, 2);
    assert_eq!(
        texts.last().map(String::as_str),
        Some("Build web.\n\ngo on"),
        "{texts:?}"
    );
}

/// A writer that stopped while the session started never learnt of it: no prompt goes into that session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_prompt_goes_into_a_session_a_stopped_writer_never_recorded() {
    let w = world(&[]);
    let release = w.driver.hold_sessions();
    let run = w.create().await;
    wait_for("the start of the session", || !w.driver.workdir(&run, 1).is_null()).await;
    let killer = w.killer();
    killer.abort();
    wait_for("the writer's end", || killer.is_finished()).await;
    drop(release);
    // What the launch does after its session started, it does at once: there is no event of a prompt not sent.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(w.driver.keys().is_empty(), "a prompt was sent");
}

/// Whether some real process of the driver was alive when each hook on `run.attempt_failed` ran.
struct Probe {
    driver: ScriptedDriver,
    alive: Mutex<Vec<bool>>,
}

impl HookRunner for Probe {
    fn run<'a>(&'a self, _: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        if payload["event"]["type"] == "run.attempt_failed" {
            let spawned = self.driver.spawned();
            let alive = spawned
                .iter()
                .any(|(pid, start)| orphans::start_time(*pid) == Some(*start));
            self.alive.lock().unwrap().push(alive);
        }
        Box::pin(async {})
    }
}

/// A hook on a failed attempt reads what the attempt's process left only once that process is dead.
#[cfg(target_os = "linux")] // a process's start time is read from `/proc`
#[tokio::test]
async fn a_hook_on_a_failed_attempt_runs_after_its_process_died() {
    let driver = ScriptedDriver::new(1);
    driver.real_processes(Duration::from_millis(500));
    let probe = Arc::new(Probe {
        driver: driver.clone(),
        alive: Mutex::default(),
    });
    let w = world_on(POLICY, probe.clone(), driver);
    let overloaded = Failure::new(FailureKind::Overloaded, Source::Adapter);
    w.driver.script(vec![Step::Emit(Normalized::Failure(overloaded))]);
    w.create().await;
    wait_for("the hook", || !probe.alive.lock().unwrap().is_empty()).await;
    assert_eq!(w.driver.spawned().len(), 1);
    assert_eq!(
        *probe.alive.lock().unwrap(),
        [false],
        "the hook ran beside the live process"
    );
}

/// A hook runner that fails the run on each `run.attempt_failed` it is given.
struct Failer;

impl HookRunner for Failer {
    fn run<'a>(&'a self, core: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let event = &payload["event"];
            let msg = Msg::Decide {
                run: event["run"].as_str().unwrap().into(),
                seq: event["seq"].as_i64().unwrap(),
                decision: Decision::Fail {
                    reason: "policy".into(),
                    gate: None,
                },
            };
            // The writer of a daemon that died meanwhile drops it; the hook runs again after the restart.
            let _ = core.send(None, msg).await;
        })
    }
}

/// Whether the decision on a failed attempt whose process outlives the stop grace comes from its hook, the daemon
/// restarting (`restart`) while the hook waits for that process.
async fn a_stopped_process_leaves_the_decision_to_the_hook(restart: bool) {
    let plugin = POLICY[1].1.replace("10s", "1s");
    let driver = ScriptedDriver::new(1);
    // Past the stop grace: the hooks start on a live process.
    driver.real_processes(Duration::from_secs(20));
    let mut w = world_on(&[POLICY[0], ("policy/plugin.toml", &plugin)], Arc::new(Failer), driver);
    let overloaded = Failure::new(FailureKind::Overloaded, Source::Adapter);
    w.driver.script(vec![Step::Emit(Normalized::Failure(overloaded))]);
    let run = w.create().await;
    w.until(Some(&run), "run.attempt_failed", 0).await;
    let spawned = w.driver.spawned();
    if restart {
        w.restart().await;
    }
    assert_eq!(w.until(Some(&run), "run.failed", 0).await["reason"], "policy");
    if restart {
        // The stop of the process died with the old daemon, and nothing of the run is left to clean it up.
        for (pid, _) in &spawned {
            let kill = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .status();
            assert!(kill.unwrap().success());
        }
        return;
    }
    wait_for("the end of the process", || {
        spawned
            .iter()
            .all(|(pid, start)| orphans::start_time(*pid) != Some(*start))
    })
    .await;
}

/// The wait of a hook for a stopped process that will not die counts in the deadline of its decision: a hook within
/// its timeout still decides.
#[cfg(target_os = "linux")] // a process's start time is read from `/proc`
#[tokio::test(start_paused = true)]
async fn a_decision_on_a_failed_attempt_waits_for_the_stopped_process() {
    a_stopped_process_leaves_the_decision_to_the_hook(false).await;
}

/// The deadline a restart sets again counts the wait for the stopped process too.
#[cfg(target_os = "linux")] // a process's start time is read from `/proc`
#[tokio::test(start_paused = true)]
async fn a_decision_on_a_failed_attempt_waits_for_the_stopped_process_after_a_restart() {
    a_stopped_process_leaves_the_decision_to_the_hook(true).await;
}
