//! The finisher on the scripted driver (docs/design/contracts.md 6.3; criterion 4, core part).

use crate::common;

use aae_core::{
    driver::{Normalized, SessionState},
    run::{Decision, HookRunner, MAX_REMARKS, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{Hanging, Lost, POLICY, World, hollow_turn, last, world, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// A world whose addon has one hook on `on`: decisions on it wait for the test.
fn hooked(on: &str) -> World {
    hooked_with(on, Arc::new(Lost))
}

fn hooked_with(on: &str, hooks: Arc<dyn HookRunner>) -> World {
    let addon = POLICY[1].1.replace("run.attempt_failed", on);
    world_with(&[POLICY[0], ("policy/addon.toml", &addon)], hooks)
}

/// A turn that ends 10 s after the prompt: time for the finisher.
fn slow_turn() -> Vec<Step> {
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(10)));
    steps
}

impl World {
    pub(crate) async fn finish(&self, run: &str, token: &str, result: Verdict, summary: &str) -> Value {
        let outcome = Outcome {
            result,
            summary: summary.into(),
            data: Some(json!({ "commit": "abc" })),
        };
        let msg = Msg::Finish {
            run: run.into(),
            token: token.into(),
            outcome,
        };
        self.core.send(None, msg).await.unwrap()["answer"].clone()
    }
}

#[tokio::test(start_paused = true)]
async fn the_last_outcome_declared_during_the_turn_passes_after_idle_without_background() {
    let w = world(&[]);
    let mut steps = slow_turn();
    let background = Normalized::State {
        state: SessionState::Idle,
        background: true,
    };
    steps.splice(3..3, [Step::Emit(background), Step::Wait(Duration::from_secs(60))]);
    w.driver.script(steps);
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(&run, 1).unwrap();
    assert_eq!(w.finish(&run, &token, Verdict::Fail, "first").await, "accepted");
    assert_eq!(w.finish(&run, &token, Verdict::Pass, "done").await, "accepted");
    assert_eq!(w.finish(&run, "forged", Verdict::Pass, "x").await, "invalid");
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert_eq!(
        w.states(&run).await.last().unwrap(),
        "running",
        "background work holds it"
    );
    tokio::time::sleep(Duration::from_secs(40)).await;
    assert_eq!(w.states(&run).await[3..], ["finishing", "passed"]);
    let events = w.events(&run, Duration::ZERO).await;
    let passed = last(&events, "run.passed");
    assert_eq!(passed["outcome"]["summary"], "done");
    assert_eq!(passed["outcome"]["data"]["commit"], "abc");
    assert_eq!(w.finish(&run, &token, Verdict::Pass, "again").await, "run_terminal");
}

#[tokio::test(start_paused = true)]
async fn a_token_of_an_earlier_attempt_is_stale() {
    let w = world(POLICY);
    w.driver.script(vec![Step::Exit {
        code: Some(1),
        signal: None,
    }]);
    let run = w.create().await;
    let after = Duration::ZERO;
    w.decide(
        &run,
        "run.attempt_failed",
        0,
        Decision::NewSession { after, gate: None },
    )
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (first, second) = (w.driver.token(&run, 1).unwrap(), w.driver.token(&run, 2).unwrap());
    assert_ne!(first, second);
    assert_eq!(w.finish(&run, &first, Verdict::Pass, "old").await, "stale_attempt");
    assert_eq!(w.finish(&run, &second, Verdict::Pass, "new").await, "accepted");
}

#[tokio::test(start_paused = true)]
async fn a_remark_sends_the_next_prompt_and_clears_the_outcome_the_token_stays_valid() {
    let w = hooked("run.finishing");
    w.driver.script(slow_turn());
    w.driver.script(slow_turn());
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(&run, 1).unwrap();
    w.finish(&run, &token, Verdict::Pass, "done").await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let message = "Add a test.".to_owned();
    w.decide(&run, "run.finishing", 0, Decision::Remark { message }).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.finish(&run, &token, Verdict::Pass, "with a test").await, "accepted");
    tokio::time::sleep(Duration::from_secs(10)).await;
    w.decide(&run, "run.finishing", 1, Decision::Accept).await;
    assert_eq!(
        w.states(&run).await[3..],
        ["finishing", "running", "finishing", "passed"]
    );
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.finishing")["remarks"], 1);
    assert_eq!(last(&events, "run.passed")["outcome"]["summary"], "with a test");
}

#[tokio::test(start_paused = true)]
async fn a_veto_fails_a_pass_with_class_check_and_a_fail_with_class_agent() {
    for (result, class, reason) in [(Verdict::Pass, "check", "no tests"), (Verdict::Fail, "agent", "stuck")] {
        let w = hooked("run.finishing");
        w.driver.script(slow_turn());
        let run = w.create().await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let token = w.driver.token(&run, 1).unwrap();
        w.finish(&run, &token, result, "stuck").await;
        tokio::time::sleep(Duration::from_secs(10)).await;
        let veto = Decision::Veto {
            reason: "no tests".into(),
        };
        w.decide(&run, "run.finishing", 0, veto).await;
        let events = w.events(&run, Duration::from_millis(200)).await;
        let failed = last(&events, "run.failed");
        assert_eq!((&failed["class"], &failed["reason"]), (&json!(class), &json!(reason)));
        assert_eq!(failed["outcome"]["summary"], "stuck");
    }
}

#[tokio::test(start_paused = true)]
async fn continue_after_a_hollow_turn_counts_toward_the_remarks_ceiling() {
    let w = hooked("run.turn_ended_without_outcome");
    w.driver.script(hollow_turn());
    let run = w.create().await;
    w.seq_of(&run, "run.turn_ended_without_outcome", 0).await;
    let token = w.driver.token(&run, 1).unwrap();
    assert_eq!(
        w.finish(&run, &token, Verdict::Pass, "late").await,
        "invalid",
        "the turn is over"
    );
    for n in 0..=MAX_REMARKS as usize {
        w.driver.script(hollow_turn());
        let message = "Keep going.".to_owned();
        w.decide(
            &run,
            "run.turn_ended_without_outcome",
            n,
            Decision::Continue { message },
        )
        .await;
    }
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.turn_ended_without_outcome")["remarks"], MAX_REMARKS);
    let failed = last(&events, "run.failed");
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&json!("hollow_done"), &json!("remarks_ceiling"))
    );
}

/// A run waiting on its finishing hooks when the daemon dies, down for `idle`: it goes on waiting for the same
/// decision, its hooks run again (docs/design/contracts.md 3; contracts 6.4).
async fn finishing_across_a_restart(hooks: Arc<dyn HookRunner>, idle: Duration) -> (World, String) {
    let mut w = hooked_with("run.finishing", hooks);
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(1)));
    w.driver.script(steps);
    let run = w.create().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let token = w.driver.token(&run, 1).unwrap();
    assert_eq!(w.finish(&run, &token, Verdict::Pass, "done").await, "accepted");
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "finishing");
    w.restart_after(idle).await;
    (w, run)
}

/// Down for longer than the hook's 10 s and the slack: the decision still gets its full time from the new start.
#[tokio::test(start_paused = true)]
async fn a_run_in_finishing_waits_across_a_restart_and_its_hooks_run_again_with_their_full_time() {
    let hooks = Arc::new(Hanging::default());
    let (w, run) = finishing_across_a_restart(hooks.clone(), Duration::from_secs(60)).await;
    let started = w.until(None, "daemon.started", 1).await;
    assert_eq!(started["interrupted_runs"], 0, "{started}");
    tokio::time::sleep(Duration::from_secs(14)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "finishing");
    assert_eq!(*hooks.0.lock().unwrap(), ["run.finishing", "run.finishing"]);
    // A hook that never answers is a veto once its deadline passes (contracts 3.3).
    tokio::time::sleep(Duration::from_secs(1)).await;
    let events = w.events(&run, Duration::ZERO).await;
    let failed = last(&events, "run.failed");
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&json!("check"), &json!("hook_deadline"))
    );
}

/// The decision after the restart judges the outcome declared before it: one attempt, nothing played again.
#[tokio::test(start_paused = true)]
async fn an_accept_after_a_restart_passes_the_outcome_declared_before_it() {
    let (w, run) = finishing_across_a_restart(Arc::new(Lost), Duration::ZERO).await;
    w.decide(&run, "run.finishing", 0, Decision::Accept).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.passed")["outcome"]["summary"], "done");
    assert!(events.iter().all(|(t, _)| t != "run.attempt_failed"), "{events:?}");
    assert_eq!(w.driver.keys().len(), 1, "one prompt");
}

/// A remark needs the session that took the turn, gone with the dead daemon: the attempt fails as `daemon_crash`
/// for the policy, and no prompt goes to the dead session (docs/design/invariants.md).
#[tokio::test(start_paused = true)]
async fn a_remark_after_a_restart_fails_the_attempt_with_daemon_crash() {
    let (w, run) = finishing_across_a_restart(Arc::new(Lost), Duration::ZERO).await;
    let remark = Decision::Remark {
        message: "again".into(),
    };
    w.decide(&run, "run.finishing", 0, remark).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let failed: Vec<&Value> = events
        .iter()
        .filter(|(t, _)| t == "run.attempt_failed")
        .map(|(_, d)| d)
        .collect();
    assert_eq!(failed.len(), 1, "{events:?}");
    assert_eq!(failed[0]["failure"]["kind"], "daemon_crash");
    assert_eq!(w.driver.keys().len(), 1, "no prompt to the dead session");
}
