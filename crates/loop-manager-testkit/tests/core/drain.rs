//! A task becomes the root bead of its run, or names the bead it takes up (docs/design/config.md,
//! docs/design/tracker-br.md), on the scripted driver and the tracker in memory.

use crate::common;

use common::{World, beads::slow_turn, last, world};
use loop_manager_core::{
    run::{GateSpec, Msg},
    tracker::{Bead, Tracker},
};
use serde_json::{Value, json};
use std::time::Duration;

const ASK: &[(&str, &str)] = &[
    (
        "shop/formulas/ask.toml",
        "name = \"ask\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
         [[step]]\nkind = \"ai\"\nagent = \"asker\"\n",
    ),
    ("shop/agents/asker/agent.toml", "harness = \"scripted\"\n"),
    ("shop/agents/asker/prompt.template.md", "Do: {{ bead.description }}"),
];

const TEXT: &str = "\n  Ship the checkout page  \nwith the new totals.";

impl World {
    async fn ask(&self, input: Value) -> Result<Value, loop_manager_core::Error> {
        self.start_bead("ask", Some("web"), input).await
    }

    /// The payload of the pending effect that creates the root bead of `run`.
    fn create_effect(&self, run: &str) -> Value {
        let db = rusqlite::Connection::open(self.dir.path().join("state.db")).unwrap();
        let payload: String = db
            .query_row(
                "SELECT payload FROM effects WHERE dedupe = ?1",
                [format!("create:{run}")],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str(&payload).unwrap()
    }

    fn run_bead(&self, run: &str) -> Option<String> {
        let db = rusqlite::Connection::open(self.dir.path().join("state.db")).unwrap();
        db.query_row("SELECT bead FROM runs WHERE id = ?1", [run], |r| r.get(0))
            .unwrap()
    }
}

#[tokio::test(start_paused = true)]
async fn a_text_task_becomes_one_root_bead_that_its_run_claims_and_shows_to_the_agent() {
    let w = world(ASK);
    w.driver.script(slow_turn());
    let reply = w.ask(json!(TEXT)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let bead = w.tracker.bead("b-1").expect("the root bead");
    assert!(w.tracker.bead("b-2").is_none(), "one bead");
    assert_eq!(
        (bead.title.as_str(), bead.description.as_str(), bead.epic),
        ("Ship the checkout page", TEXT, None)
    );
    assert_eq!(w.run_bead(&run).as_deref(), Some("b-1"));
    assert_eq!(bead.assignee, Some(format!("loop-manager:{run}")));
    assert_eq!(w.driver.sent(&run, 1).1, [format!("Do: {TEXT}")]);
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.state_changed")["to"], "running");
}

#[tokio::test(start_paused = true)]
async fn a_root_bead_created_before_a_crash_is_the_one_the_run_gets_and_a_repeated_answer_is_dropped() {
    let mut w = world(ASK);
    w.changed().await;
    w.tracker.set_down(true);
    w.driver.script(slow_turn());
    let reply = w.ask(json!("Fix the login")).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "preparing");
    let failed = w.all("tracker.sync_failed");
    assert!(failed.iter().any(|f| f["op"] == "create"), "{failed:?}");
    // The tracker created the bead, and its answer was lost with the daemon.
    let payload = w.create_effect(&run);
    let key = payload["key"].as_str().unwrap();
    assert!(key.starts_with("loop-manager-") && key.len() == 29, "{key}");
    w.tracker.set_down(false);
    let bead: Bead = Tracker::create(
        &w.tracker,
        &format!("loop-manager:{run}"),
        key,
        "Fix the login",
        "Fix the login",
    )
    .await
    .unwrap();
    w.tracker.set_down(true);
    w.restart().await;
    // The answer is recorded once; the effect, not yet removed, answers again when the tracker is back.
    let answer = Msg::Tracked {
        payload,
        result: Ok(Some(bead)),
    };
    w.core.send(None, answer).await.unwrap();
    w.tracker.set_down(false);
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert!(w.tracker.bead("b-2").is_none(), "one bead");
    assert_eq!(w.run_bead(&run).as_deref(), Some("b-1"));
    assert_eq!(w.assignee("b-1"), Some(format!("loop-manager:{run}")));
    assert!(w.states(&run).await.contains(&"running".to_owned()));
    assert!(
        w.events(&run, Duration::ZERO)
            .await
            .iter()
            .all(|(t, _)| t != "run.failed")
    );
}

#[tokio::test(start_paused = true)]
async fn a_blank_task_or_another_form_is_refused_and_one_with_no_rig_runs_in_the_root() {
    let w = world(ASK);
    for input in [
        json!(""),
        json!(" \n "),
        json!(3),
        json!({ "request": "x" }),
        json!({ "next_ready": true, "request": "x" }),
    ] {
        let refused = w.ask(input.clone()).await.unwrap_err().to_string();
        assert!(refused.contains("a task is a text"), "{input}: {refused}");
    }
    assert!(w.tracker.bead("b-1").is_none());
    // With no rig, the task's run works in the project root (docs/design/config.md).
    w.driver.script(slow_turn());
    let reply = w.start_bead("ask", None, json!("x")).await.unwrap();
    let run = reply["run"].as_str().unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let rig: String = db
        .query_row("SELECT rig FROM runs WHERE id = ?1", [run], |r| r.get(0))
        .unwrap();
    assert_eq!(rig, "root");
    assert_eq!(w.run_bead(run).as_deref(), Some("b-1"));
}

#[tokio::test(start_paused = true)]
async fn a_task_naming_a_bead_takes_it_up_and_files_no_second_bead() {
    let w = world(ASK);
    // One of the ready set, and one the core has not seen yet.
    let ready = w.tracker.add(&[], None);
    w.changed().await;
    let unseen = w.tracker.add(&[], None);
    let mut runs = Vec::new();
    for bead in [&unseen, &ready] {
        w.driver.script(slow_turn());
        let reply = w.ask(json!(bead)).await.unwrap();
        runs.push(reply["run"].as_str().unwrap().to_owned());
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    for (run, bead) in runs.iter().zip([&unseen, &ready]) {
        assert_eq!(w.run_bead(run).as_deref(), Some(bead.as_str()));
    }
    // The second run waits for the one slot; the first claimed the bead the tracker answered with.
    assert_eq!(w.assignee(&unseen), Some(format!("loop-manager:{}", runs[0])));
    assert!(w.tracker.bead("b-3").is_none(), "no bead filed");
    // A bead a run holds is no text either: a second task naming it is refused.
    let refused = w.ask(json!(unseen)).await.unwrap_err().to_string();
    assert!(refused.contains("is held by"), "{refused}");
}

#[tokio::test(start_paused = true)]
async fn two_tasks_naming_a_bead_not_read_yet_leave_it_to_the_first_and_fail_the_second() {
    let rigs = (
        "shop/loop-manager.toml",
        "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
    );
    let w = world(&[ASK, &[rigs, ("shop/api/.keep", "")]].concat());
    w.changed().await;
    let unseen = w.tracker.add(&[], None);
    // The tracker holds both answers back: neither run owns the bead when the other starts.
    w.tracker.set_down(true);
    let mut runs = Vec::new();
    for rig in ["web", "api"] {
        w.driver.script(slow_turn());
        let reply = w.start_bead("ask", Some(rig), json!(unseen)).await.unwrap();
        runs.push(reply["run"].as_str().unwrap().to_owned());
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.tracker.set_down(false);
    tokio::time::sleep(Duration::from_secs(8)).await;
    // One live owner per bead (docs/design/invariants.md): the tracker answers both with the same bead, the second
    // answer fails.
    assert_eq!(w.run_bead(&runs[0]).as_deref(), Some(unseen.as_str()));
    assert_eq!(w.assignee(&unseen), Some(format!("loop-manager:{}", runs[0])));
    assert_eq!(w.run_bead(&runs[1]), None);
    let failed = w.events(&runs[1], Duration::ZERO).await;
    let reason = last(&failed, "run.failed")["reason"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(reason.contains("held by"), "{failed:?}");
    assert!(w.tracker.bead("b-2").is_none(), "no bead filed");
    assert!(!w.states(&runs[0]).await.contains(&"failed".to_owned()));
    let (session, texts) = w.driver.sent(&runs[0], 1);
    assert!(
        session.is_some() && texts.len() == 1,
        "the first run's agent works on: {texts:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_task_naming_a_bead_not_read_yet_waits_queued_while_that_bead_is_gated() {
    let w = world(ASK);
    w.changed().await;
    let unseen = w.tracker.add(&[], None);
    let key = format!("bead:shop/{unseen}");
    let gate = GateSpec {
        key: key.clone(),
        until_ms: None,
        reason: "hold".into(),
    };
    w.core.send(None, Msg::CloseGate(gate)).await.unwrap();
    w.driver.script(slow_turn());
    let run = w.ask(json!(unseen)).await.unwrap()["run"].as_str().unwrap().to_owned();
    let events = w.events(&run, Duration::from_secs(60)).await;
    assert_eq!(last(&events, "run.queued")["gate"]["key"], key.as_str());
    assert!(w.states(&run).await.is_empty(), "never left `queued`");
    w.core.send(None, Msg::OpenGate { key }).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.run_bead(&run).as_deref(), Some(unseen.as_str()));
    assert_eq!(w.states(&run).await.last().unwrap(), "running");
}
