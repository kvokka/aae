//! The `max_runs` of an agent at the step boundary (spec §9.3; docs/design/contracts.md 7), on the managed clock.

mod common;

use aae_core::{
    driver::{Normalized, SessionState, StopReason},
    run::{Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{World, hollow_turn, last, world};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

/// A turn that ends once the test notifies: it holds its run until then.
fn held_turn(world: &World) -> Arc<Notify> {
    let release = Arc::new(Notify::new());
    world.driver.script(vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Await(release.clone()),
        Step::Emit(Normalized::TurnEnded {
            stop_reason: StopReason::Completed,
        }),
        Step::Emit(Normalized::State {
            state: SessionState::Idle,
            background: false,
        }),
    ]);
    release
}

/// Declares `pass` for attempt `attempt` of `run`, then lets its turn end.
async fn pass(w: &World, run: &str, attempt: u32, turn: &Notify) {
    let token = loop {
        match w.driver.token(run, attempt) {
            Some(token) => break token,
            None => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };
    let outcome = Outcome {
        result: Verdict::Pass,
        summary: "done".into(),
        data: None,
    };
    let msg = Msg::Finish {
        run: run.into(),
        token,
        outcome,
    };
    assert_eq!(w.core.send(None, msg).await.unwrap()["answer"], "accepted");
    turn.notify_one();
}

/// Two rigs and the formula `build` of the ai steps `steps` (`(name, agent)`); `dev` runs one run at a time.
fn shop(steps: &[(&str, &str)]) -> World {
    let mut formula = "name = \"build\"\n".to_owned();
    for (name, agent) in steps {
        formula += &format!("[[step]]\nname = \"{name}\"\nkind = \"ai\"\nagent = \"{agent}\"\nprompt = \"build.md\"\n");
    }
    world(&[
        (
            "shop/aae.toml",
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        ),
        ("shop/api/.keep", ""),
        ("shop/agents/dev/agent.toml", "harness = \"scripted\"\nmax_runs = 1\n"),
        ("shop/agents/lead/agent.toml", "harness = \"scripted\"\n"),
        ("shop/formulas/build.toml", &formula),
    ])
}

/// Runs `first` into its `dev` step (held by the returned turn) and `second` up to its park behind it.
async fn second_parked(w: &World) -> (String, String, Arc<Notify>) {
    let (plan, work) = (held_turn(w), held_turn(w));
    let first = w.create_on("web").await;
    pass(w, &first, 1, &plan).await;
    // A script goes to the next prompt sent: `work` to the first run's second step, not to the second run.
    while w.driver.sent(&first, 2).1.len() < 2 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let plan = held_turn(w);
    let second = w.create_on("api").await;
    pass(w, &second, 1, &plan).await;
    let events = w.events(&second, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.parked")["then"], "new_session");
    assert_eq!(
        w.states(&second).await.last().unwrap(),
        "parked",
        "dev already runs `{first}`"
    );
    (first, second, work)
}

#[tokio::test(start_paused = true)]
async fn a_step_of_an_agent_at_its_limit_parks_until_a_slot_frees() {
    let w = shop(&[("plan", "lead"), ("work", "dev")]);
    let (_, second, work) = second_parked(&w).await;
    w.driver.script(hollow_turn());
    work.notify_one();
    let states = w.states(&second).await;
    assert_eq!(states[states.len() - 4..], ["parked", "starting", "running", "failed"]);
    let events = w.events(&second, Duration::ZERO).await;
    assert_eq!(last(&events, "run.resumed")["gate_key"], "concurrency");
}

#[tokio::test(start_paused = true)]
async fn a_run_leaving_the_step_of_an_agent_at_its_limit_resumes_a_parked_one() {
    let w = shop(&[("plan", "lead"), ("work", "dev"), ("review", "lead")]);
    let (first, second, work) = second_parked(&w).await;
    // Both next prompts hold: `review` of the first run, then `work` of the second.
    let _held = (held_turn(&w), held_turn(&w));
    pass(&w, &first, 2, &work).await;
    let states = w.states(&second).await;
    assert_eq!(states[states.len() - 3..], ["parked", "starting", "running"]);
    assert_eq!(
        w.states(&first).await.last().unwrap(),
        "running",
        "`{first}` reviews meanwhile"
    );
}
