//! Which ends of a run count as a failed run of its bead (docs/design/contracts.md 6.2): any `failed` end of a run
//! that held the claim, whatever its class; a round back, a repeat of a step, an attempt the policy retries and a
//! cancelled run count none, and a passed run resets the count. With `bead_failed_runs = 1` a failed run is a closed
//! gate of the bead.

use crate::common;

use common::{
    POLICY, World,
    beads::{FIX, slow_turn},
    hollow_turn, world,
};
use loop_manager_core::{
    driver::{Failure, FailureKind, Normalized, Source},
    run::{Decision, Msg, Verdict},
};
use loop_manager_testkit::Step;
use serde_json::{Value, json};
use std::time::Duration;

/// The formula `rounds`: `code`, then `review`, a check that sends the work back to `code`, for 2 rounds.
const ROUNDS: (&str, &str) = (
    "shop/formulas/rounds.toml",
    "name = \"rounds\"\nrounds = 2\n[inputs]\nbead = { type = \"task\", required = true }\n\
     [[step]]\nname = \"code\"\nkind = \"ai\"\nagent = \"fixer\"\n\
     [[step]]\nname = \"review\"\nkind = \"ai\"\nagent = \"fixer\"\nback_to = \"code\"\n",
);

/// The formula `twice`: one ai step that runs at most twice.
const TWICE: (&str, &str) = (
    "shop/formulas/twice.toml",
    "name = \"twice\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
     [[step]]\nkind = \"ai\"\nagent = \"fixer\"\nmax_attempts = 2\n",
);

/// The formula `checked`: one ai step whose check script says 75, a failure for good.
const CHECKED: &[(&str, &str)] = &[
    (
        "shop/formulas/checked.toml",
        "name = \"checked\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
         [[step]]\nkind = \"ai\"\nagent = \"fixer\"\nmax_attempts = 3\nstep_check_script_path = \"check.sh\"\n",
    ),
    ("shop/formulas/check.sh", "#!/bin/sh\nexit 75\n"),
];

/// The world of `fix` with the policy addon, whose core closes a bead's gate at `failed_runs` failed runs.
fn world_at(failed_runs: u32, extra: &[(&str, &str)]) -> World {
    let config = POLICY[0]
        .1
        .replacen('\n', &format!("\nbead_failed_runs = {failed_runs}\n"), 1);
    world(&[FIX, &POLICY[1..], &[("config.toml", config.as_str())], extra].concat())
}

/// Whether the core closed the gate of `bead`.
fn gated(w: &World, bead: &str) -> bool {
    let key = format!("bead:shop/{bead}");
    w.all("gate.closed").iter().any(|c| c["gate"]["key"] == key.as_str())
}

/// The data of `run.failed` of `run`, once a slow turn (10 s) and the deadline of the hooks of a failed attempt (25 s)
/// have passed.
async fn failure(w: &World, run: &str) -> Value {
    tokio::time::sleep(Duration::from_secs(30)).await;
    let failed = w.all("run.failed").into_iter().find(|d| d["run"]["id"] == run);
    failed.unwrap_or_else(|| panic!("`{run}` did not fail"))
}

impl World {
    /// Waits for the session of attempt `attempt` of `run`.
    async fn started(&self, run: &str, attempt: u32) {
        for _ in 0..600 {
            if self.driver.token(run, attempt).is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("no attempt {attempt} of `{run}`");
    }

    /// Attempt `attempt` of `run` declares `result` within its slow turn.
    async fn declared(&self, run: &str, attempt: u32, result: Verdict) {
        self.started(run, attempt).await;
        self.finish_attempt(run, attempt, result, "still red").await;
    }
}

#[tokio::test(start_paused = true)]
async fn a_run_out_of_rounds_is_one_failed_run_and_a_round_back_is_none() {
    let w = world_at(1, &[ROUNDS]);
    let bead = w.tracker.add(&[], None);
    for _ in 0..4 {
        w.driver.script(slow_turn());
    }
    let reply = w.start_bead("rounds", Some("web"), json!(bead)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    w.declared(&run, 1, Verdict::Pass).await;
    w.declared(&run, 2, Verdict::Fail).await;
    w.started(&run, 3).await;
    assert!(!gated(&w, &bead), "the review sent the work back: the run goes on");
    w.declared(&run, 3, Verdict::Pass).await;
    w.declared(&run, 4, Verdict::Fail).await;
    let failed = failure(&w, &run).await;
    assert_eq!(failed["class"], "agent");
    let reason = failed["reason"].as_str().unwrap();
    assert!(reason.starts_with("still failing after 2 rounds"), "{reason}");
    assert_eq!(w.all("gate.closed").len(), 1, "one failed run");
    assert!(gated(&w, &bead));
}

#[tokio::test(start_paused = true)]
async fn a_step_out_of_attempts_is_one_failed_run_and_a_repeat_is_none() {
    let w = world_at(1, &[TWICE]);
    let bead = w.tracker.add(&[], None);
    w.driver.script(slow_turn());
    w.driver.script(slow_turn());
    let reply = w.start_bead("twice", Some("web"), json!(bead)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    w.declared(&run, 1, Verdict::Fail).await;
    w.started(&run, 2).await;
    assert!(!gated(&w, &bead), "the step runs again: the run goes on");
    w.declared(&run, 2, Verdict::Fail).await;
    assert_eq!(failure(&w, &run).await["class"], "agent");
    assert!(gated(&w, &bead));
}

/// The check script is a real process, so real time.
#[tokio::test]
async fn exit_75_of_a_check_script_is_a_failed_run_of_the_bead() {
    let w = world_at(1, CHECKED);
    let bead = w.tracker.add(&[], None);
    let turn = w.repeat_turn();
    let reply = w.start_bead("checked", Some("web"), json!(bead)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    w.repeat_declare(&run, 1, Verdict::Pass, &turn).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), data["class"].as_str(), data["reason"].as_str()),
        ("run.failed", Some("check"), Some("check script: exit 75")),
        "{data}"
    );
    assert!(gated(&w, &bead));
}

#[tokio::test(start_paused = true)]
async fn an_attempt_the_policy_retries_is_no_failed_run_and_the_run_it_gives_up_is_one() {
    let w = world_at(1, &[]);
    let bead = w.tracker.add(&[], None);
    for _ in 0..2 {
        let crash = Failure::new(FailureKind::HarnessCrash, Source::Adapter);
        w.driver.script(vec![
            Step::Emit(Normalized::TurnStarted),
            Step::Emit(Normalized::Failure(crash)),
        ]);
    }
    let run = w.run_on(json!(bead)).await;
    let again = Decision::NewSession {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, again).await;
    w.seq_of(&run, "run.attempt_failed", 1).await;
    assert!(!gated(&w, &bead), "the policy tries again: the run goes on");
    // No decision on the second failure: at the hook's deadline the run is given up.
    let failed = failure(&w, &run).await;
    assert_eq!(
        (failed["class"].as_str(), failed["reason"].as_str()),
        (Some("infrastructure"), Some("no_decision"))
    );
    assert!(gated(&w, &bead));
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_run_releases_its_bead_without_counting_a_failure() {
    let w = world_at(1, &[]);
    let bead = w.tracker.add(&[], None);
    w.driver.script(slow_turn());
    let run = w.run_on(json!(bead)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.assignee(&bead), Some(format!("loop-manager:{run}")));
    let cancel = Msg::Cancel {
        run: run.clone(),
        reason: "operator".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "cancelled");
    let released = w.tracker.bead(&bead).unwrap();
    assert_eq!((released.assignee, released.status.as_str()), (None, "open"));
    assert!(w.all("gate.closed").is_empty(), "{:?}", w.all("gate.closed"));
}

/// The claim answered with a bead of another rig: the run never worked on it, so its end is no failed run of it.
#[tokio::test(start_paused = true)]
async fn a_claimed_bead_of_another_rig_is_no_failed_run_of_it() {
    let w = world_at(1, &[]);
    let bead = w.tracker.add_in("w", &[], None);
    let reply = w.fix_on(Some("root"), json!(bead)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    assert_eq!(failure(&w, &run).await["reason"], "bead_of_another_rig");
    assert_eq!(w.assignee(&bead), None, "the claim went back");
    assert!(!gated(&w, &bead));
}

#[tokio::test(start_paused = true)]
async fn a_passed_run_resets_the_failed_runs_of_its_bead() {
    let w = world_at(2, &[]);
    let bead = w.tracker.add(&[], None);
    w.driver.script(hollow_turn());
    let run = w.run_on(json!(bead)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "failed");
    // With a child open, closing the bead releases it instead: the bead stays to be run again.
    w.tracker.add(&[], Some(&bead));
    w.driver.script(slow_turn());
    let run = w.run_on(json!(bead)).await;
    w.declared(&run, 1, Verdict::Pass).await;
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "passed");
    for gate_closed in [false, true] {
        w.driver.script(hollow_turn());
        let run = w.run_on(json!(bead)).await;
        assert_eq!(w.states(&run).await.last().unwrap(), "failed");
        assert_eq!(gated(&w, &bead), gate_closed, "the pass reset the count");
    }
}
