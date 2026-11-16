//! Helpers of the drain tests: a formula whose first step drains the run's bead into runs of `work`, the children of
//! a run, and their ends.

use super::{World, beads::slow_turn};
use aae_core::run::{Msg, Outcome, Verdict};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

pub(crate) const SWEEP: &[(&str, &str)] = &[
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
         [[step]]\nkind = \"ai\"\nagent = \"dev\"\nprompt = \"work.md\"\n",
    ),
    ("shop/formulas/work.md", "Work on {{ bead.title }}."),
    (
        "shop/formulas/sweep.toml",
        "name = \"sweep\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
         [[step]]\nname = \"all\"\nkind = \"drain\"\nformula = \"work\"\n\
         [[step]]\nname = \"wrap\"\nkind = \"ai\"\nagent = \"dev\"\nprompt = \"wrap.md\"\n",
    ),
    ("shop/formulas/wrap.md", "Wrap up: {{ steps.all.summary }}."),
];

/// A child run: `(id, bead, state, rig, agent, correlation, depth)`.
pub(crate) type Kid = (String, String, String, String, String, String, i64);

impl World {
    /// Starts `sweep` on `root` on rig `web`; returns its run.
    pub(crate) async fn sweep(&self, root: &str) -> String {
        for _ in 0..8 {
            self.driver.script(slow_turn());
        }
        let reply = self.start_bead("sweep", Some("web"), json!(root)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    pub(crate) fn kids(&self, parent: &str) -> Vec<Kid> {
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

    pub(crate) async fn end_attempt(&self, run: &str, attempt: u32, result: Verdict) {
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
    pub(crate) async fn settle(
        &self,
        root: &str,
        failing: &[&str],
        stop: &[&str],
        done: &mut BTreeSet<String>,
    ) -> Value {
        self.settle_nth(root, 0, failing, stop, done).await
    }

    /// [`World::settle`] up to the `n`-th end of a step of `root`, counted from 0.
    pub(crate) async fn settle_nth(
        &self,
        root: &str,
        n: usize,
        failing: &[&str],
        stop: &[&str],
        done: &mut BTreeSet<String>,
    ) -> Value {
        for _ in 0..300 {
            let step = self
                .all("run.step_finished")
                .into_iter()
                .filter(|d| d["run"]["id"] == root)
                .nth(n);
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
                    self.end_attempt(&id, 1, verdict).await;
                    done.insert(id);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        panic!("the drain of {root} did not settle: {:?}", self.kids(root));
    }

    /// Finishes the step after the drain and waits for `root` to pass.
    pub(crate) async fn wrap(&self, root: &str) -> String {
        for _ in 0..60 {
            if self.driver.token(root, 2).is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        self.end_attempt(root, 2, Verdict::Pass).await;
        self.until(Some(root), "run.passed", 0).await;
        self.driver
            .sent(root, 2)
            .1
            .into_iter()
            .find(|t| t.starts_with("Wrap"))
            .unwrap()
    }
}
