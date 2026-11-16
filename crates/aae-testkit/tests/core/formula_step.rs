//! The formula step (spec §9.1, §9.3, criterion 31, core part): the step is a child run of another formula with the
//! inputs of its parent it declares and those the step gives; its outcome is the step's; an internal formula runs
//! only so.

use crate::common;

use aae_core::{
    Error,
    driver::{Normalized, SessionState, StopReason},
    run::{Create, Decision, Msg, Verdict},
};
use aae_testkit::Step;
use common::{POLICY, World, last, world};
use serde_json::{Value, json};
use std::time::Duration;

const CHILD: (&str, &str) = (
    "shop/formulas/child.toml",
    "name = \"child\"\ninternal = true\n[inputs]\ngoal = { type = \"string\", required = true }\n\
     note = { type = \"string\" }\n[[step]]\nkind = \"ai\"\nagent = \"dev\"\n",
);

const SHIP: (&str, &str) = (
    "shop/formulas/ship.toml",
    "name = \"ship\"\n[inputs]\ngoal = { type = \"string\", required = true }\nextra = { type = \"string\" }\n\
     [[step]]\nname = \"sub\"\nkind = \"formula\"\nformula = \"child\"\ninputs = { note = \"for {{ inputs.goal }}\" }\n",
);

/// A world with `ship` over the internal `child`, whose session takes a turn of 10 s; `extra` files besides.
fn shipyard_with(extra: &[(&'static str, &'static str)]) -> World {
    let files: Vec<_> = extra.iter().copied().chain([CHILD, SHIP]).collect();
    let w = world(&files);
    w.driver.script(turn());
    w
}

fn turn() -> Vec<Step> {
    vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Wait(Duration::from_secs(10)),
        Step::Emit(Normalized::TurnEnded {
            stop_reason: StopReason::Completed,
        }),
        Step::Emit(Normalized::State {
            state: SessionState::Idle,
            background: false,
        }),
    ]
}

fn shipyard() -> World {
    shipyard_with(&[])
}

fn start(formula: &str) -> Msg {
    let inputs = json!({ "goal": "g", "extra": "x" });
    let inputs = if formula == "ship" {
        inputs
    } else {
        json!({ "goal": "g" })
    };
    start_with(formula, inputs)
}

fn start_with(formula: &str, inputs: Value) -> Msg {
    Msg::Create(Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: formula.into(),
        inputs: inputs.as_object().cloned().unwrap(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    })
}

/// Starts `ship` and returns it with the `run` view of its child.
async fn ship(w: &World) -> (String, Value) {
    ship_with(w, start("ship")).await
}

/// Starts the parent `create` and returns it with the `run` view of its first child.
async fn ship_with(w: &World, create: Msg) -> (String, Value) {
    let reply = w.core.send(None, create).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    let child = w.until(None, "run.queued", 1).await;
    (run, child["run"].clone())
}

/// A formula `name` with `goal` and `note` whose one step runs `child` with `step` keys besides.
fn parent(path: &'static str, name: &str, step: &str) -> (&'static str, &'static str) {
    let text = format!(
        "name = \"{name}\"\n[inputs]\ngoal = {{ type = \"string\", required = true }}\nnote = {{ type = \"string\" }}\n\
         [[step]]\nkind = \"formula\"\nformula = \"child\"\n{step}"
    );
    (path, Box::leak(text.into_boxed_str()))
}

#[tokio::test(start_paused = true)]
async fn the_step_runs_the_formula_as_a_child_and_takes_its_outcome() {
    let w = shipyard();
    let (run, child) = ship(&w).await;
    assert_eq!((&child["parent_run"], &child["depth"]), (&json!(run), &json!(1)));
    let kid = child["id"].as_str().unwrap();
    // Inherited only what `child` declares, the step's own besides.
    let reader = aae_core::run::Reader::open(&w.dir.path().join("state.db")).unwrap();
    let view = reader.run(kid).unwrap().unwrap();
    assert_eq!(view["inputs"], json!({ "goal": "g", "note": "for g" }));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(kid, 1).unwrap();
    assert_eq!(w.finish(kid, &token, Verdict::Pass, "child done").await, "accepted");
    tokio::time::sleep(Duration::from_secs(11)).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let passed = last(&events, "run.passed");
    assert_eq!(passed["outcome"]["summary"], "child done");
    assert_eq!(passed["outcome"]["data"], json!({ "commit": "abc" }));
}

#[tokio::test(start_paused = true)]
async fn a_failed_child_fails_the_step_and_its_run() {
    let w = shipyard();
    let (run, child) = ship(&w).await;
    let kid = child["id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(kid, 1).unwrap();
    assert_eq!(w.finish(kid, &token, Verdict::Fail, "no luck").await, "accepted");
    tokio::time::sleep(Duration::from_secs(11)).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.failed")["outcome"]["summary"], "no luck");
}

#[tokio::test(start_paused = true)]
async fn cancelling_the_parent_cancels_its_child() {
    let w = shipyard();
    let (run, child) = ship(&w).await;
    let kid = child["id"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let cancel = Msg::Cancel {
        run: run.clone(),
        reason: "operator".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    assert_eq!(w.states(&kid).await.last().unwrap(), "cancelled");
}

#[tokio::test(start_paused = true)]
async fn an_internal_formula_is_not_started_directly() {
    let w = shipyard();
    match w.core.send(None, start("child")).await {
        Err(Error::Rejected(why)) => assert!(why.contains("internal"), "{why}"),
        other => panic!("not rejected: {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_restart_keeps_the_child_and_its_end_ends_the_step() {
    let mut w = shipyard_with(POLICY);
    let (run, child) = ship(&w).await;
    let kid = child["id"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.restart().await;
    w.driver.script(turn());
    // Both attempts were lost; both runs go on, the parent with the child it had.
    let again = || Decision::NewSession {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, again()).await;
    w.decide(&kid, "run.attempt_failed", 0, again()).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(&kid, 2).unwrap();
    assert_eq!(w.finish(&kid, &token, Verdict::Pass, "after").await, "accepted");
    tokio::time::sleep(Duration::from_secs(11)).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.passed")["outcome"]["summary"], "after");
    let kids = (w.all("run.queued").into_iter())
        .filter(|e| e["run"]["parent_run"] == json!(run))
        .count();
    assert_eq!(kids, 1, "no second child");
}

#[tokio::test(start_paused = true)]
async fn the_step_input_is_stronger_than_the_inherited_one() {
    let w = shipyard_with(&[parent(
        "shop/formulas/noted.toml",
        "noted",
        "inputs = { note = \"explicit\" }\n",
    )]);
    let create = start_with("noted", json!({ "goal": "g", "note": "inherited" }));
    let (_, child) = ship_with(&w, create).await;
    let reader = aae_core::run::Reader::open(&w.dir.path().join("state.db")).unwrap();
    let view = reader.run(child["id"].as_str().unwrap()).unwrap().unwrap();
    assert_eq!(view["inputs"], json!({ "goal": "g", "note": "explicit" }));
}

#[tokio::test(start_paused = true)]
async fn a_step_past_its_timeout_is_a_runaway() {
    // The child's turn takes 10 s.
    let w = shipyard_with(&[parent("shop/formulas/timed.toml", "timed", "timeout = \"1s\"\n")]);
    let (run, child) = ship_with(&w, start("timed")).await;
    let (run, child) = (run, child["id"].as_str().unwrap().to_owned());
    let events = w.events(&run, Duration::from_secs(5)).await;
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "runaway_turn");
    // With no decision the parent fails in its step; the child it waited for goes with it.
    last(&events, "run.failed");
    let ended = w.events(&child, Duration::ZERO).await;
    assert_eq!(last(&ended, "run.cancelled")["reason"], "parent_failed");
}

#[tokio::test(start_paused = true)]
async fn the_parent_in_its_step_leaves_its_slot_to_the_child() {
    let one = (
        "config.toml",
        "projects = [\"shop\"]\nmax_runs = 1\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n",
    );
    let w = shipyard_with(&[one]);
    let (_, child) = ship(&w).await;
    let states = w.states(child["id"].as_str().unwrap()).await;
    assert!(states.iter().any(|s| s == "running"), "{states:?}");
}

#[tokio::test(start_paused = true)]
async fn a_step_with_attempts_left_runs_a_new_child() {
    let w = shipyard_with(&[parent("shop/formulas/retry.toml", "retry", "max_attempts = 2\n")]);
    w.driver.script(turn());
    let (run, child) = ship_with(&w, start("retry")).await;
    let first = child["id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(first, 1).unwrap();
    assert_eq!(w.finish(first, &token, Verdict::Fail, "no luck").await, "accepted");
    let second = w.until(None, "run.queued", 2).await["run"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(&second, 1).unwrap();
    assert_eq!(
        w.finish(&second, &token, Verdict::Pass, "second time").await,
        "accepted"
    );
    tokio::time::sleep(Duration::from_secs(11)).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.passed")["outcome"]["summary"], "second time");
}

/// `deliver` runs the internal `mend` on its bead, then a command.
const DELIVER: &[(&str, &str)] = &[
    (
        "shop/formulas/deliver.toml",
        "name = \"deliver\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
         [[step]]\nkind = \"formula\"\nformula = \"mend\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n",
    ),
    (
        "shop/formulas/mend.toml",
        "name = \"mend\"\ninternal = true\n[inputs]\nbead = { type = \"task\", required = true }\n\
         [[step]]\nkind = \"ai\"\nagent = \"dev\"\n",
    ),
];

/// Starts `deliver` on a new bead; its child has ended with `verdict` by the return.
async fn deliver(verdict: Verdict) -> (World, String, String) {
    let w = shipyard_with(DELIVER);
    let bead = w.tracker.add(&[], None);
    let reply = w.start_bead("deliver", Some("web"), json!(bead)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    let child = w.until(None, "run.queued", 1).await["run"].clone();
    assert_eq!(child["bead"], json!(bead), "the child inherits the task");
    let kid = child["id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        w.assignee(&bead),
        Some(format!("aae:{run}")),
        "the parent holds the bead"
    );
    let token = w.driver.token(kid, 1).unwrap();
    assert_eq!(w.finish(kid, &token, verdict, "mended").await, "accepted");
    tokio::time::sleep(Duration::from_secs(11)).await;
    (w, run, bead)
}

#[tokio::test(start_paused = true)]
async fn a_child_works_on_the_bead_its_parent_holds() {
    let (w, run, bead) = deliver(Verdict::Pass).await;
    // The command of the second step runs in real time.
    w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(w.tracker.bead(&bead).unwrap().status, "closed", "closed by the parent");
    assert!(w.all("resync.diverged").is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_child_leaves_the_bead_to_its_parent() {
    let (w, run, bead) = deliver(Verdict::Fail).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    last(&events, "run.failed");
    let left = w.tracker.bead(&bead).unwrap();
    assert_eq!((left.status.as_str(), left.assignee), ("open", None));
    assert!(w.all("resync.diverged").is_empty());
}

/// `stray` runs `mend` on the bead its `goal` names; `idle` on the next ready one.
const STRAY: &[(&str, &str)] = &[
    (
        "shop/formulas/stray.toml",
        "name = \"stray\"\n[inputs]\ngoal = { type = \"string\", required = true }\n\
         [[step]]\nkind = \"formula\"\nformula = \"mend\"\ninputs = { bead = \"{{ inputs.goal }}\" }\n",
    ),
    (
        "shop/formulas/idle.toml",
        "name = \"idle\"\n[[step]]\nkind = \"formula\"\nformula = \"mend\"\ninputs = { bead = { next_ready = true } }\n",
    ),
];

#[tokio::test(start_paused = true)]
async fn a_step_that_cannot_start_its_child_fails_as_infrastructure() {
    let rigs = [
        (
            "shop/aae.toml",
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        ),
        ("shop/api/.keep", ""),
    ];
    let files: Vec<_> = (DELIVER.iter().chain(STRAY).chain(common::beads::FIX).copied())
        .chain(rigs)
        .collect();
    let w = shipyard_with(&files);
    // A bead a run on the other rig holds, and no bead ready besides it.
    let bead = w.tracker.add(&[], None);
    w.fix_on(Some("api"), json!(bead)).await.unwrap();
    for (formula, inputs) in [("stray", json!({ "goal": bead })), ("idle", json!({}))] {
        let reply = w.core.send(None, start_with(formula, inputs)).await.unwrap();
        let run = reply["run"].as_str().unwrap();
        let events = w.events(run, Duration::from_millis(200)).await;
        assert_eq!(last(&events, "run.failed")["class"], "infrastructure", "{formula}");
    }
}
