//! A run started from an agent session (docs/design/contracts.md 6.1): the child of the session's run, its
//! correlation one level deeper, the chain bounded; cancelling the parent cancels its live children.

mod common;

use aae_core::{
    Error, Journal,
    run::{Create, Msg},
};
use aae_testkit::Step;
use common::{World, hollow_turn, last, world};
use serde_json::{Value, json};
use std::time::Duration;

/// A run of `build` on `web`, as `aae run start` in a session of `parent` with `token` asks for it.
fn child(parent: Option<&str>, token: Option<&str>, depth: u32) -> Msg {
    Msg::Create(Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "build".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth,
        parent_run: parent.map(str::to_owned),
        token: token.map(str::to_owned),
        continue_from: None,
    })
}

/// A parent whose session runs a minute, and its attempt token.
async fn parent(w: &World) -> (String, String) {
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(60)));
    w.driver.script(steps);
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(&run, 1).unwrap();
    (run, token)
}

/// The envelope of the first `run.queued` of `run`.
fn queued(w: &World, run: &str) -> Value {
    let all = Journal::new(&w.journal).read_from(1, 100_000).unwrap();
    all.into_iter()
        .find(|e| e["type"] == "run.queued" && e["run"] == run)
        .unwrap()
}

fn rejected(reply: Result<Value, Error>) -> String {
    match reply {
        Err(Error::Rejected(why)) => why,
        other => panic!("not rejected: {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_run_started_in_a_session_is_its_child_one_level_deeper() {
    let w = world(&[]);
    let (run, token) = parent(&w).await;
    let reply = w.core.send(None, child(Some(&run), Some(&token), 0)).await.unwrap();
    let kid = reply["run"].as_str().unwrap();
    let (mine, theirs) = (queued(&w, kid), queued(&w, &run));
    assert_eq!(mine["correlation"], theirs["correlation"]);
    assert_eq!(mine["data"]["run"]["parent_run"], json!(run));
    assert_eq!(mine["data"]["run"]["depth"], 1);
    let forged = w.core.send(None, child(Some(&run), Some("forged"), 0)).await;
    assert!(rejected(forged).contains("stale attempt"));
    let orphan = w.core.send(None, child(None, Some(&token), 0)).await;
    assert!(rejected(orphan).contains("needs its run"));
}

#[tokio::test(start_paused = true)]
async fn the_chain_of_children_stops_at_depth_ten() {
    let w = world(&[]);
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(60)));
    w.driver.script(steps);
    let deep = w.core.send(None, child(None, None, 10)).await.unwrap();
    let deep = deep["run"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let token = w.driver.token(&deep, 1).unwrap();
    let reply = w.core.send(None, child(Some(&deep), Some(&token), 0)).await;
    assert!(rejected(reply).contains("depth 11 is over 10"));
}

#[tokio::test(start_paused = true)]
async fn cancelling_the_parent_cancels_its_live_children_and_stales_its_token() {
    let w = world(&[]);
    let (run, token) = parent(&w).await;
    let reply = w.core.send(None, child(Some(&run), Some(&token), 0)).await.unwrap();
    let kid = reply["run"].as_str().unwrap().to_owned();
    let cancel = Msg::Cancel {
        run: run.clone(),
        reason: "by the operator".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    let events = w.events(&kid, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.cancelled")["reason"], "parent_cancelled");
    let late = w.core.send(None, child(Some(&run), Some(&token), 0)).await;
    assert!(rejected(late).contains("stale attempt"));
}
