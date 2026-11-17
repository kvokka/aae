//! Background work after the outcome (docs/design/contracts.md 6, criterion 34, core part): once the outcome is
//! declared and the turn has ended, the commands the session left are stopped and the step ends; without an outcome, or
//! with background the harness cannot stop (a subagent, criterion 4a), the background is waited for.

use crate::common;

use common::{POLICY, World, last, world};
use loop_manager_core::{
    driver::{Normalized, SessionState, StopReason},
    run::Verdict,
};
use loop_manager_testkit::Step;
use std::time::Duration;

fn idle(background: bool) -> Step {
    Step::Emit(Normalized::State {
        state: SessionState::Idle,
        background,
    })
}

fn ended() -> Step {
    Step::Emit(Normalized::TurnEnded {
        stop_reason: StopReason::Completed,
    })
}

/// A turn of 10 s that leaves a command running: the session is idle with background, and stays so.
fn turn_leaving_a_command() -> Vec<Step> {
    let started = Step::Emit(Normalized::TurnStarted);
    vec![started, Step::Wait(Duration::from_secs(10)), ended(), idle(true)]
}

/// Creates a run and declares `pass` with its token at the 1st second.
async fn declared(w: &World) -> String {
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(&run, 1).unwrap();
    assert_eq!(w.finish(&run, &token, Verdict::Pass, "done").await, "accepted");
    run
}

#[tokio::test(start_paused = true)]
async fn a_declared_outcome_stops_the_commands_left_in_the_background() {
    let w = world(&[]);
    w.driver.stoppable_background();
    w.driver.script(turn_leaving_a_command());
    let run = declared(&w).await;
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert_eq!(w.states(&run).await[3..], ["finishing", "passed"]);
    assert_eq!(w.driver.background_stops(), 1);
}

#[tokio::test(start_paused = true)]
async fn without_an_outcome_the_background_is_waited_for_and_a_late_outcome_stops_it() {
    let w = world(&[]);
    w.driver.stoppable_background();
    w.driver.script(turn_leaving_a_command());
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(
        w.states(&run).await.last().unwrap(),
        "running",
        "the background holds it"
    );
    assert_eq!(w.driver.background_stops(), 0);
    // A background command declares the outcome after the turn: what is left goes then.
    let token = w.driver.token(&run, 1).unwrap();
    assert_eq!(w.finish(&run, &token, Verdict::Pass, "late").await, "accepted");
    assert_eq!(w.states(&run).await[3..], ["finishing", "passed"]);
    assert_eq!(w.driver.background_stops(), 1);
}

#[tokio::test(start_paused = true)]
async fn idle_with_background_inside_a_turn_stops_nothing() {
    let w = world(&[]);
    w.driver.stoppable_background();
    let started = Step::Emit(Normalized::TurnStarted);
    let wait = || Step::Wait(Duration::from_secs(10));
    w.driver
        .script(vec![started, wait(), idle(true), wait(), ended(), idle(false)]);
    let run = declared(&w).await;
    tokio::time::sleep(Duration::from_secs(14)).await;
    assert_eq!(w.driver.background_stops(), 0, "the turn has not ended");
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(w.states(&run).await[3..], ["finishing", "passed"]);
}

#[tokio::test(start_paused = true)]
async fn a_restart_while_the_background_stops_keeps_the_declared_outcome() {
    let mut w = world(POLICY);
    // Background the harness does not stop: the run is still `running` when the daemon dies.
    w.driver.script(turn_leaving_a_command());
    let run = declared(&w).await;
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert_eq!(w.driver.background_stops(), 1);
    w.restart().await;
    // The restart killed the instance, which is the stop the run waited for (docs/design/contracts.md 6,
    // docs/design/invariants.md).
    let events = w.events(&run, Duration::from_millis(300)).await;
    assert!(!events.iter().any(|(t, _)| t == "run.attempt_failed"));
    assert_eq!(last(&events, "run.finishing")["outcome"]["summary"], "done");
    assert_eq!(w.states(&run).await[3..], ["finishing", "passed"]);
}
