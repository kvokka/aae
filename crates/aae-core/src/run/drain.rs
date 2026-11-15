//! The root bead of a run started from a request (spec §10.1): a bead input `{ request = "<text>" }` makes the
//! tracker create an epic for the run as it prepares, once per key, and the run then claims it as any bead.
//!
//! The drain step (spec §9.1): an attempt with no process that, on every read of the tracker, gives each ready bead
//! under the run's bead a run of the step's formula — the one creation every start goes through — once per attempt.

use super::{
    Create, Outcome, RunHandler, Verdict,
    beads::{sync, tracker_key},
    new_token, rejected,
    render::DRAIN_TIMEOUT_MS,
    row::Row,
    triggers::MAX_DEPTH,
};
use crate::{
    config::InputType,
    tracker::{Bead, TrackerError},
    writer::{Error, Tx},
};
use rusqlite::params;
use serde_json::{Map, Value, json};

/// The longest title of a root bead, in characters.
const TITLE_CHARS: usize = 80;

/// A run in a drain step: its attempt has no process, so it holds no slot, does not hold the workdir from its own
/// children and is not interrupted by a restart.
pub(super) const DRAINING: &str = "(step_kind = 'drain' AND state = 'running')";

impl RunHandler {
    /// The attempt of a drain step: the read of the tracker it asks for makes the first pass; its deadline is the
    /// step's timeout.
    pub(super) fn start_drain(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        if row.bead.is_none() || !self.trackers.contains_key(&row.project) {
            let reason = "a drain step needs the bead of the run";
            return self.terminal(tx, row, "failed", Some("infrastructure"), reason);
        }
        Self::run_attempt(tx, &mut row, "drain")?;
        let dedupe = format!("sync:drain:{}:{}", row.id, row.attempt);
        tx.effect(&tracker_key(&row.project), &dedupe, &sync(tx, &row.project)?)?;
        let timeout_ms = row.timeout_ms.unwrap_or(DRAIN_TIMEOUT_MS);
        tx.set_deadline(&row.deadline("state"), tx.now_ms().saturating_add(timeout_ms))
    }

    /// One pass of every run of `project` in a drain step, after a read of its tracker.
    pub(super) fn drained(&mut self, tx: &mut Tx<'_>, project: &str) -> Result<(), Error> {
        let runs: Vec<String> = {
            let sql = format!("SELECT id FROM runs WHERE project = ?1 AND {DRAINING} ORDER BY n");
            let mut stmt = tx.sql().prepare(&sql)?;
            stmt.query_map([project], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in runs {
            self.pass(tx, Row::load(tx, &id)?)?;
        }
        Ok(())
    }

    /// Gives each ready bead under the run's bead that this attempt has not given yet a run of the step's formula;
    /// a refused bead gets no run and stays open. The step passes once nothing was given, no child is alive and no
    /// later read is queued, with `{closed, failed, refused}` of the beads of this attempt.
    fn pass(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
        let Some(root) = row.bead.clone().filter(|_| row.state == "running") else {
            return Ok(());
        };
        let config: Value = (row.config.as_deref())
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        let formula = config["drain"].as_str().unwrap_or_default().to_owned();
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let input = (project.and_then(|p| p.formula(&formula).ok()))
            .and_then(|(_, f)| f.inputs.iter().find(|(_, i)| i.kind == InputType::Bead))
            .map(|(k, _)| k.clone());
        let Some(input) = input else {
            let reason = format!("formula `{formula}` of the drain step is gone or takes no bead");
            return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
        };
        // `(bead, state, given by this attempt)` of every child on a bead.
        let kids: Vec<(String, String, bool)> = {
            let mut stmt = tx.sql().prepare(
                "SELECT r.bead, r.state, r.created_ms >= a.started_ms FROM runs r JOIN attempts a \
                 ON a.run = ?1 AND a.n = ?2 WHERE r.parent_run = ?1 AND r.bead IS NOT NULL ORDER BY r.n",
            )?;
            let rows = stmt.query_map(params![row.id, row.attempt], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let depth = row.depth + 1;
        let (mut given, mut refused) = (0, Map::new());
        for bead in Self::ready_in(tx, &row.project, Some(&root), None)? {
            if kids.iter().any(|(b, _, mine)| *mine && *b == bead) {
                continue;
            }
            if depth > MAX_DEPTH {
                refused.insert(bead, json!(format!("depth {depth} is over {MAX_DEPTH}")));
                continue;
            }
            let create = Create {
                project: row.project.clone(),
                // The rig of the bead's `rig:` label.
                rig: None,
                formula: formula.clone(),
                inputs: [(input.clone(), json!(bead))].into_iter().collect(),
                correlation: Some(row.correlation.clone()),
                depth: u32::try_from(depth).unwrap_or(u32::MAX),
                parent_run: Some(row.id.clone()),
                token: None,
                continue_from: None,
            };
            match tx.unit(|tx| self.create(tx, create)) {
                Ok(_) => given += 1,
                Err(Error::Rejected(why)) => drop(refused.insert(bead, json!(why))),
                Err(e) => return Err(e),
            }
        }
        let alive = (kids.iter()).any(|(_, state, _)| !matches!(state.as_str(), "passed" | "failed" | "cancelled"));
        // The read in hand is still in the queue. Another one behind it may bring a bead the end of a child made
        // ready (one the child's bead blocked): the step ends on the last read.
        let reads: i64 = tx.sql().query_row(
            "SELECT COUNT(*) FROM effects WHERE key = ?1 AND dedupe LIKE 'sync:%'",
            [tracker_key(&row.project)],
            |r| r.get(0),
        )?;
        if given > 0 || alive || reads > 1 {
            return Ok(());
        }
        let of = |states: &[&str]| -> Vec<&String> {
            (kids.iter())
                .filter(|(_, state, mine)| *mine && states.contains(&state.as_str()))
                .map(|(bead, ..)| bead)
                .collect()
        };
        let (closed, failed) = (of(&["passed"]), of(&["failed", "cancelled"]));
        let outcome = Outcome {
            result: Verdict::Pass,
            summary: format!(
                "{} closed, {} failed, {} refused",
                closed.len(),
                failed.len(),
                refused.len()
            ),
            data: Some(json!({ "closed": closed, "failed": failed, "refused": refused })),
        };
        self.step_done(tx, row, &outcome)
    }

    /// The text of a bead input of the form `{ request = "<text>" }`; `None`: another form. A request with another
    /// key, or with no text, is refused.
    pub(super) fn request(input: &Value) -> Result<Option<&str>, Error> {
        let Some(object) = input.as_object().filter(|o| o.contains_key("request")) else {
            return Ok(None);
        };
        match object["request"].as_str() {
            Some(text) if object.len() == 1 && !text.trim().is_empty() => Ok(Some(text)),
            _ => Err(rejected(
                "a bead request is `{ request = \"<text>\" }` with a text that is not blank",
            )),
        }
    }

    /// Queues the creation of the root bead of a run started from `text`; the run waits in `preparing` for it, as
    /// for a claim. The key is in the payload: a repeat of the effect after a crash creates no second bead.
    pub(super) fn create_root(&mut self, tx: &mut Tx<'_>, row: Row, text: &str) -> Result<(), Error> {
        let Some(token) = new_token() else {
            return self.terminal(
                tx,
                row,
                "failed",
                Some("infrastructure"),
                "bead_not_created: no randomness",
            );
        };
        let title: String = (text.lines().map(str::trim).find(|l| !l.is_empty()))
            .unwrap_or_default()
            .chars()
            .take(TITLE_CHARS)
            .collect();
        let payload = json!({ "op": "tracker", "call": "create", "project": row.project, "run": row.id,
            "key": format!("aae-{}", &token[..16]), "title": title, "description": text });
        // shortcut: a tracker that answers after `start_timeout` leaves a root without a run; adopt or close it once
        // an operator needs that.
        tx.effect(&tracker_key(&row.project), &format!("create:{}", row.id), &payload)?;
        tx.set_deadline(
            &row.deadline("state"),
            tx.now_ms() + self.config.watchdog.start_timeout_ms,
        )
    }

    /// The answer of the tracker to the creation of the root bead of `run`: the run owns and claims the bead, its
    /// input names it. A repeated answer, or one for a run no longer preparing, is dropped.
    pub(super) fn created(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        result: Result<Option<Bead>, TrackerError>,
    ) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        if row.state != "preparing" || row.bead.is_some() {
            return Ok(());
        }
        let bead = match result {
            Ok(Some(bead)) => bead.id,
            Ok(None) => return Ok(()),
            Err(e) => {
                let reason = format!("bead_not_created: {e}");
                return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
            }
        };
        match Self::own_bead(tx, &row.project, &bead, &row.id) {
            Err(Error::Rejected(reason)) => {
                return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
            }
            owned => owned?,
        }
        let mut inputs: Value = serde_json::from_str(&row.inputs).unwrap_or_default();
        if let Some((_, input)) = requested(&mut inputs) {
            *input = json!(bead);
        }
        tx.sql().execute(
            "UPDATE runs SET bead = ?2, inputs = ?3 WHERE id = ?1",
            params![row.id, bead, inputs.to_string()],
        )?;
        self.claim(tx, &Row::load(tx, run)?, &bead)
    }
}

/// The text of the request among the inputs of a run (as JSON), and its input; only a bead input holds an object.
pub(super) fn requested(inputs: &mut Value) -> Option<(String, &mut Value)> {
    let object = inputs.as_object_mut()?;
    let input = object.values_mut().find(|v| v.get("request").is_some())?;
    Some((input["request"].as_str()?.to_owned(), input))
}
