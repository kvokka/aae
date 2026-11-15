//! The drain step (spec §9.1, §10.1): every ready bead under the run's bead gets a run of the step's formula, once
//! per attempt of the step, on the scripted driver and the tracker in memory.

mod common;

use aae_core::run::{Create, GateSpec, Msg, Outcome, Verdict};
use common::{World, beads::slow_turn, world};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

const SWEEP: &[(&str, &str)] = &[
    (
        "config.toml",
        "projects = [\"shop\"]\nmax_runs = 1\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n",
    ),
    (
        "shop/aae.toml",
        "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
    ),
    ("shop/api/.keep", ""),
    ("shop/agents/qa/agent.toml", "harness = \"scripted\"\n"),
    (
        "shop/formulas/work.toml",
        "name = \"work\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
         [[step]]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"work.md\"\n",
    ),
    ("shop/formulas/work.md", "Work on {{ bead.title }}."),
    (
        "shop/formulas/sweep.toml",
        "name = \"sweep\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
         [[step]]\nname = \"all\"\nkind = \"drain\"\nformula = \"work\"\n\
         [[step]]\nname = \"wrap\"\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"wrap.md\"\n",
    ),
    ("shop/formulas/wrap.md", "Wrap up: {{ steps.all.summary }}."),
];

/// A child run: `(id, bead, state, rig, agent, correlation, depth)`.
type Kid = (String, String, String, String, String, String, i64);

impl World {
    /// Starts `sweep` on `root` on rig `web`; returns its run.
    async fn sweep(&self, root: &str) -> String {
        for _ in 0..8 {
            self.driver.script(slow_turn());
        }
        let reply = self.start_bead("sweep", Some("web"), json!(root)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    fn kids(&self, parent: &str) -> Vec<Kid> {
        let db = rusqlite::Connection::open(self.dir.path().join("state.db")).unwrap();
        let mut stmt = db
            .prepare(
                "SELECT id, bead, state, rig, agent, correlation, depth FROM runs WHERE parent_run = ?1 ORDER BY n",
            )
            .unwrap();
        let rows = stmt.query_map([parent], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        });
        rows.unwrap().map(Result::unwrap).collect()
    }

    async fn finish(&self, run: &str, attempt: u32, result: Verdict) {
        let token = self.driver.token(run, attempt).unwrap();
        let outcome = Outcome {
            result,
            summary: "done".into(),
            data: None,
        };
        let msg = Msg::Finish {
            run: run.into(),
            token,
            outcome,
        };
        self.core.send(None, msg).await.unwrap();
    }

    /// Finishes each child of `root` once its session has started — `fail` on the beads in `failing` — until the
    /// drain step of `root` has finished (`stop` beads: until a child on each of them is alive, unfinished); returns
    /// the outcome of the step.
    async fn settle(&self, root: &str, failing: &[&str], stop: &[&str], done: &mut BTreeSet<String>) -> Value {
        for _ in 0..300 {
            let step = self
                .all("run.step_finished")
                .into_iter()
                .find(|d| d["run"]["id"] == root);
            if let Some(step) = step {
                return step["outcome"].clone();
            }
            let kids = self.kids(root);
            let alive = |b: &&str| kids.iter().any(|k| k.1 == *b && k.2 == "running");
            if !stop.is_empty() && stop.iter().all(alive) {
                return Value::Null;
            }
            for (id, bead, ..) in kids {
                let skip = stop.contains(&bead.as_str()) || done.contains(&id);
                if !skip && self.driver.token(&id, 1).is_some() {
                    let verdict = if failing.contains(&bead.as_str()) {
                        Verdict::Fail
                    } else {
                        Verdict::Pass
                    };
                    self.finish(&id, 1, verdict).await;
                    done.insert(id);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        panic!("the drain of {root} did not settle: {:?}", self.kids(root));
    }

    /// Finishes the step after the drain and waits for `root` to pass.
    async fn wrap(&self, root: &str) -> String {
        for _ in 0..60 {
            if self.driver.token(root, 2).is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        self.finish(root, 2, Verdict::Pass).await;
        self.until(Some(root), "run.passed", 0).await;
        self.driver
            .sent(root, 2)
            .1
            .into_iter()
            .find(|t| t.starts_with("Wrap"))
            .unwrap()
    }
}

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
    let none = w.tracker.add(&[], Some(&root));
    let run = w.sweep(&root).await;
    let outcome = w.settle(&run, &[], &[], &mut BTreeSet::new()).await;
    let refused = &outcome["data"]["refused"];
    let why = refused[&codex].as_str().unwrap();
    assert!(
        why.contains("labelled `harness:codex`: the config gives `scripted` to `shop.dev`"),
        "{why}"
    );
    let why = refused[&none].as_str().unwrap();
    assert!(why.contains("has no `rig:` label"), "{why}");
    assert!(w.kids(&run).is_empty());
    for bead in [codex, none] {
        assert_eq!(w.tracker.bead(&bead).unwrap().assignee, None);
    }
    assert_eq!(w.wrap(&run).await, "Wrap up: 0 closed, 0 failed, 2 refused.");
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

/// `kill -9` while one bead is done, one has a live run and one waits in the queue (decision 10).
#[tokio::test(start_paused = true)]
async fn a_restart_mid_drain_keeps_the_attempt_and_gives_no_bead_twice() {
    let mut w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let [one, two, three] = [(); 3].map(|()| w.tracker.add(&["rig:web"], Some(&root)));
    let run = w.sweep(&root).await;
    let mut done = BTreeSet::new();
    w.settle(&run, &[], &[two.as_str()], &mut done).await;
    let states: Vec<String> = w.kids(&run).into_iter().map(|k| k.2).collect();
    assert_eq!(states, ["passed", "running", "queued"]);
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
    let outcome = w.settle(&run, &[], &[], &mut done).await;
    // The live run's attempt was lost and no policy decides: it fails, and its bead is not given again.
    assert_eq!(
        outcome["data"],
        json!({ "closed": [one, three], "failed": [two], "refused": {} })
    );
    let beads: Vec<String> = w.kids(&run).into_iter().map(|k| k.1).collect();
    assert_eq!(beads, [one, two, three]);
    let events = w.events(&run, Duration::ZERO).await;
    assert!(events.iter().all(|(t, _)| t != "run.attempt_failed"));
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let sql = "SELECT COUNT(*), MIN(harness) FROM attempts WHERE run = ?1 AND step = 'all'";
    let attempts: (i64, String) = db.query_row(sql, [&run], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(attempts, (1, "drain".to_owned()), "one attempt of the drain step");
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
    w.finish(&kid, 1, Verdict::Pass).await;
    w.until(Some(&kid), "run.passed", 0).await;
    let stale = Msg::Ready {
        project: "shop".into(),
        beads: Vec::new(),
        changes: Vec::new(),
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
