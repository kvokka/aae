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
    async fn run_on(&self, bead: Value) -> String {
        let reply = self.fix(bead).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    fn assignee(&self, bead: &str) -> Option<String> {
        self.tracker.bead(bead).unwrap().assignee
    }

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

    async fn finish(&self, run: &str, result: Verdict, summary: &str) {
        let token = self.driver.token(run, 1).unwrap();
        let outcome = Outcome {
            result,
            summary: summary.into(),
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
    w.finish(&run, Verdict::Pass, "fixed").await;
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

#[tokio::test(start_paused = true)]
async fn the_next_ready_bead_in_an_epic_skips_beads_of_other_epics() {
    let w = world(FIX);
    w.tracker.add(&[], Some("e-other"));
    w.tracker.add(&[], None);
    let mine = w.tracker.add(&[], Some("e-1"));
    w.changed().await;
    let in_epic = || json!({ "next_ready": true, "epic": "e-1" });
    let run = w.run_on(in_epic()).await;
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.queued")["run"]["bead"], json!(mine));
    assert_eq!(w.fix(in_epic()).await.unwrap(), json!({ "nothing_ready": true }));
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_run_releases_its_bead_without_counting_a_failure() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    w.driver.script(slow_turn());
    let run = w.run_on(json!(bead)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.assignee(&bead), Some(format!("aae:{run}")));
    let cancel = Msg::Cancel {
        run: run.clone(),
        reason: "operator".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "cancelled");
    let released = w.tracker.bead(&bead).unwrap();
    assert_eq!((released.assignee, released.status.as_str()), (None, "open"));
}

#[tokio::test(start_paused = true)]
async fn an_agent_failure_whose_summary_starts_with_bead_still_releases_the_bead() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    w.driver.script(slow_turn());
    let run = w.run_on(json!(bead)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.finish(&run, Verdict::Fail, "bead_claimed").await;
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "failed");
    assert_eq!(w.assignee(&bead), None, "the claim of a failed run goes back");
}

#[tokio::test(start_paused = true)]
async fn claims_that_answer_after_their_runs_failed_neither_count_nor_reach_the_next_run() {
    let w = world(FIX);
    let bead = w.tracker.add(&[], None);
    w.tracker.set_down(true);
    for _ in 0..3 {
        let run = w.run_on(json!(bead)).await;
        tokio::time::sleep(Duration::from_secs(121)).await;
        let events = w.events(&run, Duration::ZERO).await;
        assert_eq!(last(&events, "run.failed")["reason"], "tracker_unavailable");
    }
    assert!(
        w.all("gate.closed").is_empty(),
        "a claim never held is not a failure on the bead"
    );
    w.tracker.set_down(false);
    // The queue drains at its next retry (at most the 5 min cap): each late claim, then its release.
    tokio::time::sleep(Duration::from_secs(301)).await;
    assert_eq!(w.assignee(&bead), None);
    w.driver.script(slow_turn());
    let run = w.run_on(json!(bead)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(w.states(&run).await.contains(&"running".to_owned()));
    assert_eq!(
        w.assignee(&bead),
        Some(format!("aae:{run}")),
        "the stale claims were released before"
    );
    assert_eq!(w.driver.sent(&run, 1).1, [format!("Fix Task {bead}.")]);
}

#[tokio::test(start_paused = true)]
async fn tracker_writes_become_bead_events_and_a_write_by_a_run_carries_its_chain() {
    let w = world(FIX);
    let old = w.tracker.add(&[], None);
    w.changed().await;
    assert!(
        w.all("bead.created").is_empty(),
        "the history before the first read raises nothing"
    );
    let ready = w.all("bead.ready");
    assert_eq!((ready.len(), &ready[0]["bead"]["id"]), (1, &json!(old)));
    let new = w.tracker.add(&["rig:web"], None);
    w.changed().await;
    let created = w.all("bead.created");
    assert_eq!(created.len(), 1, "{created:?}");
    assert_eq!(
        (&created[0]["bead"]["id"], &created[0]["actor"]),
        (&json!(new), &json!("human"))
    );
    assert_eq!(w.all("bead.ready").len(), 2, "each bead is ready once");
    w.driver.script(slow_turn());
    let run = w.run_on(json!(new)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.changed().await;
    let all = Journal::new(&w.journal).read_from(1, 100_000).unwrap();
    let claimed = all.iter().rfind(|e| e["type"] == "bead.updated").unwrap();
    let queued = all.iter().find(|e| e["type"] == "run.queued").unwrap();
    assert_eq!(
        (&claimed["run"], &claimed["rig"], &claimed["correlation"]),
        (&json!(run), &json!("web"), &queued["correlation"]),
        "the claim is the run's write"
    );
    assert_eq!(claimed["data"]["changed"], json!(["status", "assignee"]));
}
