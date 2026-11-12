//! Bead ownership (spec §10.3, invariants 2–5, docs/design/contracts.md 5): the core records the owner run of a bead
//! when it decides to run on it, claims it in the tracker as the run prepares, and closes or conditionally releases
//! it when the run ends. The tracker calls are effects on the queue `tracker:<project>`, so the calls of one project
//! keep their order.

use super::{GateSpec, Msg, RunHandler, rejected, row::Row};
use crate::{
    tracker::{Bead, Tracker, TrackerError},
    writer::{CoreHandle, Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

pub(super) type Trackers = Arc<BTreeMap<String, Arc<dyn Tracker>>>;

impl RunHandler {
    /// The bead a run created with `input` works on: the bead id it names, or the first ready bead (in the tracker's
    /// order) that is in `epic`, belongs to `rig` (or to no rig), has no owner and no closed gate. `None`: nothing is
    /// ready (§10.3).
    pub(super) fn pick_bead(tx: &Tx<'_>, project: &str, rig: &str, input: &Value) -> Result<Option<String>, Error> {
        if let Some(id) = input.as_str() {
            return Ok(Some(id.to_owned()));
        }
        if input["next_ready"] != json!(true)
            || input
                .as_object()
                .is_some_and(|o| o.keys().any(|k| k != "next_ready" && k != "epic"))
        {
            return Err(rejected(
                "a bead input is a bead id or `{ next_ready = true, epic = <id> }`",
            ));
        }
        let epic = input["epic"].as_str();
        let mut stmt = tx.sql().prepare(
            "SELECT r.id, r.body FROM ready_beads r WHERE r.project = ?1 AND NOT EXISTS \
             (SELECT 1 FROM beads b WHERE b.project = r.project AND b.id = r.id AND b.owner IS NOT NULL) ORDER BY r.pos",
        )?;
        let ready = stmt.query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Value>(1)?)))?;
        let rig_label = format!("rig:{rig}");
        for bead in ready {
            let (id, body) = bead?;
            let labels: Vec<&str> = body["labels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let other_rig = labels.iter().any(|l| l.starts_with("rig:")) && !labels.contains(&rig_label.as_str());
            if epic.is_some_and(|e| body["epic"] != e) || other_rig {
                continue;
            }
            let key = [format!("bead:{project}/{id}")];
            if super::gates::closures(tx.sql(), tx.now_ms(), Some(&key))?.is_empty() {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Records `run` as the one live owner of `bead` with a new generation (invariants 2, 3); a held bead refuses.
    pub(super) fn own_bead(tx: &Tx<'_>, project: &str, bead: &str, run: &str) -> Result<(), Error> {
        let owner: Option<Option<String>> = tx
            .sql()
            .query_row(
                "SELECT owner FROM beads WHERE project = ?1 AND id = ?2",
                params![project, bead],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(Some(owner)) = owner {
            return Err(rejected(format!("bead `{bead}` is held by `{owner}`")));
        }
        tx.sql().execute(
            "INSERT INTO beads (project, id, owner, generation) VALUES (?1, ?2, ?3, 1) \
             ON CONFLICT (project, id) DO UPDATE SET owner = ?3, generation = generation + 1",
            params![project, bead, run],
        )?;
        Ok(())
    }

    /// Queues the claim of the run's bead in the tracker; the run waits in `preparing` for the bead it returns.
    pub(super) fn claim(tx: &mut Tx<'_>, row: &Row, bead: &str) -> Result<(), Error> {
        let generation: i64 = tx.sql().query_row(
            "SELECT generation FROM beads WHERE project = ?1 AND id = ?2",
            params![row.project, bead],
            |r| r.get(0),
        )?;
        let payload = json!({ "op": "tracker", "call": "claim", "project": row.project, "bead": bead, "run": row.id,
            "generation": generation });
        tx.effect(&tracker_key(&row.project), &format!("claim:{}", row.id), &payload)?;
        tx.set_deadline(&row.deadline("state"), tx.now_ms() + super::machine::START_TIMEOUT_MS)
    }

    /// The result of a tracker call (`payload` is its effect).
    pub(super) fn tracked(
        &mut self,
        tx: &mut Tx<'_>,
        payload: &Value,
        result: Result<Option<Bead>, TrackerError>,
    ) -> Result<(), Error> {
        let text = |key: &str| payload[key].as_str().unwrap_or_default().to_owned();
        let (call, project, bead, run) = (text("call"), text("project"), text("bead"), text("run"));
        let current: Option<(Option<String>, i64)> = tx
            .sql()
            .query_row(
                "SELECT owner, generation FROM beads WHERE project = ?1 AND id = ?2",
                params![project, bead],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        // A claim answers only the ownership it was made for; a stale one is dropped (invariant 2).
        let owns = current == Some((Some(run.clone()), payload["generation"].as_i64().unwrap_or_default()));
        let event = |kind: &str, data: Value| Event {
            kind: kind.to_owned(),
            project: Some(project.clone()),
            rig: None,
            run: Some(run.clone()).filter(|r| !r.is_empty()),
            correlation: None,
            data,
        };
        match (call.as_str(), result) {
            ("claim", Ok(Some(data))) if owns => {
                let row = Row::load(tx, &run)?;
                if row.state != "preparing" {
                    return Ok(());
                }
                tx.clear_deadline(&row.deadline("state"))?;
                tx.sql().execute(
                    "UPDATE runs SET bead_data = ?2 WHERE id = ?1",
                    params![run, json!(data).to_string()],
                )?;
                self.render_and_prepare(tx, Row::load(tx, &run)?)
            }
            (
                "claim",
                Err(e @ (TrackerError::Claimed { .. } | TrackerError::NotFound | TrackerError::Invalid { .. })),
            ) if owns => {
                let row = Row::load(tx, &run)?;
                let reason = match e {
                    TrackerError::Claimed { .. } => "bead_claimed".to_owned(),
                    TrackerError::NotFound => "bead_not_found".to_owned(),
                    other => format!("bead_invalid: {other}"),
                };
                if row.state != "preparing" {
                    return Ok(());
                }
                // The refused bead was never ours: dropping the ownership here tells `let_go_bead` to leave it.
                tx.sql().execute(
                    "UPDATE beads SET owner = NULL WHERE project = ?1 AND id = ?2",
                    params![project, bead],
                )?;
                self.terminal(tx, row, "failed", Some("infrastructure"), &reason)
            }
            (_, Err(e @ (TrackerError::Unavailable { .. } | TrackerError::Invalid { .. }))) => {
                // The effect in flight heads its queue; it records this failure after the message, and only an
                // unavailable tracker is tried again.
                let failures: Option<i64> = tx
                    .sql()
                    .query_row(
                        "SELECT attempts FROM effects WHERE key = ?1 ORDER BY id LIMIT 1",
                        [tracker_key(&project)],
                        |r| r.get(0),
                    )
                    .optional()?;
                let retry = matches!(e, TrackerError::Unavailable { .. });
                let next = (failures.filter(|_| retry))
                    .map(|n| crate::journal::rfc3339_ms(tx.now_ms() + crate::effects::backoff_ms(n)));
                let data = json!({ "op": call, "bead": Some(bead.clone()).filter(|b| !b.is_empty()),
                    "error": e.to_string(), "next_retry_at": next });
                tx.emit(&event("tracker.sync_failed", data)).map(drop)
            }
            ("close" | "release", Err(e)) => {
                let observed = match e {
                    TrackerError::OwnerMismatch { holder } => json!(holder),
                    other => json!(other.to_string()),
                };
                let data = json!({ "subject": "bead", "id": bead, "expected": format!("aae:{run}"),
                    "observed": observed });
                tx.emit(&event("resync.diverged", data)).map(drop)
            }
            _ => Ok(()),
        }
    }

    /// The last ready set of a project's tracker.
    pub(super) fn ready(tx: &Tx<'_>, project: &str, beads: &[Bead]) -> Result<(), Error> {
        tx.sql()
            .execute("DELETE FROM ready_beads WHERE project = ?1", [project])?;
        for (pos, bead) in (0_i64..).zip(beads) {
            tx.sql().execute(
                "INSERT INTO ready_beads (project, pos, id, body) VALUES (?1, ?2, ?3, ?4)",
                params![project, pos, bead.id, json!(bead).to_string()],
            )?;
        }
        Ok(())
    }

    /// The run ended: its hold on the bead goes; a claimed bead is closed (`passed`) or released conditionally, and
    /// its failure counter moves (§10.3): `bead_failures` failed runs close the bead's gate for the core.
    pub(super) fn let_go_bead(&mut self, tx: &mut Tx<'_>, row: &Row, state: &str, reason: &str) -> Result<(), Error> {
        let Some(bead) = &row.bead else { return Ok(()) };
        let owned = tx.sql().execute(
            "UPDATE beads SET owner = NULL WHERE project = ?1 AND id = ?2 AND owner = ?3",
            params![row.project, bead, row.id],
        )? > 0;
        // The claim was asked for once the run prepared; a refused claim has already dropped the ownership.
        if row.started_ms.is_none() || !owned {
            return Ok(());
        }
        let call = if state == "passed" { "close" } else { "release" };
        let payload = json!({ "op": "tracker", "call": call, "project": row.project, "bead": bead, "run": row.id,
            "reason": format!("{state}: {reason}") });
        let key = tracker_key(&row.project);
        tx.effect(&key, &format!("{call}:{}", row.id), &payload)?;
        tx.effect(&key, &format!("sync:{call}:{}", row.id), &sync(&row.project))?;
        let failures = match state {
            "passed" => {
                tx.sql().execute(
                    "DELETE FROM ready_beads WHERE project = ?1 AND id = ?2",
                    params![row.project, bead],
                )?;
                0
            }
            // Only a run that held the claim counts; a claim that never answered says nothing of the bead.
            "failed" if row.bead_data.is_some() => 1,
            _ => return Ok(()),
        };
        let total: u32 = tx.sql().query_row(
            // `?3` is 0 (a pass resets the count) or 1 (a failure adds one).
            "UPDATE beads SET failures = (failures + ?3) * ?3 WHERE project = ?1 AND id = ?2 RETURNING failures",
            params![row.project, bead, failures],
            |r| r.get(0),
        )?;
        if total < self.config.bead_failures {
            return Ok(());
        }
        let gate = GateSpec {
            key: format!("bead:{}/{bead}", row.project),
            until_ms: None,
            reason: format!("{total} failed runs on the bead"),
        };
        self.close_gate(tx, &gate, "core")
    }

    /// The operator opened a bead's gate: its failures start from zero.
    pub(super) fn forgive_bead(tx: &Tx<'_>, key: &str) -> Result<(), Error> {
        if let Some((project, bead)) = key.strip_prefix("bead:").and_then(|k| k.split_once('/')) {
            tx.sql().execute(
                "UPDATE beads SET failures = 0 WHERE project = ?1 AND id = ?2",
                params![project, bead],
            )?;
        }
        Ok(())
    }

    /// Reads the ready set of every project's tracker again (on start, and when a tracker says it changed).
    pub(super) fn sync_trackers(&self, tx: &mut Tx<'_>, project: Option<&str>, why: &str) -> Result<(), Error> {
        let projects = self.trackers.keys().filter(|p| project.is_none_or(|q| q == *p));
        for project in projects {
            tx.effect(
                &tracker_key(project),
                &format!("sync:{why}:{}", tx.now_ms()),
                &sync(project),
            )?;
        }
        Ok(())
    }
}

fn tracker_key(project: &str) -> String {
    format!("tracker:{project}")
}

fn sync(project: &str) -> Value {
    json!({ "op": "tracker", "call": "sync", "project": project })
}

/// Runs one tracker call of an effect and reports it; `false` retries it (the tracker was unavailable).
pub(super) async fn execute(trackers: &Trackers, core: &CoreHandle<Msg>, payload: &Value) -> bool {
    let text = |key: &str| payload[key].as_str().unwrap_or_default();
    let Some(tracker) = trackers.get(text("project")) else {
        return true;
    };
    let (owner, bead) = (format!("aae:{}", text("run")), text("bead"));
    let result = match text("call") {
        "claim" => tracker.claim(&owner, bead).await.map(Some),
        "close" => tracker.close(&owner, bead, text("reason")).await.map(|()| None),
        "release" => tracker.release(&owner, bead).await.map(|()| None),
        _ => match tracker.ready().await {
            Ok(beads) => {
                let project = text("project").to_owned();
                // A stopped writer drops the message; the next start reads the ready set again.
                let _ = core.send(None, Msg::Ready { project, beads }).await;
                return true;
            }
            Err(e) => Err(e),
        },
    };
    let retry = matches!(result, Err(TrackerError::Unavailable { .. }));
    let _ = core
        .send(
            None,
            Msg::Tracked {
                payload: payload.clone(),
                result,
            },
        )
        .await;
    !retry
}
