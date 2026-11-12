//! Tracker writes and the ready set as events (spec §9.5, contracts 5.2): `bead.created|updated|closed` from the
//! writes after the project's cursor, `bead.ready` from the beads new to the ready set.

use super::RunHandler;
use crate::{
    tracker::{Bead, Change},
    writer::{Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

impl RunHandler {
    /// The writes to a project's tracker after its cursor become `bead.created|updated|closed`, and the beads new to
    /// its ready set `bead.ready` (spec §9.5); the ready set is kept for the next diff and for picking.
    pub(super) fn ready(tx: &mut Tx<'_>, project: &str, beads: &[Bead], changes: &[Change]) -> Result<(), Error> {
        let cursor: Option<i64> = tx
            .sql()
            .query_row("SELECT seq FROM tracker_cursors WHERE project = ?1", [project], |r| {
                r.get(0)
            })
            .optional()?;
        // A tracker read for the first time has a history, not news: its writes so far raise no events.
        let cursor = cursor.unwrap_or_else(|| changes.iter().map(|c| c.seq).max().unwrap_or_default());
        // shortcut: a bead that became ready takes the chain of the last aae write read with it (the write that
        // freed it is usually that one); a per-bead cause needs the blocker graph from the tracker.
        let mut cause = (None, None);
        for change in changes.iter().filter(|c| c.seq > cursor) {
            let by = bead_event(
                tx,
                project,
                &format!("bead.{}", change.kind),
                &change.bead,
                &change.actor,
                &change.changed,
            )?;
            if by.0.is_some() {
                cause = by;
            }
        }
        let last = changes.iter().map(|c| c.seq).max().unwrap_or(cursor);
        tx.sql().execute(
            "INSERT INTO tracker_cursors (project, seq) VALUES (?1, ?2) \
             ON CONFLICT (project) DO UPDATE SET seq = MAX(seq, ?2)",
            params![project, last],
        )?;
        let known: std::collections::HashSet<String> = tx
            .sql()
            .prepare("SELECT id FROM ready_beads WHERE project = ?1")?
            .query_map([project], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for bead in beads.iter().filter(|b| !known.contains(&b.id)) {
            emit_bead(tx, project, "bead.ready", bead, Value::Null, &[], cause.clone())?;
        }
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
}

/// A tracker write as an event; a write by a run's attempt (`aae:<run>[:<n>]`) carries that run and its correlation,
/// any other actor starts a new chain (§9.5). Returns the run and correlation it carried.
fn bead_event(
    tx: &mut Tx<'_>,
    project: &str,
    kind: &str,
    bead: &Bead,
    actor: &str,
    changed: &[String],
) -> Result<(Option<String>, Option<String>), Error> {
    let run = actor.strip_prefix("aae:").map(|r| r.split(':').next().unwrap_or(r));
    let correlation: Option<String> = match run {
        Some(run) => tx
            .sql()
            .query_row("SELECT correlation FROM runs WHERE id = ?1", [run], |r| r.get(0))
            .optional()?,
        None => None,
    };
    let by = (run.map(str::to_owned).filter(|_| correlation.is_some()), correlation);
    emit_bead(tx, project, kind, bead, json!(actor), changed, by.clone())?;
    Ok(by)
}

fn emit_bead(
    tx: &mut Tx<'_>,
    project: &str,
    kind: &str,
    bead: &Bead,
    actor: Value,
    changed: &[String],
    (run, correlation): (Option<String>, Option<String>),
) -> Result<(), Error> {
    let rig = bead
        .labels
        .iter()
        .find_map(|l| l.strip_prefix("rig:"))
        .map(str::to_owned);
    let event = Event {
        kind: kind.to_owned(),
        project: Some(project.to_owned()),
        rig,
        run,
        correlation,
        data: json!({ "bead": bead, "actor": actor, "changed": changed }),
    };
    tx.emit(&event).map(drop)
}
