//! Creation and the queue (spec §9.3, docs/design/contracts.md 6.1–6.2): a run waits in `queued` for a free
//! workdir and a concurrency slot, then leaves for `preparing`, oldest first.

use super::{Create, RunHandler, machine::QUEUE_TIMEOUT_MS, rejected, row::Row};
use crate::{
    config::Step,
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

impl RunHandler {
    pub(super) fn create(&mut self, tx: &mut Tx<'_>, c: Create) -> Result<Value, Error> {
        let project = self.config.projects.iter().find(|p| p.name == c.project);
        let project = project.ok_or_else(|| rejected(format!("no project `{}`", c.project)))?;
        let rig = project.rigs.get(&c.rig);
        let rig = rig.ok_or_else(|| rejected(format!("no rig `{}` in `{}`", c.rig, c.project)))?;
        let (name, formula) = project.formula(&c.formula).map_err(rejected)?;
        let Step::Prompt { agent, .. } = &formula.step else {
            return Err(rejected(format!(
                "`{name}` is an exec step; exec steps are not run yet"
            )));
        };
        if let Some(extra) = c.inputs.keys().find(|k| !formula.inputs.contains_key(*k)) {
            return Err(rejected(format!("`{name}` has no input `{extra}`")));
        }
        let missing = formula
            .inputs
            .iter()
            .find(|(k, i)| i.required && !c.inputs.contains_key(*k));
        if let Some((key, _)) = missing {
            return Err(rejected(format!("`{name}` needs input `{key}`")));
        }
        if let Some(from) = &c.continue_from {
            let found: Option<(String, bool)> = tx
                .sql()
                .query_row(
                    "SELECT state, EXISTS (SELECT 1 FROM attempts WHERE run = ?1 AND session IS NOT NULL) \
                     FROM runs WHERE id = ?1",
                    [from],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match found {
                None => return Err(rejected(format!("no run `{from}` to continue"))),
                Some((state, _)) if !matches!(state.as_str(), "passed" | "failed" | "cancelled") => {
                    return Err(rejected(format!("`{from}` is still `{state}`")));
                }
                Some((_, false)) => return Err(rejected(format!("`{from}` has no session to continue"))),
                Some(_) => {}
            }
        }
        let n: i64 = tx
            .sql()
            .query_row("SELECT COALESCE(MAX(n), 0) + 1 FROM runs", [], |r| r.get(0))?;
        let id = format!("run_{n}");
        let correlation = c.correlation.unwrap_or_else(|| format!("cor_{n}"));
        tx.sql().execute(
            "INSERT INTO runs (n, id, project, rig, formula, agent, inputs, correlation, depth, parent_run, \
             workdir, state, created_ms, continue_from) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'queued', ?12, ?13)",
            params![
                n,
                id,
                c.project,
                c.rig,
                name.to_string(),
                agent.to_string(),
                Value::Object(c.inputs).to_string(),
                correlation,
                c.depth,
                c.parent_run,
                rig.path.to_string_lossy(),
                tx.now_ms(),
                c.continue_from
            ],
        )?;
        let row = Row::load(tx, &id)?;
        let waiting = self.blocker(tx, &row)?;
        Self::queued(tx, &row, waiting, true)?;
        tx.set_deadline(&row.deadline("state"), tx.now_ms() + QUEUE_TIMEOUT_MS)?;
        self.schedule(tx)?;
        Ok(json!({ "run": id }))
    }

    /// A workdir holds one live started run (spec §9.3).
    fn workdir_busy(tx: &Tx<'_>, row: &Row) -> Result<bool, Error> {
        let busy = tx.sql().query_row(
            "SELECT EXISTS (SELECT 1 FROM runs WHERE workdir = ?1 AND id != ?2 \
             AND state NOT IN ('queued', 'passed', 'failed', 'cancelled'))",
            params![row.workdir, row.id],
            |r| r.get(0),
        )?;
        Ok(busy)
    }

    /// Why a queued run cannot start yet, `None` when it can: its workdir is busy, or the orchestrator or its agent
    /// has as many started runs as allowed (spec §9.3; a parked run has no process and holds no slot).
    fn blocker(&self, tx: &Tx<'_>, row: &Row) -> Result<Option<&'static str>, Error> {
        if Self::workdir_busy(tx, row)? {
            return Ok(Some("workdir"));
        }
        let started = "state NOT IN ('queued', 'parked', 'passed', 'failed', 'cancelled')";
        let all: u32 = tx
            .sql()
            .query_row(&format!("SELECT COUNT(*) FROM runs WHERE {started}"), [], |r| r.get(0))?;
        let agent: u32 = tx.sql().query_row(
            &format!("SELECT COUNT(*) FROM runs WHERE project = ?1 AND agent = ?2 AND {started}"),
            params![row.project, row.agent],
            |r| r.get(0),
        )?;
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let limit = project.into_iter().flat_map(|p| &p.packs).find_map(|pack| {
            let mut agents = pack.agents.iter();
            agents
                .find(|(n, _)| format!("{}.{n}", pack.name) == row.agent)?
                .1
                .max_runs
        });
        let full = all >= self.config.max_runs || limit.is_some_and(|l| agent >= l);
        Ok(full.then_some("concurrency"))
    }

    /// Emits `run.queued` on creation (`first`) and whenever the reason the run waits for changes.
    fn queued(tx: &mut Tx<'_>, row: &Row, waiting: Option<&str>, first: bool) -> Result<(), Error> {
        let changed = tx.sql().execute(
            "UPDATE runs SET waiting_for = ?2 WHERE id = ?1 AND waiting_for IS NOT ?2",
            params![row.id, waiting],
        )?;
        if changed == 0 && !first {
            return Ok(());
        }
        row.emit(
            tx,
            "run.queued",
            json!({ "run": row.view(), "waiting_for": waiting, "gate": null }),
        )
        .map(drop)
    }

    /// Starts every queued run that may start, oldest first.
    pub(super) fn schedule(&mut self, tx: &mut Tx<'_>) -> Result<(), Error> {
        let queued: Vec<String> = {
            let mut stmt = tx
                .sql()
                .prepare("SELECT id FROM runs WHERE state = 'queued' ORDER BY n")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in queued {
            let row = Row::load(tx, &id)?;
            // A run started meanwhile by a nested schedule (a terminal state during preparation) is not queued.
            if row.state != "queued" {
                continue;
            }
            match self.blocker(tx, &row)? {
                None => self.prepare(tx, row)?,
                waiting => Self::queued(tx, &row, waiting, false)?,
            }
        }
        Ok(())
    }
}
