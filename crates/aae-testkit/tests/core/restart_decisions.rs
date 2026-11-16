//! A daemon restart while a run waits for a decision on a deciding event (spec §5.6, §11.3; docs/design/contracts.md
//! 6.4): the run keeps waiting, its hooks run again, and the time the daemon was down is not theirs. A drain waits
//! for those decisions and starts no new turn.

use crate::common;

use aae_core::{
    driver::{Failure, FailureKind, Normalized, Source},
    run::{Create, Decision, GateSpec, HookRunner, MAX_ATTEMPTS, Msg, Reader},
};
use aae_testkit::Step;
use common::{Hanging, Lost, POLICY, World, beads::FIX, hollow_turn, last, world, world_with};
use serde_json::json;
use std::{sync::Arc, time::Duration};

/// Longer than any hook here (10 s) with its slack (5 s): a deadline counted from before the restart has passed.
const IDLE: Duration = Duration::from_secs(60);

/// A world whose plugin has one hook on `on`, 10 s long.
fn hooked(on: &str, hooks: Arc<dyn HookRunner>) -> World {
    hooked_in(on, hooks, &[])
}

fn hooked_in(on: &str, hooks: Arc<dyn HookRunner>, extra: &[(&str, &str)]) -> World {
    let plugin = POLICY[1].1.replace("run.attempt_failed", on);
    let mut files = vec![POLICY[0], ("policy/plugin.toml", plugin.as_str())];
    files.extend_from_slice(extra);
    world_with(&files, hooks)
}

impl World {
    async fn last_state(&self, run: &str) -> String {
        self.states(run).await.pop().unwrap()
    }

    async fn failed(&self, run: &str) -> (String, String) {
        let events = self.events(run, Duration::ZERO).await;
        let failed = last(&events, "run.failed");
        (failed["class"].to_string(), failed["reason"].to_string())
    }
}

/// The hooks of `run` still decide 14 s after the restart; at 15 s their deadline passes.
async fn waits_its_full_time(w: &World, run: &str, hooks: &Hanging, state: &str, kind: &str) {
    tokio::time::sleep(Duration::from_secs(14)).await;
    assert_eq!(w.last_state(run).await, state);
    assert_eq!(*hooks.0.lock().unwrap(), [kind, kind], "the hooks ran again");
    tokio::time::sleep(Duration::from_secs(1)).await;
}

#[tokio::test(start_paused = true)]
async fn a_preparing_run_waits_for_its_hooks_after_a_long_restart() {
    let hooks = Arc::new(Hanging::default());
    let mut w = hooked("run.preparing", hooks.clone());
    let run = w.create().await;
    w.until(Some(&run), "run.preparing", 0).await;
    w.restart_after(IDLE).await;
    waits_its_full_time(&w, &run, &hooks, "preparing", "run.preparing").await;
    assert_eq!(w.failed(&run).await, ("\"check\"".into(), "\"hook_deadline\"".into()));
}

#[tokio::test(start_paused = true)]
async fn an_interrupted_run_waits_for_the_policy_after_a_long_restart() {
    let hooks = Arc::new(Hanging::default());
    let mut w = hooked("run.attempt_failed", hooks.clone());
    w.driver.script(vec![Step::Exit {
        code: None,
        signal: Some(9),
    }]);
    let run = w.create().await;
    w.until(Some(&run), "run.attempt_failed", 0).await;
    w.restart_after(IDLE).await;
    tokio::time::sleep(Duration::from_secs(14)).await;
    assert_eq!(w.last_state(&run).await, "interrupted");
    assert_eq!(*hooks.0.lock().unwrap(), ["run.attempt_failed", "run.attempt_failed"]);
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, resume).await;
    let states = w.states(&run).await;
    assert_eq!(states.iter().filter(|s| *s == "starting").count(), 2, "{states:?}");
}

/// The claim of the bead is retried after the restart: a tracker that answers is not taken for one that is down.
#[tokio::test(start_paused = true)]
async fn a_claim_waits_for_the_tracker_after_a_long_restart() {
    let mut w = world(FIX);
    let bead = w.tracker.add(&[], None);
    w.changed().await;
    w.tracker.set_down(true);
    w.driver.script(hollow_turn());
    let run = w.fix(json!(bead)).await.unwrap()["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(w.last_state(&run).await, "preparing");
    w.tracker.set_down(false);
    // Past the 120 s the claim had.
    w.restart_after(Duration::from_secs(300)).await;
    let states = w.events(&run, Duration::from_secs(5)).await;
    let states: Vec<_> = states
        .iter()
        .filter(|(t, _)| t == "run.state_changed")
        .map(|(_, d)| &d["to"])
        .collect();
    assert_eq!(states, ["preparing", "starting", "running"]);
}

/// A run whose turn ended without an outcome, waiting on its hook when the daemon dies, down for `idle`. (With a
/// lost runner the hook is done and only its deadline is left: no wait past it.)
async fn hollow_across_a_restart(hooks: Arc<dyn HookRunner>, idle: Duration) -> (World, String) {
    let mut w = hooked("run.turn_ended_without_outcome", hooks);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    w.until(Some(&run), "run.turn_ended_without_outcome", 0).await;
    w.restart_after(idle).await;
    let started = w.until(None, "daemon.started", 1).await;
    assert_eq!(started["interrupted_runs"], 0, "{started}");
    (w, run)
}

#[tokio::test(start_paused = true)]
async fn a_hollow_turn_waits_across_a_restart_and_no_decision_fails_it_as_hollow() {
    let hooks = Arc::new(Hanging::default());
    let (w, run) = hollow_across_a_restart(hooks.clone(), IDLE).await;
    waits_its_full_time(&w, &run, &hooks, "running", "run.turn_ended_without_outcome").await;
    assert_eq!(
        w.failed(&run).await,
        ("\"hollow_done\"".into(), "\"no_decision\"".into())
    );
}

#[tokio::test(start_paused = true)]
async fn a_fail_after_a_restart_fails_a_hollow_turn_as_hollow() {
    let (w, run) = hollow_across_a_restart(Arc::new(Lost), Duration::ZERO).await;
    let fail = Decision::Fail {
        reason: "nothing done".into(),
        gate: None,
    };
    w.decide(&run, "run.turn_ended_without_outcome", 0, fail).await;
    assert_eq!(
        w.failed(&run).await,
        ("\"hollow_done\"".into(), "\"nothing done\"".into())
    );
}

/// `continue` needs the session that took the turn: the attempt fails as `daemon_crash`, and the message waits as
/// the prompt the resumed session gets.
#[tokio::test(start_paused = true)]
async fn a_continue_after_a_restart_fails_the_attempt_with_daemon_crash() {
    let (w, run) = hollow_across_a_restart(Arc::new(Lost), Duration::ZERO).await;
    let go_on = Decision::Continue {
        message: "go on".into(),
    };
    w.decide(&run, "run.turn_ended_without_outcome", 0, go_on).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "daemon_crash");
    assert_eq!(w.driver.keys().len(), 1, "no prompt to the dead session");
}

/// An exec step whose command ended before the restart: the outcome stands, the command does not run again, its
/// finishing hook does.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_exec_step_in_finishing_passes_after_a_restart_with_its_command_run_once() {
    let formula = "name = \"check\"\n[[step]]\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", \"echo ran >> runs.log\"]\n";
    let hooks = Arc::new(Hanging::default());
    let mut w = hooked_in("run.finishing", hooks.clone(), &[("shop/formulas/check.toml", formula)]);
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "check".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    w.until(Some(&run), "run.finishing", 0).await;
    w.restart().await;
    w.decide(&run, "run.finishing", 0, Decision::Accept).await;
    let passed = w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(passed["outcome"]["result"], "pass", "{passed}");
    let log = std::fs::read_to_string(w.dir.path().join("shop/web/runs.log")).unwrap();
    assert_eq!(log, "ran\n", "the command ran once");
    assert_eq!(*hooks.0.lock().unwrap(), ["run.finishing", "run.finishing"]);
}

/// Spec §2, decisions 49, 50 takes only the wait on a closed account gate out of the ceiling: a park on the account
/// whose `until` has passed waits for nothing, so its attempts count and the run ends (invariant 16).
#[tokio::test(start_paused = true)]
async fn parks_on_an_account_gate_that_does_not_close_end_at_the_attempts_ceiling() {
    let agent = "harness = \"scripted\"\n[native.scripted]\naccount = \"a\"\n";
    let w = world(&[POLICY[0], POLICY[1], ("shop/agents/dev/agent.toml", agent)]);
    let forbidden = || {
        Step::Emit(Normalized::Failure(Failure::new(
            FailureKind::AuthRequired,
            Source::Adapter,
        )))
    };
    for _ in 0..=MAX_ATTEMPTS {
        w.driver.script(vec![forbidden()]);
    }
    let run = w.create().await;
    for n in 0..MAX_ATTEMPTS {
        let park = Decision::Park {
            gate: GateSpec {
                key: "account:scripted/a".into(),
                until_ms: Some(1),
                reason: "auth".into(),
            },
            then: "new_session".into(),
        };
        w.until(Some(&run), "run.attempt_failed", n as usize).await;
        w.decide(&run, "run.attempt_failed", n as usize, park).await;
    }
    let (class, reason) = w.failed(&run).await;
    assert_eq!(
        (class.as_str(), reason.as_str()),
        ("\"infrastructure\"", "\"attempts_ceiling\"")
    );
}

impl World {
    fn active_runs(&self) -> i64 {
        let status = Reader::open(&self.dir.path().join("state.db"))
            .unwrap()
            .status()
            .unwrap();
        status["active_runs"].as_i64().unwrap()
    }

    async fn drain(&self) {
        let stopping = Msg::Stopping { reason: "drain".into() };
        self.core.send(None, stopping).await.unwrap();
    }
}

/// A drain waits for the policy's decision on an attempt that failed during it; the retry it decides parks.
#[tokio::test(start_paused = true)]
async fn a_drain_waits_for_the_decision_on_a_failed_attempt() {
    let w = world(POLICY);
    w.driver.script(vec![Step::Exit {
        code: None,
        signal: Some(9),
    }]);
    let run = w.create().await;
    w.until(Some(&run), "run.attempt_failed", 0).await;
    w.drain().await;
    assert_eq!(w.active_runs(), 1, "the decision is pending");
    let retry = Decision::NewSession {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, retry).await;
    w.until(Some(&run), "run.parked", 0).await;
    assert_eq!(w.active_runs(), 0);
}

/// A remark at the turn boundary of a drain starts no turn: the attempt ends as `daemon_crash`, its retry parks, and
/// the next daemon resumes the session with the remark.
#[tokio::test(start_paused = true)]
async fn a_remark_during_a_drain_waits_for_the_next_daemon() {
    let plugin = format!(
        "{}[[hook]]\non = \"run.finishing\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n",
        POLICY[1].1
    );
    let mut w = world_with(&[POLICY[0], ("policy/plugin.toml", &plugin)], Arc::new(Lost));
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(1)));
    w.driver.script(steps);
    let run = w.create().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let token = w.driver.token(&run, 1).unwrap();
    let outcome = aae_core::run::Outcome {
        result: aae_core::run::Verdict::Pass,
        summary: "done".into(),
        data: None,
    };
    let finish = Msg::Finish {
        run: run.clone(),
        token,
        outcome,
    };
    assert_eq!(w.core.send(None, finish).await.unwrap()["answer"], "accepted");
    w.until(Some(&run), "run.finishing", 0).await;
    w.drain().await;
    let remark = Decision::Remark {
        message: "check the tests".into(),
    };
    w.decide(&run, "run.finishing", 0, remark).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "daemon_crash");
    assert_eq!(w.driver.keys().len(), 1, "no new turn");
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, resume).await;
    w.until(Some(&run), "run.parked", 0).await;
    assert_eq!(w.active_runs(), 0);

    w.restart().await;
    w.until(Some(&run), "run.resumed", 0).await;
    w.events(&run, Duration::from_millis(300)).await;
    let (_, texts) = w.driver.sent(&run, 2);
    assert_eq!(texts.last().map(String::as_str), Some("check the tests"), "{texts:?}");
}
