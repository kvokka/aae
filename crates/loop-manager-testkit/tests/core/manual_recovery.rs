//! Recovery by hand of a run that failed for good (docs/guide/recovery.md): what the state shows of the failure and
//! of the sessions of every attempt, and the ways on from it with the operations of the frontends: continue the
//! session, a new run on the same bead with or without a note, or abandon the bead. On the managed clock.

use crate::common;

use common::{
    POLICY, World,
    beads::{FIX, next_ready, slow_turn},
    world,
};
use loop_manager_core::{
    Error,
    run::{Create, Decision, GateSpec, Msg, Reader, RunFilter},
};
use loop_manager_testkit::Step;
use serde_json::{Value, json};
use std::time::Duration;

/// A formula on a bead whose prompt carries a note for the session: the note is an input of the formula.
const REFIX: &[(&str, &str)] = &[
    ("shop/agents/refixer/agent.toml", "harness = \"scripted\"\n"),
    (
        "shop/agents/refixer/prompt.template.md",
        "{{ inputs.note }} Fix {{ bead.title }}.",
    ),
    (
        "shop/formulas/refix.toml",
        "name = \"refix\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
         note = { type = \"string\", required = true }\n[[step]]\nkind = \"ai\"\nagent = \"refixer\"\n",
    ),
];

fn crash() -> Vec<Step> {
    vec![Step::Exit {
        code: Some(1),
        signal: None,
    }]
}

impl World {
    /// `loop-manager run show`: the state of `run`.
    fn show(&self, run: &str) -> Value {
        let reader = Reader::open(&self.dir.path().join("state.db")).unwrap();
        reader.run(run).unwrap().unwrap()
    }

    /// A start of `formula` with `inputs` on `web`, continuing the session of `from` if given.
    async fn start_with(&self, formula: &str, inputs: Value, from: Option<&str>) -> String {
        let create = Create {
            project: "shop".into(),
            rig: Some("web".into()),
            formula: formula.into(),
            inputs: inputs.as_object().unwrap().clone(),
            correlation: None,
            depth: 0,
            parent_run: None,
            token: None,
            continue_from: from.map(Into::into),
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// A run of `fix` on `bead` whose only attempt fails: no policy decides, so the run fails for good.
    async fn failed_on(&self, bead: &str) -> String {
        self.driver.script(crash());
        let run = self.run_on(json!(bead)).await;
        assert_eq!(self.states(&run).await.last().unwrap(), "failed");
        run
    }
}

#[tokio::test(start_paused = true)]
async fn a_failed_run_shows_its_failure_and_the_session_and_failure_kind_of_every_attempt() {
    let w = world(&[POLICY, FIX].concat());
    let bead = w.tracker.add(&[], None);
    w.driver.script(crash());
    let run = w.run_on(json!(bead)).await;
    let new_session = Decision::NewSession {
        after: Duration::ZERO,
        gate: None,
    };
    w.driver.script(crash());
    w.decide(&run, "run.attempt_failed", 0, new_session).await;
    let give_up = Decision::Fail {
        reason: "given up".into(),
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 1, give_up).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let reader = Reader::open(&w.dir.path().join("state.db")).unwrap();
    let failed = RunFilter {
        state: Some("failed".into()),
        ..RunFilter::default()
    };
    let listed = reader.runs(&failed).unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    let view = &listed[0];
    assert_eq!(
        (&view["id"], &view["class"], &view["reason"], &view["bead"]),
        (&json!(run), &json!("infrastructure"), &json!("given up"), &json!(bead))
    );
    let attempts = view["attempts"].as_array().unwrap();
    let kinds: Vec<&Value> = attempts.iter().map(|a| &a["failure_kind"]).collect();
    assert_eq!(kinds, [&json!("harness_crash"), &json!("harness_crash")]);
    let sessions: Vec<&Value> = attempts.iter().map(|a| &a["session"]).collect();
    assert!(
        sessions[0].is_object() && sessions[1].is_object() && sessions[0] != sessions[1],
        "{sessions:?}"
    );
}

/// S1: a new run continues the session of the run that failed on its attempt, on the same bead.
#[tokio::test(start_paused = true)]
async fn a_run_that_failed_on_an_attempt_is_continued_in_its_session_on_its_bead() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    let failed = w.failed_on(&bead).await;
    w.driver.script(slow_turn());
    let next = w.start_with("fix", json!({ "bead": bead }), Some(&failed)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.assignee(&bead), Some(format!("loop-manager:{next}")));
    let attempt = &w.show(&next)["attempts"][0];
    assert_eq!(attempt["kind"], "resume", "{attempt}");
    assert_eq!(attempt["session"], w.show(&failed)["attempts"][0]["session"]);
}

/// S2 and S3: a new run of a formula takes the bead of the failed run again, in a session of its own; a formula
/// whose prompt carries a note tells that session the work was begun before.
#[tokio::test(start_paused = true)]
async fn a_new_run_takes_the_bead_of_a_failed_run_again_with_or_without_a_note() {
    let w = world(&[FIX, REFIX].concat());
    let bead = w.tracker.add(&[], None);
    let first = w.failed_on(&bead).await;
    let again = w.failed_on(&bead).await;
    assert_eq!(w.show(&again)["attempts"][0]["kind"], "new_session");
    assert_ne!(
        w.show(&again)["attempts"][0]["session"],
        w.show(&first)["attempts"][0]["session"]
    );
    w.driver.script(slow_turn());
    let note = "Begun before, state unknown: sort it out.";
    let noted = w.start_with("refix", json!({ "bead": bead, "note": note }), None).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.assignee(&bead), Some(format!("loop-manager:{noted}")));
    let prompts = w.driver.sent(&noted, 1).1;
    assert_eq!(prompts.last(), Some(&format!("{note} Fix Task {bead}.")));
}

/// S4: a failed run has already released its bead and cannot be cancelled; a gate on the bead keeps every run off it
/// until the operator opens the gate.
#[tokio::test(start_paused = true)]
async fn an_abandoned_bead_is_released_and_a_gate_keeps_runs_off_it() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    let failed = w.failed_on(&bead).await;
    let released = w.tracker.bead(&bead).unwrap();
    assert_eq!((released.assignee, released.status.as_str()), (None, "open"));
    let cancel = Msg::Cancel {
        run: failed,
        reason: "abandoned".into(),
    };
    assert!(matches!(w.core.send(None, cancel).await, Err(Error::Rejected(_))));
    let key = format!("bead:shop/{bead}");
    let gate = GateSpec {
        key: key.clone(),
        until_ms: None,
        reason: "abandoned".into(),
    };
    w.core.send(None, Msg::CloseGate(gate)).await.unwrap();
    w.changed().await;
    assert_eq!(w.fix(next_ready()).await.unwrap(), json!({ "nothing_ready": true }));
    let waiting = w.run_on(json!(bead)).await;
    assert_eq!(w.show(&waiting)["gate"]["key"], key.as_str());
    assert_eq!(w.show(&waiting)["state"], "queued");
}
