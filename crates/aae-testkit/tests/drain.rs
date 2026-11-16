//! A request becomes the root bead of its run (spec §10.1), on the scripted driver and the tracker in memory.

mod common;

use aae_core::{
    run::Msg,
    tracker::{Bead, Tracker},
};
use common::{World, beads::slow_turn, last, world};
use serde_json::{Value, json};
use std::time::Duration;

const ASK: &[(&str, &str)] = &[
    (
        "shop/formulas/ask.toml",
        "name = \"ask\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
         [[step]]\nkind = \"ai\"\nagent = \"dev\"\nprompt = \"ask.md\"\n",
    ),
    ("shop/formulas/ask.md", "Do: {{ bead.description }}"),
];

const TEXT: &str = "\n  Ship the checkout page  \nwith the new totals.";

impl World {
    async fn ask(&self, input: Value) -> Result<Value, aae_core::Error> {
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
async fn a_request_becomes_one_root_bead_that_its_run_claims_and_shows_to_the_agent() {
    let w = world(ASK);
    w.driver.script(slow_turn());
    let reply = w.ask(json!({ "request": TEXT })).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let bead = w.tracker.bead("b-1").expect("the root bead");
    assert!(w.tracker.bead("b-2").is_none(), "one bead");
    assert_eq!(
        (bead.title.as_str(), bead.description.as_str(), bead.epic),
        ("Ship the checkout page", TEXT, None)
    );
    assert_eq!(w.run_bead(&run).as_deref(), Some("b-1"));
    assert_eq!(bead.assignee, Some(format!("aae:{run}")));
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
    let reply = w.ask(json!({ "request": "Fix the login" })).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "preparing");
    let failed = w.all("tracker.sync_failed");
    assert!(failed.iter().any(|f| f["op"] == "create"), "{failed:?}");
    // The tracker created the bead, and its answer was lost with the daemon.
    let payload = w.create_effect(&run);
    let key = payload["key"].as_str().unwrap();
    assert!(key.starts_with("aae-") && key.len() == 20, "{key}");
    w.tracker.set_down(false);
    let bead: Bead = Tracker::create(&w.tracker, &format!("aae:{run}"), key, "Fix the login", "Fix the login")
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
    assert_eq!(w.assignee("b-1"), Some(format!("aae:{run}")));
    assert!(w.states(&run).await.contains(&"running".to_owned()));
    assert!(
        w.events(&run, Duration::ZERO)
            .await
            .iter()
            .all(|(t, _)| t != "run.failed")
    );
}

#[tokio::test(start_paused = true)]
async fn a_request_with_no_text_or_another_key_or_no_rig_is_refused() {
    let w = world(ASK);
    for input in [
        json!({ "request": "" }),
        json!({ "request": " \n " }),
        json!({ "request": 3 }),
        json!({ "request": "x", "epic": "b-9" }),
    ] {
        let refused = w.ask(input.clone()).await.unwrap_err().to_string();
        assert!(refused.contains("a bead request is"), "{input}: {refused}");
    }
    let refused = w
        .start_bead("ask", None, json!({ "request": "x" }))
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("no rig"), "{refused}");
    assert!(w.tracker.bead("b-1").is_none());
}
