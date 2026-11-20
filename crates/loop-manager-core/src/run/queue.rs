//! Creation and the queue (docs/design/contracts.md 6.1–6.2): a run waits in `queued` for open gates,
//! a concurrency slot and a place in its rig (`max_writers`), then leaves for `preparing`, oldest first.

use super::{
    Create, RunHandler, drain::NO_PROCESS, events::attempt_live, not_found, rejected, row::Row, token_sha256,
    triggers::MAX_DEPTH,
};
use crate::{
    config::{Formula, InputType, Name, Project, Rig, Step, StepKind},
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
            StepKind::Ai { .. } => {
                let (agent, config) = self.config.for_run_as(project, rig, formula, &step.name, as_agent)?;
                Ok((agent, config.model_key()))
            }
            StepKind::Exec { .. } | StepKind::Drain { .. } | StepKind::Formula { .. } | StepKind::Await { .. } => {
                Ok((String::new(), None))
            }
        }
    }

    /// What the bead `bead` of a run of `formula` on `rig` makes of it: the bead, the agent its `agent:` label names,
    /// and the agent and model key of the first step. Refused when the bead has a `rig:` label, when a `harness:`
    /// label differs from the harness the config gives an ai step, or when the named agent does not resolve
    /// (docs/design/config.md).
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
        // The rig of a bead was once its `rig:` label; such a bead is refused, not run on the root unnoticed.
        if !matches!(label("rig"), Ok(None)) {
            return Err(rejected(format!(
                "bead `{}` has a `rig:` label: the rig of a bead is the prefix of its id",
                bead.unwrap_or_default()
            )));
        }
        let (as_agent, harness) = (label("agent")?, label("harness")?);
        if let Some(harness) = &harness {
            let prompts = formula.steps.iter().filter(|s| matches!(s.kind, StepKind::Ai { .. }));
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
        // A step of the parent creates with no token; a start by hand or from a session is direct. A trigger never
        // names an internal formula: the config refuses it.
        let direct = c.parent_run.is_none() || c.token.is_some();
        if let Some(token) = c.token.take() {
            Self::child_of(tx, &mut c, &token)?;
        }
        let project = self.config.projects.iter().find(|p| p.name == c.project);
        let project = project.ok_or_else(|| not_found(format!("no project `{}`", c.project)))?;
        let (name, formula) = c.resolve_formula(project, direct)?;
        let (rig_name, rig) = c.select_rig(project, formula)?;
        c.validate_inputs(&name, formula)?;
        let first = &formula.steps[0];
        // A run holds at most one bead (docs/design/contracts.md 5); its task input becomes the id of the bead it works
        // on.
        let bead_input = formula
            .inputs
            .iter()
            .find(|(k, i)| i.kind == InputType::Task && c.inputs.contains_key(*k));
        // A bead the run cannot take as labelled is passed over by a pick and refused when named
        // (docs/design/contracts.md 5).
        let mut passed_over = serde_json::Map::new();
        let (bead, as_agent, (agent, model)) = match bead_input {
            Some(_) if !self.trackers.contains_key(&c.project) => {
                return Err(rejected(format!("`{}` has no tracker", c.project)));
            }
            // A text gets its bead as the run prepares (docs/design/contracts.md 5).
            Some((key, _)) if Self::is_text(tx, &c.project, &c.inputs[key])? => {
                self.bead_agent(tx, &c.project, &rig_name, &name.to_string(), formula, None)?
            }
            Some((key, _)) => {
                let named = c.inputs[key].is_string();
                let mut picked = None;
                for id in Self::pick_bead(project, tx, &rig_name, &c.inputs[key])? {
                    match self.bead_agent(tx, &c.project, &rig_name, &name.to_string(), formula, Some(&id)) {
                        Ok(found) => {
                            picked = Some(found);
                            break;
                        }
                        Err(Error::Rejected(why) | Error::NotFound(why)) if !named => {
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
        let workdir = match &c.continue_from {
            Some(from) => continued_workdir(tx, from)?,
            None => rig.path.to_string_lossy().into_owned(),
        };
        let n: i64 = tx
            .sql()
            .query_row("SELECT COALESCE(MAX(n), 0) + 1 FROM runs", [], |r| r.get(0))?;
        let id = format!("run_{n}");
        let correlation = c.correlation.unwrap_or_else(|| format!("cor_{n}"));
        // A formula step lends its child the bead the parent holds: the child neither claims nor lets go of it.
        let lent = match (&bead, direct) {
            (Some(bead), false) => Self::lent(tx, c.parent_run.as_deref(), bead)?,
            _ => None,
        };
        if let Some(bead) = bead.as_ref().filter(|_| lent.is_none()) {
            Self::own_bead(tx, &c.project, bead, &id)?;
        }
        tx.sql().execute(
            "INSERT INTO runs (n, id, project, rig, formula, agent, inputs, correlation, depth, parent_run, \
             workdir, state, created_ms, continue_from, bead, step, as_agent, step_kind, bead_data) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'queued', ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
                first.kind.name(),
                lent
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

    /// Why a queued or parked run cannot start yet, `None` when it can: the daemon is stopping, a gate of the run is
    /// closed (with the closure), the orchestrator or its agent has as many started runs as allowed
    /// (docs/design/contracts.md 6; a parked run, or one in a step with no process, holds no slot), or a queued run
    /// would be one writer too many in its rig's directory (`max_writers`; a parked run already holds its place).
    fn blocker(&self, tx: &Tx<'_>, row: &Row) -> Result<Option<(&'static str, Value)>, Error> {
        if self.stopping.is_some() {
            return Ok(Some(("daemon_stopping", Value::Null)));
        }
        if let Some(gate) = Self::closed_gate(tx, row)? {
            return Ok(Some(("gate", gate)));
        }
        let all: u32 = tx
            .sql()
            .query_row(&format!("SELECT COUNT(*) FROM runs WHERE {}", started()), [], |r| {
                r.get(0)
            })?;
        if all >= self.config.max_runs || self.agent_full(tx, row)? {
            return Ok(Some(("concurrency", Value::Null)));
        }
        let full = row.state == "queued" && self.rig_full(tx, row)?;
        Ok(full.then_some(("writers", Value::Null)))
    }

    /// Whether the other started runs of the agent of `row` are as many as its `max_runs` allows
    /// (docs/design/contracts.md 6).
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
                ("queued", None) => {
                    // What it waited for is past: only a queued or parked run waits.
                    (tx.sql()).execute("UPDATE runs SET waiting_for = NULL WHERE id = ?1", [&row.id])?;
                    self.prepare(tx, row)?;
                }
                ("queued", waiting) => Self::queued(tx, &row, waiting, false)?,
                ("parked", None) => self.unpark(tx, row)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// The runs that hold a concurrency slot: a parked run, or one in a drain, formula or await step, has no process and holds none.
fn started() -> String {
    format!("state NOT IN ('queued', 'parked', 'passed', 'failed', 'cancelled') AND NOT {NO_PROCESS}")
}

/// `reply` with the beads a pick passed over and why, when there are any.
fn with_passed_over(mut reply: Value, passed_over: serde_json::Map<String, Value>) -> Value {
    if !passed_over.is_empty() {
        reply["passed_over"] = Value::Object(passed_over);
    }
    reply
}

impl Create {
    fn resolve_formula<'a>(&self, project: &'a Project, direct: bool) -> Result<(Name, &'a Formula), Error> {
        // An ambiguous name is of formulas that are there; only a name of none is "no such thing".
        let (name, formula) = project
            .formula(&self.formula)
            .map_err(|why| match why.starts_with("no formula") {
                true => not_found(why),
                false => rejected(why),
            })?;
        if direct && formula.internal {
            return Err(rejected(format!(
                "`{name}` is internal: only a step of another formula runs it"
            )));
        }
        Ok((name, formula))
    }

    fn select_rig<'a>(&mut self, project: &'a Project, formula: &Formula) -> Result<(String, &'a Rig), Error> {
        // A start that names no rig runs on the rig of the bead its task names, else on the root.
        let rig_name = self.rig.take().unwrap_or_else(|| {
            let beads = formula.inputs.iter().filter(|(_, i)| i.kind == InputType::Task);
            let named = beads.filter_map(|(k, _)| self.inputs.get(k)?.as_str()).next();
            named
                .and_then(|id| project.rig_of(id))
                .unwrap_or(&project.root.0)
                .to_owned()
        });
        let rig = project.place(&rig_name);
        let rig = rig.ok_or_else(|| not_found(format!("no rig `{rig_name}` in `{}`", self.project)))?;
        Ok((rig_name, rig))
    }

    fn validate_inputs(&mut self, name: &Name, formula: &Formula) -> Result<(), Error> {
        let first = &formula.steps[0];
        if self.continue_from.is_some() && !matches!(first.kind, StepKind::Ai { .. }) {
            return Err(rejected(format!(
                "`{name}` starts with a step of kind `{}`: it has no session to continue",
                first.kind.name()
            )));
        }
        // A skipped first step would hand the continued session to a later step (docs/design/config.md).
        if self.continue_from.is_some() && !first.condition.is_empty() {
            return Err(rejected(format!(
                "`{name}` starts with a step with an `if`: it may be skipped, so it cannot continue a session"
            )));
        }
        if let Some(extra) = self.inputs.keys().find(|k| !formula.inputs.contains_key(*k)) {
            return Err(rejected(format!("`{name}` has no input `{extra}`")));
        }
        // A null is no value; an input given no value takes the one the config gives (docs/design/config.md).
        self.inputs.retain(|_, value| !value.is_null());
        for (key, given) in &formula.defaults {
            self.inputs.entry(key.clone()).or_insert_with(|| given.value.clone());
        }
        let missing = formula
            .inputs
            .iter()
            .find(|(k, i)| i.required && !self.inputs.contains_key(*k));
        if let Some((key, _)) = missing {
            return Err(rejected(format!("`{name}` needs input `{key}`")));
        }
        for (key, value) in &mut self.inputs {
            *value = (formula.inputs[key].kind.take(value))
                .map_err(|want| rejected(format!("input `{key}` of `{name}` must be {want}, not {value}")))?;
        }
        Ok(())
    }
}

/// A continued session resumes in the workdir where the harness wrote its transcript.
fn continued_workdir(tx: &Tx<'_>, from: &str) -> Result<String, Error> {
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
        None => Err(not_found(format!("no run `{from}` to continue"))),
        Some((state, ..)) if !matches!(state.as_str(), "passed" | "failed" | "cancelled") => {
            Err(rejected(format!("`{from}` is still `{state}`")))
        }
        Some((_, false, _)) => Err(rejected(format!("`{from}` has no session to continue"))),
        Some((.., dir)) => Ok(dir),
    }
}
