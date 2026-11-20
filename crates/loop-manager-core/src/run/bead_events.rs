//! Tracker writes and the ready set as events (docs/design/config.md, contracts 5.2): `bead.created|updated|closed`
//! from the writes after the project's cursor, `bead.labeled` from the labels a known bead gained, `bead.ready` from
//! the beads new to the ready set.

use super::RunHandler;
use crate::{
    config::Project,
    tracker::{Bead, Change},
    writer::{Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params, types::Type};
use serde_json::{Value, json};
use std::collections::HashSet;

impl RunHandler {
    /// The writes to a project's tracker after its cursor become `bead.created|updated|closed`, a label a known bead
    /// gained `bead.labeled`, and the beads new to its ready set `bead.ready` (docs/design/config.md); the ready set
    /// is kept for the next diff and for picking, the labels of every bead read for the next `bead.labeled`.
    pub(super) fn ready(
        &self,
        tx: &mut Tx<'_>,
        project: &str,
        beads: &[Bead],
        changes: &[Change],
    ) -> Result<(), Error> {
        // A project gone from the config still has its tracker read; its beads then name no rig.
        let place = (project, self.config.projects.iter().find(|p| p.name == project));
        let cursor: Option<i64> = tx
            .sql()
            .query_row("SELECT seq FROM tracker_cursors WHERE project = ?1", [project], |r| {
                r.get(0)
            })
            .optional()?;
        // A tracker read for the first time has a history, not news: its writes so far raise no events, and give the
        // labels a later `bead.labeled` is told from.
        let first = cursor.is_none();
        let cursor = cursor.unwrap_or_else(|| changes.iter().map(|c| c.seq).max().unwrap_or_default());
        let known: HashSet<String> = tx
            .sql()
            .prepare("SELECT id FROM ready_beads WHERE project = ?1")?
            .query_map([project], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        // Ready for `bead.labeled`: in the set before this read and still in it; a bead that enters it now raises
        // `bead.ready` with its labels instead.
        let stays: HashSet<&str> = (beads.iter())
            .filter(|b| known.contains(&b.id))
            .map(|b| b.id.as_str())
            .collect();
        // shortcut: a bead that became ready takes the chain of the last loop-manager write read with it (the write that
        // freed it is usually that one); a per-bead cause needs the blocker graph from the tracker.
        let mut cause = (None, None);
        for change in changes {
            // Not news: the history of a first read, or the writes a read queued before the last one was made repeats.
            if change.seq <= cursor {
                if first {
                    remember_labels(tx, project, &change.bead)?;
                }
                continue;
            }
            let before = remember_labels(tx, project, &change.bead)?;
            let by = bead_event(tx, place, change)?;
            // A bead never read gains nothing: its labels are those it was filed with.
            if let Some(before) = before {
                let ready = stays.contains(change.bead.id.as_str());
                for label in change.bead.labels.iter().filter(|l| !before.contains(l)) {
                    let data = json!({ "bead": change.bead, "actor": change.actor, "label": label, "ready": ready });
                    emit_bead(tx, place, "bead.labeled", &change.bead.id, data, by.clone())?;
                }
            }
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
        for bead in beads {
            // A ready bead whose write is not read yet (the ready set is read after the writes) is known by its labels
            // from here on.
            tx.sql().execute(
                "INSERT OR IGNORE INTO bead_labels (project, id, labels) VALUES (?1, ?2, ?3)",
                params![project, bead.id, json!(bead.labels).to_string()],
            )?;
            if !known.contains(&bead.id) {
                let data = json!({ "bead": bead, "actor": null, "changed": [] });
                emit_bead(tx, place, "bead.ready", &bead.id, data, cause.clone())?;
            }
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

/// Keeps the labels `bead` has now and returns those it had when last read; `None`: it was never read.
fn remember_labels(tx: &Tx<'_>, project: &str, bead: &Bead) -> Result<Option<Vec<String>>, Error> {
    // Labels it cannot read are an error of the state, not a bead with none: each would be told as gained.
    let before: Option<Vec<String>> = tx
        .sql()
        .query_row(
            "SELECT labels FROM bead_labels WHERE project = ?1 AND id = ?2",
            [project, &bead.id],
            |r| {
                let text: String = r.get(0)?;
                serde_json::from_str(&text)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))
            },
        )
        .optional()?;
    tx.sql().execute(
        "INSERT INTO bead_labels (project, id, labels) VALUES (?1, ?2, ?3) \
         ON CONFLICT (project, id) DO UPDATE SET labels = ?3",
        params![project, bead.id, json!(bead.labels).to_string()],
    )?;
    Ok(before)
}

/// A tracker write as an event; a write by a run's attempt (`loop-manager:<run>[:<n>]`) carries that run and its correlation,
/// any other actor starts a new chain (docs/design/config.md). Returns the run and correlation it carried.
fn bead_event(
    tx: &mut Tx<'_>,
    place: (&str, Option<&Project>),
    change: &Change,
) -> Result<(Option<String>, Option<String>), Error> {
    let run = (change.actor)
        .strip_prefix("loop-manager:")
        .map(|r| r.split(':').next().unwrap_or(r));
    let correlation: Option<String> = match run {
        Some(run) => tx
            .sql()
            .query_row("SELECT correlation FROM runs WHERE id = ?1", [run], |r| r.get(0))
            .optional()?,
        None => None,
    };
    let by = (run.map(str::to_owned).filter(|_| correlation.is_some()), correlation);
    let data = json!({ "bead": change.bead, "actor": change.actor, "changed": change.changed });
    emit_bead(
        tx,
        place,
        &format!("bead.{}", change.kind),
        &change.bead.id,
        data,
        by.clone(),
    )?;
    Ok(by)
}

/// `place`: the project's name and config; the rig of the event is the rig of the bead's id
/// (docs/design/config.md).
fn emit_bead(
    tx: &mut Tx<'_>,
    (project, config): (&str, Option<&Project>),
    kind: &str,
    bead: &str,
    data: Value,
    (run, correlation): (Option<String>, Option<String>),
) -> Result<(), Error> {
    let rig = config.and_then(|p| p.rig_of(bead)).map(str::to_owned);
    let event = Event {
        kind: kind.to_owned(),
        project: Some(project.to_owned()),
        rig,
        run,
        correlation,
        data,
    };
    tx.emit(&event).map(drop)
}
