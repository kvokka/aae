//! Bead ownership on the scripted driver and the tracker in memory (spec §10.3, invariants 2–5; §15 criteria 16
//! and 5a, core part).

mod common;

use aae_core::{
    Journal,
    run::{Create, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{World, hollow_turn, last, world};
use serde_json::{Value, json};
use std::time::Duration;

const FIX: &[(&str, &str)] = &[
    (
        "shop/formulas/fix.toml",
        "name = \"fix\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
         [step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"fix.md\"\n",
    ),
    ("shop/formulas/fix.md", "Fix {{ bead.title }}."),
];

impl World {
    async fn fix(&self, bead: Value) -> Result<Value, aae_core::Error> {
        let create = Create {
            project: "shop".into(),
            rig: "web".into(),
            formula: "fix".into(),
            inputs: [("bead".to_owned(), bead)].into_iter().collect(),
            correlation: None,
            depth: 0,
            parent_run: None,
            continue_from: None,
        };
        self.core.send(None, Msg::Create(create)).await
    }

    /// The tracker says it changed; the core reads its ready set again.
    async fn changed(&self) {
        let msg = Msg::TrackerChanged("shop".into());
        self.core.send(None, msg).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    fn all(&self, kind: &str) -> Vec<Value> {
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        all.into_iter()
            .filter(|e| e["type"] == kind)
            .map(|e| e["data"].clone())
            .collect()
    }

    async fn pass(&self, run: &str) {
        let token = self.driver.token(run, 1).unwrap();
        let outcome = Outcome {
            result: Verdict::Pass,
            summary: "fixed".into(),
            data: None,
        };
        let msg = Msg::Finish {
            run: run.into(),
            token,
            outcome,
        };
        self.core.send(None, msg).await.unwrap();
    }
}

fn next_ready() -> Value {
    json!({ "next_ready": true })
}

/// A turn that ends 10 s after the prompt: time for the finisher.
fn slow_turn() -> Vec<Step> {
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(10)));
    steps
}

#[tokio::test(start_paused = true)]
async fn five_requests_for_the_next_ready_bead_take_five_beads_and_the_sixth_finds_nothing() {
    let w = world(FIX);
    let beads: Vec<String> = (0..5).map(|_| w.tracker.add(&[], None)).collect();
    w.changed().await;
    let replies = tokio::join!(
        w.fix(next_ready()),
        w.fix(next_ready()),
        w.fix(next_ready()),
        w.fix(next_ready()),
        w.fix(next_ready()),
    );
    let runs: Vec<String> = [replies.0, replies.1, replies.2, replies.3, replies.4]
        .into_iter()
        .map(|r| r.unwrap()["run"].as_str().unwrap().to_owned())
        .collect();
    let mut taken: Vec<String> = Vec::new();
    for run in &runs {
        let events = w.events(run, Duration::ZERO).await;
        taken.push(last(&events, "run.queued")["run"]["bead"].as_str().unwrap().to_owned());
    }
    taken.sort();
    assert_eq!(taken, beads, "every bead to exactly one run");
    assert_eq!(w.fix(next_ready()).await.unwrap(), json!({ "nothing_ready": true }));
    let second = w.fix(json!(beads[0])).await.unwrap_err().to_string();
    assert!(second.contains("is held by `run_"), "{second}");
}

#[tokio::test(start_paused = true)]
async fn a_run_claims_its_bead_shows_it_to_the_agent_and_closes_it_when_it_passes() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    w.driver.script(slow_turn());
    let run = w.fix(json!(bead)).await.unwrap()["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let held = w.tracker.bead(&bead).unwrap();
    assert_eq!(held.assignee.as_deref(), Some(format!("aae:{run}").as_str()));
    assert_eq!(w.driver.sent(&run, 1).1, [format!("Fix Task {bead}.")]);
    w.pass(&run).await;
    tokio::time::sleep(Duration::from_secs(20)).await;
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.passed")["run"]["bead"], json!(bead));
    assert_eq!(w.tracker.bead(&bead).unwrap().status, "closed");
    w.changed().await;
    assert_eq!(w.fix(next_ready()).await.unwrap(), json!({ "nothing_ready": true }));
}

#[tokio::test(start_paused = true)]
async fn failed_runs_release_the_bead_and_the_third_closes_its_gate_until_the_operator_opens_it() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    for _ in 0..3 {
        w.driver.script(hollow_turn());
        let run = w.fix(json!(bead)).await.unwrap()["run"].as_str().unwrap().to_owned();
        assert_eq!(w.states(&run).await.last().unwrap(), "failed");
        let released = w.tracker.bead(&bead).unwrap();
        assert_eq!((released.assignee, released.status.as_str()), (None, "open"));
    }
    let closed = w.all("gate.closed");
    let key = format!("bead:shop/{bead}");
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(
        (
            closed[0]["gate"]["key"].as_str(),
            closed[0]["gate"]["closed_by"].as_str()
        ),
        (Some(key.as_str()), Some("core"))
    );
    w.changed().await;
    assert_eq!(
        w.fix(next_ready()).await.unwrap(),
        json!({ "nothing_ready": true }),
        "a gated bead is skipped"
    );
    let waiting = w.fix(json!(bead)).await.unwrap()["run"].as_str().unwrap().to_owned();
    let events = w.events(&waiting, Duration::ZERO).await;
    assert_eq!(last(&events, "run.queued")["waiting_for"], "gate");
    w.driver.script(hollow_turn());
    w.core.send(None, Msg::OpenGate { key }).await.unwrap();
    assert_eq!(w.states(&waiting).await.last().unwrap(), "failed");
    assert_eq!(w.all("gate.closed").len(), 1, "the operator's opening reset the count");
}

#[tokio::test(start_paused = true)]
async fn a_bead_someone_else_holds_fails_the_run_as_bead_claimed_and_stays_theirs() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    w.tracker.assign(&bead, "alice");
    let run = w.fix(json!(bead)).await.unwrap()["run"].as_str().unwrap().to_owned();
    let events = w.events(&run, Duration::from_secs(1)).await;
    let failed = last(&events, "run.failed");
    assert_eq!(
        (failed["class"].as_str(), failed["reason"].as_str()),
        (Some("infrastructure"), Some("bead_claimed"))
    );
    assert_eq!(w.tracker.bead(&bead).unwrap().assignee.as_deref(), Some("alice"));
    assert!(w.all("resync.diverged").is_empty(), "nothing of alice's was released");
}

#[tokio::test(start_paused = true)]
async fn an_unavailable_tracker_is_reported_and_the_claim_retried_until_it_answers() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    w.changed().await;
    w.tracker.set_down(true);
    w.driver.script(slow_turn());
    let run = w.fix(json!(bead)).await.unwrap()["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "preparing");
    let all = w.all("tracker.sync_failed");
    let failed: Vec<&Value> = all.iter().filter(|f| f["op"] == "claim").collect();
    assert!(failed.len() >= 2, "{all:?}");
    assert_eq!(
        (failed[0]["op"].as_str(), failed[0]["bead"].as_str()),
        (Some("claim"), Some(bead.as_str()))
    );
    assert!(failed[0]["next_retry_at"].is_string());
    w.tracker.set_down(false);
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(w.states(&run).await.contains(&"running".to_owned()));
}

#[tokio::test(start_paused = true)]
async fn a_trigger_on_the_next_ready_bead_takes_each_bead_once_then_skips_with_nothing_ready() {
    let take = "[rigs.web]\npath = \"web\"\n\n[[trigger]]\nname = \"take\"\nevery = \"1h\"\nrun = \"fix\"\nrig = \"web\"\n\
        inputs = { bead = { next_ready = true } }\n";
    let w = world(&[FIX[0], FIX[1], ("shop/aae.toml", take)]);
    let beads = [
        w.tracker.add(&[], None),
        w.tracker.add(&["rig:api"], None),
        w.tracker.add(&["rig:web"], None),
    ];
    w.changed().await;
    let fire = || Msg::Fire {
        project: "shop".into(),
        rig: "web".into(),
        trigger: "take".into(),
    };
    let mut taken = Vec::new();
    for _ in 0..2 {
        let run = w.core.send(None, fire()).await.unwrap()["run"]
            .as_str()
            .unwrap()
            .to_owned();
        let events = w.events(&run, Duration::ZERO).await;
        taken.push(last(&events, "run.queued")["run"]["bead"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        taken,
        [beads[0].clone(), beads[2].clone()],
        "a bead of another rig is not taken"
    );
    let third = w.core.send(None, fire()).await.unwrap();
    assert_eq!(third["skipped"], "nothing_ready");
}
