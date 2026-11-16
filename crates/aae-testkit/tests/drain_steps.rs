//! The drain step (spec §9.1, §10.1): every ready bead under the run's bead, at any depth, gets a run of the step's
//! formula, once per entry of the step, on the scripted driver and the tracker in memory.

mod common;

use aae_core::run::{Create, Decision, GateSpec, Msg, Verdict};
use common::{POLICY, beads::slow_turn, drain::SWEEP, world};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[tokio::test(start_paused = true)]
async fn a_drain_gives_every_ready_bead_under_the_root_a_run() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let web = w.tracker.add(&["rig:web", "agent:qa"], Some(&root));
    let api = w.tracker.add(&["rig:api"], Some(&root));
    w.tracker.add(&["rig:web"], None);
    let run = w.sweep(&root).await;
    let outcome = w.settle(&run, &[], &[], &mut BTreeSet::new()).await;
    assert_eq!(outcome["result"], "pass");
    assert_eq!(
        outcome["data"],
        json!({ "closed": [web, api], "failed": [], "refused": {} })
    );
    let kids = w.kids(&run);
    assert_eq!(kids.len(), 2, "{kids:?}");
    let correlation = &kids[0].5;
    for (kid, (bead, rig, agent)) in kids.iter().zip([(&web, "web", "shop.qa"), (&api, "api", "shop.dev")]) {
        assert_eq!(
            (&kid.1, kid.2.as_str(), kid.3.as_str(), kid.4.as_str(), kid.6),
            (bead, "passed", rig, agent, 1)
        );
        assert_eq!(&kid.5, correlation);
        assert_eq!(w.tracker.bead(bead).unwrap().status, "closed");
    }
    assert_eq!(w.wrap(&run).await, "Wrap up: 2 closed, 0 failed, 0 refused.");
}

#[tokio::test(start_paused = true)]
async fn a_bead_added_meanwhile_runs_too() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let first = w.tracker.add(&["rig:web"], Some(&root));
    let run = w.sweep(&root).await;
    let mut done = BTreeSet::new();
    w.settle(&run, &[], &[first.as_str()], &mut done).await;
    // No word from the tracker: the read after the close of the first bead brings it.
    let later = w.tracker.add(&["rig:api"], Some(&root));
    let outcome = w.settle(&run, &[], &[], &mut done).await;
    assert_eq!(outcome["data"]["closed"], json!([first, later]));
    assert_eq!(w.kids(&run).len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_failed_bead_stops_nothing_and_runs_once() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let bad = w.tracker.add(&["rig:web"], Some(&root));
    let good = w.tracker.add(&["rig:web"], Some(&root));
    let run = w.sweep(&root).await;
    let outcome = w.settle(&run, &[bad.as_str()], &[], &mut BTreeSet::new()).await;
    assert_eq!(
        (
            &outcome["result"],
            &outcome["data"]["closed"],
            &outcome["data"]["failed"]
        ),
        (&json!("pass"), &json!([good]), &json!([bad]))
    );
    let kids: Vec<String> = w.kids(&run).into_iter().map(|k| k.1).collect();
    assert_eq!(kids, [bad.clone(), good], "one run per bead");
    assert_eq!(w.tracker.bead(&bad).unwrap().status, "open");
    assert_eq!(w.wrap(&run).await, "Wrap up: 1 closed, 1 failed, 0 refused.");
}

#[tokio::test(start_paused = true)]
async fn a_refused_bead_gets_no_run_and_a_reason() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let codex = w.tracker.add(&["rig:web", "harness:codex"], Some(&root));
    let run = w.sweep(&root).await;
    let outcome = w.settle(&run, &[], &[], &mut BTreeSet::new()).await;
    let why = outcome["data"]["refused"][&codex].as_str().unwrap();
    assert!(
        why.contains("labelled `harness:codex`: the config gives `scripted` to `shop.dev`"),
        "{why}"
    );
    assert!(w.kids(&run).is_empty());
    assert_eq!(w.tracker.bead(&codex).unwrap().assignee, None);
    assert_eq!(w.wrap(&run).await, "Wrap up: 0 closed, 0 failed, 1 refused.");
}

/// A child of a drain is one level deeper than its run, under the common depth limit (spec §9.3).
#[tokio::test(start_paused = true)]
async fn a_drain_at_the_depth_limit_gives_no_bead_a_run() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let bead = w.tracker.add(&["rig:web"], Some(&root));
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "sweep".into(),
        inputs: [("bead".to_owned(), json!(root))].into_iter().collect(),
        correlation: None,
        depth: 10,
        parent_run: None,
        token: None,
        continue_from: None,
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let run = reply["run"].as_str().unwrap();
    let outcome = w.settle(run, &[], &[], &mut BTreeSet::new()).await;
    assert_eq!(outcome["data"]["refused"][&bead], "depth 11 is over 10", "{outcome}");
    assert!(w.kids(run).is_empty());
}

/// `kill -9` while one bead has failed, one has a live run and one waits in the queue (spec §5.6, decision 10):
/// the drain's attempt fails with `daemon_crash` too, and its next attempt gives no bead of the entry again.
#[tokio::test(start_paused = true)]
async fn a_restart_mid_drain_fails_the_attempt_and_gives_no_bead_twice() {
    let config = format!("{}[[plugin]]\npath = \"policy\"\n", SWEEP[0].1);
    let mut files = vec![("config.toml", config.as_str()), POLICY[1]];
    files.extend_from_slice(&SWEEP[1..]);
    let mut w = world(&files);
    let root = w.tracker.add(&[], None);
    let [one, two, three] = [(); 3].map(|()| w.tracker.add(&["rig:web"], Some(&root)));
    let run = w.sweep(&root).await;
    let mut done = BTreeSet::new();
    w.settle(&run, &[one.as_str()], &[two.as_str()], &mut done).await;
    let states: Vec<String> = w.kids(&run).into_iter().map(|k| k.2).collect();
    assert_eq!(states, ["failed", "running", "queued"]);
    let status = aae_core::run::Reader::open(&w.dir.path().join("state.db"))
        .unwrap()
        .status()
        .unwrap();
    assert_eq!(
        status["active_runs"], 1,
        "a planned restart waits for the live child, not for the drain"
    );
    w.restart().await;
    for _ in 0..8 {
        w.driver.script(slow_turn());
    }
    let kid = w.kids(&run)[1].0.clone();
    let fail = Decision::Fail {
        reason: "lost".into(),
        gate: None,
    };
    w.decide(&kid, "run.attempt_failed", 0, fail).await;
    // The drain resumes after the child's end has freed the slot it held while interrupted.
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, resume).await;
    let outcome = w.settle(&run, &[], &[], &mut done).await;
    assert_eq!(
        outcome["data"],
        json!({ "closed": [three], "failed": [one, two], "refused": {} })
    );
    let beads: Vec<String> = w.kids(&run).into_iter().map(|k| k.1).collect();
    assert_eq!(beads, [one, two, three]);
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let sql =
        "SELECT harness, json_extract(failure, '$.kind') FROM attempts WHERE run = ?1 AND step = 'all' ORDER BY n";
    let mut stmt = db.prepare(sql).unwrap();
    let attempts: Vec<(String, Option<String>)> = (stmt.query_map([&run], |r| Ok((r.get(0)?, r.get(1)?))))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        attempts,
        [("drain".into(), Some("daemon_crash".into())), ("drain".into(), None)]
    );
}

/// A read made before a child ended may come after its end: while a later read is queued, the step goes on.
#[tokio::test(start_paused = true)]
async fn a_read_older_than_the_end_of_a_child_does_not_end_the_step() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let first = w.tracker.add(&["rig:web"], Some(&root));
    let run = w.sweep(&root).await;
    let mut done = BTreeSet::new();
    w.settle(&run, &[], &[first.as_str()], &mut done).await;
    w.tracker.set_down(true);
    // A read in flight, and its answer: made before the end of the child, it has nothing ready.
    w.changed().await;
    let kid = w.kids(&run)[0].0.clone();
    w.end_attempt(&kid, 1, Verdict::Pass).await;
    w.until(Some(&kid), "run.passed", 0).await;
    let stale = Msg::Ready {
        project: "shop".into(),
        beads: Vec::new(),
        changes: Vec::new(),
        under: [(root.clone(), Vec::new())].into_iter().collect(),
    };
    w.core.send(None, stale).await.unwrap();
    let later = w.tracker.add(&["rig:web"], Some(&root));
    w.tracker.set_down(false);
    done.insert(kid);
    let outcome = w.settle(&run, &[], &[], &mut done).await;
    assert_eq!(outcome["data"]["closed"], json!([first, later]));
}

/// A child that ends before it started touches no bead; the read it asks for still tells the drain.
#[tokio::test(start_paused = true)]
async fn a_child_cancelled_in_the_queue_ends_the_step_at_once() {
    let w = world(SWEEP);
    let gate = GateSpec {
        key: "rig:shop/api".into(),
        until_ms: None,
        reason: "paused".into(),
    };
    w.core.send(None, Msg::CloseGate(gate)).await.unwrap();
    let root = w.tracker.add(&[], None);
    let bead = w.tracker.add(&["rig:api"], Some(&root));
    let run = w.sweep(&root).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let kid = w.kids(&run)[0].clone();
    assert_eq!((&kid.1, kid.2.as_str()), (&bead, "queued"));
    let cancel = Msg::Cancel {
        run: kid.0,
        reason: "operator".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    // Well before the periodic read of the tracker.
    tokio::time::sleep(Duration::from_secs(30)).await;
    let step = w.all("run.step_finished").into_iter().find(|d| d["run"]["id"] == run);
    assert_eq!(
        step.expect("the step ended")["outcome"]["data"]["failed"],
        json!([bead])
    );
}

/// A read asked before the attempt began does not know the root: it ends nothing.
#[tokio::test(start_paused = true)]
async fn a_read_that_does_not_know_the_root_decides_nothing() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let first = w.tracker.add(&["rig:web"], Some(&root));
    let run = w.sweep(&root).await;
    let mut done = BTreeSet::new();
    w.settle(&run, &[], &[first.as_str()], &mut done).await;
    w.tracker.set_down(true);
    let kid = w.kids(&run)[0].0.clone();
    w.end_attempt(&kid, 1, Verdict::Pass).await;
    w.until(Some(&kid), "run.passed", 0).await;
    let blind = Msg::Ready {
        project: "shop".into(),
        beads: Vec::new(),
        changes: Vec::new(),
        under: BTreeMap::new(),
    };
    w.core.send(None, blind).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(w.all("run.step_finished").iter().all(|d| d["run"]["id"] != run));
    w.tracker.set_down(false);
    done.insert(kid);
    let outcome = w.settle(&run, &[], &[], &mut done).await;
    assert_eq!(outcome["data"]["closed"], json!([first]));
}

/// A bead of an epic under the root runs too; the epic, ready once its beads are closed, is given out as any bead:
/// with no `rig:` label, its run works in the project root (spec §6.1).
#[tokio::test(start_paused = true)]
async fn a_bead_of_a_nested_epic_runs_and_the_emptied_epic_runs_in_the_project_root() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let sub = w.tracker.add(&[], Some(&root));
    let deep = w.tracker.add(&["rig:web"], Some(&sub));
    let child = w.tracker.add(&["rig:api"], Some(&root));
    let run = w.sweep(&root).await;
    let outcome = w.settle(&run, &[], &[], &mut BTreeSet::new()).await;
    assert_eq!(outcome["data"]["closed"], json!([deep, child, sub]), "{outcome}");
    let rigs: Vec<(String, String)> = w.kids(&run).into_iter().map(|k| (k.1, k.3)).collect();
    assert_eq!(
        rigs,
        [(deep, "web".into()), (child, "api".into()), (sub, "root".into())]
    );
}
