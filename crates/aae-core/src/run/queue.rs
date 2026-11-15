//! Creation and the queue (spec §9.3, docs/design/contracts.md 6.1–6.2): a run waits in `queued` for a free
//! workdir and a concurrency slot, then leaves for `preparing`, oldest first.

use super::{
    Create, RunHandler, drain::DRAINING, events::attempt_live, rejected, row::Row, token_sha256, triggers::MAX_DEPTH,
};
use crate::{
    config::{Formula, InputType, Step, StepKind},
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

impl RunHandler {
    /// The agent of `step` of a run of `formula` on `rig` — the one the rig runs for it, a variant by its name — and
    /// the model key of its config; an exec step has none, and the runs table keeps an empty name. `as_agent`: the
    /// agent the run's bead names in place of the step's.
    pub(super) fn step_agent(
        &self,
        project: &str,
        rig: &str,
        formula: &str,
        step: &Step,
        as_agent: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        match step.kind {
            StepKind::Prompt { .. } => {
                let (agent, config) = self.config.for_run_as(project, rig, formula, &step.name, as_agent)?;
                Ok((agent, config.model_key()))
            }
            StepKind::Exec { .. } | StepKind::Drain { .. } => Ok((String::new(), None)),
        }
    }

    /// What the bead `bead` of a run of `formula` on `rig` makes of it: the bead, the agent its `agent:` label names,
    /// and the agent and model key of the first step. Refused when a `harness:` label differs from the harness the
    /// config gives a prompt step, or when the named agent does not resolve (spec §9.1).
    #[expect(clippy::type_complexity, reason = "a private tuple read at its one call site")]
    fn bead_agent(
        &self,
        tx: &Tx<'_>,
        project: &str,
        rig: &str,
        name: &str,
        formula: &Formula,
        bead: Option<&str>,
    ) -> Result<(Option<String>, Option<String>, (String, Option<String>)), Error> {
        let label = |prefix| bead.map_or(Ok(None), |id| Self::label(tx, project, id, prefix));
        let (as_agent, harness) = (label("agent")?, label("harness")?);
        if let Some(harness) = &harness {
            let prompts = formula
                .steps
                .iter()
                .filter(|s| matches!(s.kind, StepKind::Prompt { .. }));
            for step in prompts {
                let (agent, config) = (self.config)
                    .for_run_as(project, rig, name, &step.name, as_agent.as_deref())
                    .map_err(rejected)?;
                if &config.harness != harness {
                    return Err(rejected(format!(
                        "bead `{}` is labelled `harness:{harness}`: the config gives `{}` to `{agent}` on `{rig}`",
                        bead.unwrap_or_default(),
                        config.harness
                    )));
                }
            }
        }
        let first = self
            .step_agent(project, rig, name, &formula.steps[0], as_agent.as_deref())
            .map_err(rejected)?;
        Ok((bead.map(str::to_owned), as_agent, first))
    }

    pub(super) fn create(&mut self, tx: &mut Tx<'_>, mut c: Create) -> Result<Value, Error> {
        if let Some(token) = c.token.take() {
            Self::child_of(tx, &mut c, &token)?;
        }
        let project = self.config.projects.iter().find(|p| p.name == c.project);
        let project = project.ok_or_else(|| rejected(format!("no project `{}`", c.project)))?;
        let (name, formula) = project.formula(&c.formula).map_err(rejected)?;
        let rig_name = match c.rig.take() {
            Some(rig) => rig,
            None => {
                let beads = formula.inputs.iter().filter(|(_, i)| i.kind == InputType::Bead);
                let named = beads.filter_map(|(k, _)| c.inputs.get(k)?.as_str()).next();
                Self::rig_of_bead(tx, &c.project, named)?
            }
        };
        let rig = project.rigs.get(&rig_name);
        let rig = rig.ok_or_else(|| rejected(format!("no rig `{rig_name}` in `{}`", c.project)))?;
        let first = &formula.steps[0];
        if c.continue_from.is_some() && !matches!(first.kind, StepKind::Prompt { .. }) {
            return Err(rejected(format!(
                "`{name}` starts with a step of kind `{}`: it has no session to continue",
                first.kind.name()
            )));
        }
        if let Some(extra) = c.inputs.keys().find(|k| !formula.inputs.contains_key(*k)) {
            return Err(rejected(format!("`{name}` has no input `{extra}`")));
        }
        // A null is no value; the text of an integer or a boolean is one, as the CLI and templates give text.
        c.inputs.retain(|_, value| !value.is_null());
        for (key, value) in &mut c.inputs {
            let parsed = match (formula.inputs[key].kind, value.as_str()) {
                (InputType::Integer, Some(text)) => text.parse::<i64>().ok().map(Value::from),
                (InputType::Boolean, Some(text)) => text.parse::<bool>().ok().map(Value::from),
                _ => None,
            };
            if let Some(parsed) = parsed {
                *value = parsed;
            }
        }
        let missing = formula
            .inputs
            .iter()
            .find(|(k, i)| i.required && !c.inputs.contains_key(*k));
        if let Some((key, _)) = missing {
            return Err(rejected(format!("`{name}` needs input `{key}`")));
        }
        let mistyped = c.inputs.iter().find_map(|(key, value)| {
            let want = match formula.inputs[key].kind {
                InputType::Integer if !value.is_i64() && !value.is_u64() => "an integer",
                InputType::Boolean if !value.is_boolean() => "a boolean",
                InputType::String if !value.is_string() => "a string",
                _ => return None,
            };
            Some(format!("input `{key}` of `{name}` must be {want}, not {value}"))
        });
        if let Some(why) = mistyped {
            return Err(rejected(why));
        }
        // A run holds at most one bead (spec §10.3); its input becomes the id of the bead it works on.
        let bead_input = formula
            .inputs
            .iter()
            .find(|(k, i)| i.kind == InputType::Bead && c.inputs.contains_key(*k));
        // A bead the run cannot take as labelled is passed over by a pick and refused when named (spec §10.3).
        let mut passed_over = serde_json::Map::new();
        let (bead, as_agent, (agent, model)) = match bead_input {
            Some(_) if !self.trackers.contains_key(&c.project) => {
                return Err(rejected(format!("`{}` has no tracker", c.project)));
            }
            // A request gets its bead as the run prepares (spec §10.1).
            Some((key, _)) if Self::request(&c.inputs[key])?.is_some() => {
                self.bead_agent(tx, &c.project, &rig_name, &name.to_string(), formula, None)?
            }
            Some((key, _)) => {
                let named = c.inputs[key].is_string();
                let mut picked = None;
                for id in Self::pick_bead(tx, &c.project, &rig_name, &c.inputs[key])? {
                    match self.bead_agent(tx, &c.project, &rig_name, &name.to_string(), formula, Some(&id)) {
                        Ok(found) => {
                            picked = Some(found);
                            break;
                        }
                        Err(Error::Rejected(why)) if !named => {
                            passed_over.insert(id, json!(why));
                        }
                        Err(e) => return Err(e),
                    }
                }
                let Some(found) = picked else {
                    return Ok(with_passed_over(json!({ "nothing_ready": true }), passed_over));
                };
                c.inputs.insert(key.clone(), json!(found.0));
                found
            }
            None => self.bead_agent(tx, &c.project, &rig_name, &name.to_string(), formula, None)?,
        };
        // A continued session resumes in the workdir it ran in (a harness may find its transcript by the cwd).
        let mut workdir = rig.path.to_string_lossy().into_owned();
        if let Some(from) = &c.continue_from {
            let found: Option<(String, bool, String)> = tx
                .sql()
                .query_row(
                    "SELECT state, EXISTS (SELECT 1 FROM attempts WHERE run = ?1 AND session IS NOT NULL), \
                     workdir FROM runs WHERE id = ?1",
                    [from],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match found {
                None => return Err(rejected(format!("no run `{from}` to continue"))),
                Some((state, ..)) if !matches!(state.as_str(), "passed" | "failed" | "cancelled") => {
                    return Err(rejected(format!("`{from}` is still `{state}`")));
                }
                Some((_, false, _)) => return Err(rejected(format!("`{from}` has no session to continue"))),
                Some((.., dir)) => workdir = dir,
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
             workdir, state, created_ms, continue_from, bead, step, as_agent, step_kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'queued', ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                n,
                id,
                c.project,
                rig_name,
                name.to_string(),
                agent,
                Value::Object(c.inputs).to_string(),
                correlation,
                c.depth,
                c.parent_run,
                workdir,
                tx.now_ms(),
                c.continue_from,
                bead,
                first.name,
                as_agent,
                first.kind.name()
            ],
        )?;
        let row = Row::load(tx, &id)?;
        Self::learn_keys(tx, &row, None, model)?;
        let waiting = self.blocker(tx, &row)?;
        Self::queued(tx, &row, waiting, true)?;
        tx.set_deadline(
            &row.deadline("state"),
            tx.now_ms() + self.config.watchdog.queue_timeout_ms,
        )?;
        self.schedule(tx)?;
        Ok(with_passed_over(json!({ "run": id }), passed_over))
    }

    /// A run an agent session starts is a child of the session's run: the token must be that of the parent's live
    /// current attempt, as for the finisher (contracts 6.3); the child takes the parent's correlation, one level
    /// deeper, within the depth ceiling triggers share.
    fn child_of(tx: &Tx<'_>, c: &mut Create, token: &str) -> Result<(), Error> {
        let parent = c
            .parent_run
            .as_deref()
            .ok_or_else(|| rejected("an attempt token needs its run"))?;
        let row = Row::load(tx, parent)?;
        let n: Option<u32> = tx
            .sql()
            .query_row(
                "SELECT n FROM attempts WHERE run = ?1 AND token_sha256 = ?2",
                params![parent, token_sha256(token)],
                |r| r.get(0),
            )
            .optional()?;
        if n != Some(row.attempt) || !attempt_live(&row.state) {
            return Err(rejected(format!(
                "stale attempt: the token is not that of a live attempt of `{parent}`"
            )));
        }
        let depth = row.depth + 1;
        if depth > MAX_DEPTH {
            return Err(rejected(format!(
                "depth {depth} is over {MAX_DEPTH}: `{parent}` is too deep a chain"
            )));
        }
        c.correlation = Some(row.correlation);
        c.depth = u32::try_from(depth).unwrap_or(u32::MAX);
        Ok(())
    }

    /// A workdir holds one live started run (spec §9.3); a parent in a drain step does not hold it from its children.
    fn workdir_busy(tx: &Tx<'_>, row: &Row) -> Result<bool, Error> {
        let sql = format!(
            "SELECT EXISTS (SELECT 1 FROM runs WHERE workdir = ?1 AND id != ?2 \
             AND state NOT IN ('queued', 'passed', 'failed', 'cancelled') AND NOT (id IS ?3 AND {DRAINING}))"
        );
        let busy = tx
            .sql()
            .query_row(&sql, params![row.workdir, row.id, row.parent_run], |r| r.get(0))?;
        Ok(busy)
    }

    /// Why a queued or parked run cannot start yet, `None` when it can: the daemon is stopping, a gate of the run is
    /// closed (with the
    /// closure), its workdir is busy, or the orchestrator or its agent has as many started runs as allowed (spec
    /// §9.3; a parked run, or one in a drain step, has no process and holds no slot).
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
        let all: u32 = tx
            .sql()
            .query_row(&format!("SELECT COUNT(*) FROM runs WHERE {}", started()), [], |r| {
                r.get(0)
            })?;
        let full = all >= self.config.max_runs || self.agent_full(tx, row)?;
        Ok(full.then_some(("concurrency", Value::Null)))
    }

    /// Whether the other started runs of the agent of `row` are as many as its `max_runs` allows (spec §9.3).
    pub(super) fn agent_full(&self, tx: &Tx<'_>, row: &Row) -> Result<bool, Error> {
        let Some(limit) = self.config.max_runs(&row.project, &row.agent) else {
            return Ok(false);
        };
        let others: u32 = tx.sql().query_row(
            &format!(
                "SELECT COUNT(*) FROM runs WHERE project = ?1 AND agent = ?2 AND id != ?3 AND {}",
                started()
            ),
            params![row.project, row.agent, row.id],
            |r| r.get(0),
        )?;
        Ok(others >= limit)
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

/// The runs that hold a concurrency slot: a parked run, or one in a drain step, has no process and holds none.
fn started() -> String {
    format!("state NOT IN ('queued', 'parked', 'passed', 'failed', 'cancelled') AND NOT {DRAINING}")
}

/// `reply` with the beads a pick passed over and why, when there are any.
fn with_passed_over(mut reply: Value, passed_over: serde_json::Map<String, Value>) -> Value {
    if !passed_over.is_empty() {
        reply["passed_over"] = Value::Object(passed_over);
    }
    reply
}
