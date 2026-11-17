//! The formula step (docs/design/config.md, docs/design/contracts.md 6): an attempt with no process whose step is a run
//! of another formula of the project, a child of the run on its rig, created once per entry of the step; its end is the
//! step's outcome.

use super::{Create, Outcome, RunHandler, Verdict, row::Row, triggers::MAX_DEPTH};
use crate::writer::{Error, Tx};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

impl RunHandler {
    /// The attempt of a formula step: the child of this entry if an earlier attempt made it (a restart cut that
    /// attempt), else a new one; its deadline is the step's timeout.
    pub(super) fn start_formula(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        Self::run_attempt(tx, &mut row, "formula")?;
        let timeout_ms = row.timeout_ms.unwrap_or(super::render::DRAIN_TIMEOUT_MS);
        tx.set_deadline(&row.deadline("state"), tx.now_ms().saturating_add(timeout_ms))?;
        if let Some(child) = Self::entry_child(tx, &row)? {
            return self.child_ended(tx, &row.id, &child);
        }
        let config: Value = (row.config.as_deref())
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        let depth = row.depth + 1;
        if depth > MAX_DEPTH {
            let why = format!("depth {depth} is over {MAX_DEPTH}");
            return self.terminal(tx, row, "failed", Some("infrastructure"), &why);
        }
        let create = Create {
            project: row.project.clone(),
            rig: Some(row.rig.clone()),
            formula: config["formula"].as_str().unwrap_or_default().to_owned(),
            inputs: config["inputs"].as_object().cloned().unwrap_or_default(),
            correlation: Some(row.correlation.clone()),
            depth: u32::try_from(depth).unwrap_or(u32::MAX),
            parent_run: Some(row.id.clone()),
            token: None,
            continue_from: None,
        };
        // A child refused, or with no bead ready to take, is no work of the formula: nothing ran (invariant 13).
        match tx.unit(|tx| self.create(tx, create)) {
            Ok(reply) if reply["run"].is_null() => {
                self.terminal(tx, row, "failed", Some("infrastructure"), "nothing_ready")
            }
            Ok(_) => Ok(()),
            Err(Error::Rejected(why) | Error::NotFound(why)) => {
                self.terminal(tx, row, "failed", Some("infrastructure"), &why)
            }
            Err(e) => Err(e),
        }
    }

    /// The end of `child`, a run of `parent`: when it is the child of the formula step `parent` is running, its
    /// outcome — or its reason, without one — is the step's.
    pub(super) fn child_ended(&mut self, tx: &mut Tx<'_>, parent: &str, child: &str) -> Result<(), Error> {
        let row = Row::load(tx, parent)?;
        if row.state != "running"
            || row.step_kind != "formula"
            || Self::entry_child(tx, &row)?.as_deref() != Some(child)
        {
            return Ok(());
        }
        let kid = Row::load(tx, child)?;
        if !matches!(kid.state.as_str(), "passed" | "failed" | "cancelled") {
            return Ok(());
        }
        let declared = Self::outcome(tx, &kid)?;
        let result = if kid.state == "passed" {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
        let reason: Option<String> = tx
            .sql()
            .query_row("SELECT reason FROM runs WHERE id = ?1", [child], |r| r.get(0))?;
        let reason = reason.unwrap_or_default();
        let outcome = match declared {
            Some(o) if kid.state != "cancelled" => Outcome { result, ..o },
            _ => Outcome {
                result,
                summary: format!("`{child}` {}: {reason}", kid.state),
                data: None,
            },
        };
        self.step_done(tx, row, &outcome)
    }

    /// Cancels the live child of the formula step `row` failed in: its bead is no longer held for it.
    pub(super) fn end_child(&mut self, tx: &mut Tx<'_>, row: &Row) -> Result<(), Error> {
        if row.step_kind != "formula" {
            return Ok(());
        }
        match Self::entry_child(tx, row)? {
            Some(child) if Row::load(tx, &child)?.live() => self.cancel(tx, &child, "parent_failed"),
            _ => Ok(()),
        }
    }

    /// The bead data of `parent` when its formula step lends `bead` to its child (docs/design/config.md,
    /// docs/design/contracts.md 5): the child works on the bead and the parent keeps it; a child of that child may
    /// borrow it on.
    pub(super) fn lent(tx: &Tx<'_>, parent: Option<&str>, bead: &str) -> Result<Option<String>, Error> {
        let Some(parent) = parent else { return Ok(None) };
        let data: Option<Option<String>> = tx
            .sql()
            .query_row(
                "SELECT bead_data FROM runs WHERE id = ?1 AND bead = ?2 AND step_kind = 'formula'",
                params![parent, bead],
                |r| r.get(0),
            )
            .optional()?;
        Ok(data.flatten())
    }

    /// The last child the run's current entry of its step made: the entry is the tail of its attempts on the step
    /// after its last execution, as for a drain step.
    fn entry_child(tx: &Tx<'_>, row: &Row) -> Result<Option<String>, Error> {
        let child = tx
            .sql()
            .query_row(
                "SELECT r.id FROM runs r WHERE r.parent_run = ?1 AND r.trigger IS NULL \
                 AND r.created_ms >= (SELECT MIN(a.started_ms) FROM attempts a WHERE a.run = ?1 \
                 AND a.n > (SELECT COALESCE(MAX(n), 0) FROM attempts WHERE run = ?1 \
                 AND (step != ?2 OR (outcome IS NOT NULL AND failure IS NULL)))) ORDER BY r.n DESC LIMIT 1",
                params![row.id, row.step],
                |r| r.get(0),
            )
            .optional()?;
        Ok(child)
    }
}
