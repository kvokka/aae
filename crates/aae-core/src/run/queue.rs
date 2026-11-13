//! Creation and the queue (spec §9.3, docs/design/contracts.md 6.1–6.2): a run waits in `queued` for a free
//! workdir and a concurrency slot, then leaves for `preparing`, oldest first.

use super::{Create, RunHandler, machine::QUEUE_TIMEOUT_MS, rejected, row::Row};
use crate::{
    config::{InputType, Step},
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

impl RunHandler {
    pub(super) fn create(&mut self, tx: &mut Tx<'_>, mut c: Create) -> Result<Value, Error> {
        let project = self.config.projects.iter().find(|p| p.name == c.project);
        let project = project.ok_or_else(|| rejected(format!("no project `{}`", c.project)))?;
        let rig = project.rigs.get(&c.rig);
        let rig = rig.ok_or_else(|| rejected(format!("no rig `{}` in `{}`", c.rig, c.project)))?;
        let (name, formula) = project.formula(&c.formula).map_err(rejected)?;
        // An exec step has no agent; the runs table keeps it as an empty name.
        let agent = match &formula.step {
            Step::Prompt { agent, .. } => agent.to_string(),
            Step::Exec { .. } if c.continue_from.is_some() => {
                return Err(rejected(format!(
                    "`{name}` is an exec step: it has no session to continue"
                )));
            }
            Step::Exec { .. } => String::new(),
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
        // A run holds at most one bead (spec §10.3); its input becomes the id of the bead it works on.
        let bead_input = formula
            .inputs
            .iter()
            .find(|(k, i)| i.kind == InputType::Bead && c.inputs.contains_key(*k));
        let bead = match bead_input {
            Some(_) if !self.trackers.contains_key(&c.project) => {
                return Err(rejected(format!("`{}` has no tracker", c.project)));
            }
            Some((key, _)) => match Self::pick_bead(tx, &c.project, &c.rig, &c.inputs[key])? {
                Some(id) => {
                    c.inputs.insert(key.clone(), json!(id));
                    Some(id)
                }
                None => return Ok(json!({ "nothing_ready": true })),
            },
            None => None,
        };
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
        if let Some(bead) = &bead {
            Self::own_bead(tx, &c.project, bead, &id)?;
        }
        tx.sql().execute(
            "INSERT INTO runs (n, id, project, rig, formula, agent, inputs, correlation, depth, parent_run, \
             workdir, state, created_ms, continue_from, bead) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'queued', ?12, ?13, ?14)",
            params![
                n,
                id,
                c.project,
                c.rig,
                name.to_string(),
                agent,
                Value::Object(c.inputs).to_string(),
                correlation,
                c.depth,
                c.parent_run,
                rig.path.to_string_lossy(),
                tx.now_ms(),
                c.continue_from,
                bead
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

    /// Why a queued or parked run cannot start yet, `None` when it can: the daemon is stopping, a gate of the run is closed (with the
    /// closure), its workdir is busy, or the orchestrator or its agent has as many started runs as allowed (spec
    /// §9.3; a parked run has no process and holds no slot).
    fn blocker(&self, tx: &Tx<'_>, row: &Row) -> Result<Option<(&'static str, Value)>, Error> {
        if self.stopping.is_some() {
            return Ok(Some(("daemon_stopping", Value::Null)));
        }
        if let Some(gate) = Self::closed_gate(tx, row)? {
            return Ok(Some(("gate", gate)));
        }
        if Self::workdir_busy(tx, row)? {
            return Ok(Some(("workdir", Value::Null)));
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
        Ok(full.then_some(("concurrency", Value::Null)))
    }

    /// Emits `run.queued` on creation (`first`) and whenever the reason the run waits for changes.
    fn queued(tx: &mut Tx<'_>, row: &Row, waiting: Option<(&str, Value)>, first: bool) -> Result<(), Error> {
        let (waiting, gate) = waiting.map_or((None, Value::Null), |(w, g)| (Some(w), g));
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
            json!({ "run": row.view(), "waiting_for": waiting, "gate": gate }),
        )
        .map(drop)
    }

    /// Starts every queued run and resumes every parked run that may go, oldest first (contracts 7).
    pub(super) fn schedule(&mut self, tx: &mut Tx<'_>) -> Result<(), Error> {
        let waiting: Vec<String> = {
            let mut stmt = tx
                .sql()
                .prepare("SELECT id FROM runs WHERE state IN ('queued', 'parked') ORDER BY n")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in waiting {
            let row = Row::load(tx, &id)?;
            // A nested schedule (a terminal state during preparation, a park) may have moved the run meanwhile.
            match (row.state.as_str(), self.blocker(tx, &row)?) {
                ("queued", None) => self.prepare(tx, row)?,
                ("queued", waiting) => Self::queued(tx, &row, waiting, false)?,
                ("parked", None) => self.unpark(tx, row)?,
                _ => {}
            }
        }
        Ok(())
    }
}
