//! The finisher on the scripted driver (docs/design/contracts.md 6.3, spec §9.4; §15 criterion 4, core part).

mod common;

use aae_core::{
    driver::{Normalized, SessionState},
    run::{Decision, MAX_REMARKS, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{POLICY, World, hollow_turn, last, world};
use serde_json::{Value, json};
use std::time::Duration;

/// A world whose plugin has one hook on `on`: decisions on it wait for the test.
fn hooked(on: &str) -> World {
    let plugin = POLICY[1].1.replace("run.attempt_failed", on);
    world(&[POLICY[0], ("policy/plugin.toml", &plugin)])
}

/// A turn that ends 10 s after the prompt: time for the finisher.
fn slow_turn() -> Vec<Step> {
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(10)));
    steps
}

impl World {
    async fn finish(&self, run: &str, token: &str, result: Verdict, summary: &str) -> Value {
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

/// A run waiting on its finishing hooks when the daemon dies: hooks run again, a turn does not (contracts 6.4).
async fn finishing_across_a_restart() -> (World, String) {
    let mut w = hooked("run.finishing");
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(1)));
    w.driver.script(steps);
    let run = w.create().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let token = w.driver.token(&run, 1).unwrap();
    assert_eq!(w.finish(&run, &token, Verdict::Pass, "done").await, "accepted");
    tokio::time::sleep(Duration::from_millis(900)).await;
    w.restart().await;
    (w, run)
}

#[tokio::test(start_paused = true)]
async fn a_run_in_finishing_keeps_its_outcome_across_a_restart() {
    let (w, run) = finishing_across_a_restart().await;
    assert_eq!(w.states(&run).await.last().unwrap(), "finishing");
    let started = w.until(None, "daemon.started", 1).await;
    assert_eq!(started["interrupted_runs"], 0, "{started}");
    w.decide(&run, "run.finishing", 0, Decision::Accept).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "passed");
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.passed")["outcome"]["summary"], "done");
}

/// The remark goes to the session of the dead daemon's instance: the attempt fails at once, nothing waits.
#[tokio::test(start_paused = true)]
async fn a_remark_after_a_restart_fails_the_attempt_instead_of_waiting() {
    let (w, run) = finishing_across_a_restart().await;
    let message = "Add a test.".to_owned();
    w.decide(&run, "run.finishing", 0, Decision::Remark { message }).await;
    let states = w.states(&run).await;
    assert_eq!(
        states[states.len() - 4..],
        ["finishing", "running", "interrupted", "failed"],
        "{states:?}"
    );
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "harness_crash");
}
