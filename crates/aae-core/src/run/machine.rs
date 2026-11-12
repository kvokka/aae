//! The run states and transitions of docs/design/contracts.md 6.1–6.2 that do not come from a harness: creation,
//! the queue, preparation, attempts and the terminal states.

use super::{
    Create, RunHandler, rejected,
    row::{Row, attempts},
};
use crate::{
    config::Step,
    driver::{Failure, PromptKey, SessionPlan, SessionRef},
    journal::rfc3339_ms,
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::path::PathBuf;

pub(super) const QUEUE_TIMEOUT_MS: i64 = 7 * 24 * 3_600_000;
pub(super) const START_TIMEOUT_MS: i64 = 120_000;
pub(super) const STEP_TIMEOUT_MS: i64 = 45 * 60_000;
/// The prompt of a resumed attempt (contracts 6.4).
const RECONCILE: &str =
    "Your previous turn was interrupted. Check the current state of the work and continue toward the goal.";

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
        let n: i64 = tx
            .sql()
            .query_row("SELECT COALESCE(MAX(n), 0) + 1 FROM runs", [], |r| r.get(0))?;
        let id = format!("run_{n}");
        let correlation = c.correlation.unwrap_or_else(|| format!("cor_{n}"));
        tx.sql().execute(
            "INSERT INTO runs (n, id, project, rig, formula, agent, inputs, correlation, depth, parent_run, \
             workdir, state, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'queued', ?12)",
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
                tx.now_ms()
            ],
        )?;
        let row = Row::load(tx, &id)?;
        let waiting = Self::workdir_busy(tx, &row)?.then_some("workdir");
        row.emit(
            tx,
            "run.queued",
            json!({ "run": row.view(), "waiting_for": waiting, "gate": null }),
        )?;
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

    /// Starts every queued run whose workdir is free, oldest first.
    pub(super) fn schedule(&mut self, tx: &mut Tx<'_>) -> Result<(), Error> {
        let queued: Vec<String> = {
            let mut stmt = tx
                .sql()
                .prepare("SELECT id FROM runs WHERE state = 'queued' ORDER BY n")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in queued {
            let row = Row::load(tx, &id)?;
            if row.state == "queued" && !Self::workdir_busy(tx, &row)? {
                self.prepare(tx, row)?;
            }
        }
        Ok(())
    }

    fn prepare(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        tx.clear_deadline(&row.deadline("state"))?;
        row.started_ms = Some(tx.now_ms());
        tx.sql().execute(
            "UPDATE runs SET started_ms = ?2 WHERE id = ?1",
            params![row.id, row.started_ms],
        )?;
        row.set_state(tx, "preparing", None)?;
        row.emit(tx, "run.started", json!({ "run": row.view() }))?;
        let prepared = self.render(&row);
        let (prompt, timeout_ms, config) = match prepared {
            Ok(p) => p,
            Err(reason) => return self.terminal(tx, row, "failed", Some("infrastructure"), &reason),
        };
        let env_keys: Vec<&String> = config.env.keys().collect();
        let data = json!({ "run": row.view(), "workdir": row.workdir, "env_keys": env_keys, "prompt_append": null });
        tx.sql().execute(
            "UPDATE runs SET prompt = ?2, timeout_ms = ?3, config = ?4 WHERE id = ?1",
            params![row.id, prompt, timeout_ms, json!(config).to_string()],
        )?;
        row.config = Some(json!(config).to_string());
        row.prompt = Some(prompt);
        row.timeout_ms = Some(timeout_ms);
        self.deciding(tx, row, "run.preparing", data)
    }

    /// The prompt, the step timeout and the effective config of a run, from the config of now.
    fn render(&self, row: &Row) -> Result<(String, i64, crate::config::EffectiveConfig), String> {
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let project = project.ok_or(format!("project `{}` is gone from the config", row.project))?;
        let rig = project
            .rigs
            .get(&row.rig)
            .ok_or(format!("rig `{}` is gone from the config", row.rig))?;
        let (_, formula) = project.formula(&row.formula)?;
        let Step::Prompt { agent, prompt, timeout } = &formula.step else {
            return Err(format!("`{}` is no longer a prompt step", row.formula));
        };
        if agent.to_string() != row.agent {
            return Err(format!("`{}` now names agent `{agent}`", row.formula));
        }
        let config = self.config.effective(&row.project, Some(&row.rig), agent)?;
        let mut rig_view = json!({ "name": row.rig, "path": rig.path });
        for (key, value) in &rig.vars {
            rig_view[key] = json!(value);
        }
        let context = json!({
            "inputs": serde_json::from_str::<Value>(&row.inputs).unwrap_or_default(),
            "rig": rig_view, "run": row.view(),
            "project": { "name": project.name, "path": project.root },
        });
        let text = prompt.render(&context).map_err(|e| format!("prompt: {e}"))?;
        let timeout_ms = timeout.map_or(STEP_TIMEOUT_MS, |t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX));
        Ok((text, timeout_ms, config))
    }

    /// A new attempt (contracts 6.2): a new or resumed session on an instance of its own, then the prompt.
    pub(super) fn start_attempt(&mut self, tx: &mut Tx<'_>, mut row: Row, kind: &str) -> Result<(), Error> {
        let config = row
            .config
            .as_deref()
            .map(serde_json::from_str::<crate::config::EffectiveConfig>);
        let Some(Ok(config)) = config else {
            return self.terminal(tx, row, "failed", Some("infrastructure"), "no config snapshot");
        };
        let Some(driver) = self.drivers.get(&config.harness).cloned() else {
            let reason = format!("no driver for harness `{}`", config.harness);
            return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
        };
        let previous: Option<String> = tx
            .sql()
            .query_row(
                "SELECT session FROM attempts WHERE run = ?1 AND session IS NOT NULL ORDER BY n DESC LIMIT 1",
                [&row.id],
                |r| r.get(0),
            )
            .optional()?;
        let resume = previous
            .filter(|_| kind == "resume")
            .and_then(|s| serde_json::from_str::<SessionRef>(&s).ok());
        row.attempt += 1;
        let (run, attempt, workdir) = (row.id.clone(), row.attempt, PathBuf::from(&row.workdir));
        let plan = match resume.clone() {
            Some(session) => SessionPlan::Resume {
                session,
                workdir,
                run,
                attempt,
            },
            None => SessionPlan::New { workdir, run, attempt },
        };
        tx.sql().execute(
            "UPDATE runs SET attempt = ?2, next = NULL WHERE id = ?1",
            params![row.id, row.attempt],
        )?;
        tx.sql().execute(
            "INSERT INTO attempts (run, n, kind, harness, started_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            // The kind that happens: a resume with no session to resume is a new session.
            params![
                row.id,
                row.attempt,
                if resume.is_some() { "resume" } else { "new_session" },
                config.harness,
                tx.now_ms()
            ],
        )?;
        row.set_state(tx, "starting", None)?;
        let split = match driver.split(&config, &plan) {
            Ok(split) => split,
            Err(failure) => return self.attempt_failed(tx, row, failure),
        };
        let prompt = match resume {
            Some(_) => RECONCILE.to_owned(),
            None => row.prompt.clone().unwrap_or_default(),
        };
        let payload = json!({
            "op": "launch", "run": row.id, "attempt": row.attempt, "harness": config.harness,
            "process": split.process.0, "session": split.session.0, "resume": resume.map(|s| s.0),
            "prompt": prompt, "key": PromptKey::new(&row.id, row.attempt, 1).0.to_string(),
        });
        tx.effect(&row.id, &format!("launch:{}", row.attempt), &payload)?;
        tx.set_deadline(&row.deadline("state"), tx.now_ms() + START_TIMEOUT_MS)
    }

    /// Ends the current attempt with `failure` and asks for a decision on it (`interrupted`).
    pub(super) fn attempt_failed(&mut self, tx: &mut Tx<'_>, mut row: Row, failure: Failure) -> Result<(), Error> {
        let failure = super::failure_json(&failure);
        self.end_attempt(tx, &row, Some(&failure))?;
        row.set_state(tx, "interrupted", failure["kind"].as_str())?;
        let attempts = attempts(tx, &row.id)?;
        let agent_failures: Vec<Value> = {
            let mut stmt = tx.sql().prepare(
                "SELECT a.run, json_extract(a.failure, '$.kind'), a.ended_ms FROM attempts a \
                 JOIN runs r ON r.id = a.run WHERE r.agent = ?1 AND a.failure IS NOT NULL AND a.ended_ms >= ?2 \
                 ORDER BY a.ended_ms",
            )?;
            let rows = stmt.query_map(params![row.agent, tx.now_ms() - 3_600_000], |r| {
                Ok(json!({ "run": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?,
                    "at": rfc3339_ms(r.get(2)?) }))
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let data = json!({ "run": row.view(), "failure": failure, "attempts": attempts, "gates": [],
            "agent_failures_1h": agent_failures });
        self.deciding(tx, row, "run.attempt_failed", data)
    }

    /// Closes the live attempt of `row`, if any: records its end and stops its session and instance.
    fn end_attempt(&mut self, tx: &mut Tx<'_>, row: &Row, failure: Option<&Value>) -> Result<(), Error> {
        for what in ["state", "silence", "human"] {
            tx.clear_deadline(&row.deadline(what))?;
        }
        let live: Option<(String, Option<String>, Option<String>)> = tx
            .sql()
            .query_row(
                "SELECT harness, handle, session FROM attempts WHERE run = ?1 AND n = ?2 AND ended_ms IS NULL",
                params![row.id, row.attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((harness, handle, session)) = live else {
            return Ok(());
        };
        tx.sql().execute(
            "UPDATE attempts SET ended_ms = ?3, failure = ?4 WHERE run = ?1 AND n = ?2",
            params![row.id, row.attempt, tx.now_ms(), failure.map(Value::to_string)],
        )?;
        if let Some(handle) = handle {
            let session: Value = session.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let handle: Value = serde_json::from_str(&handle).unwrap_or_default();
            let payload = json!({ "op": "stop", "harness": harness, "handle": handle, "session": session });
            tx.effect(&row.id, &format!("stop:{}", row.attempt), &payload)?;
        }
        Ok(())
    }

    /// `passed`, `failed{class}` or `cancelled`; frees the workdir for the queue.
    pub(super) fn terminal(
        &mut self,
        tx: &mut Tx<'_>,
        mut row: Row,
        state: &str,
        class: Option<&str>,
        reason: &str,
    ) -> Result<(), Error> {
        self.end_attempt(tx, &row, None)?;
        for what in ["state", "decide"] {
            tx.clear_deadline(&row.deadline(what))?;
        }
        tx.sql().execute(
            "UPDATE runs SET class = ?2, reason = ?3, waiting = NULL, next = NULL WHERE id = ?1",
            params![row.id, class, reason],
        )?;
        row.class = class.map(str::to_owned);
        row.set_state(tx, state, Some(reason))?;
        match state {
            "failed" => row.emit(
                tx,
                "run.failed",
                json!({ "run": row.view(), "class": class, "reason": reason, "outcome": null }),
            )?,
            _ => row.emit(
                tx,
                &format!("run.{state}"),
                json!({ "run": row.view(), "reason": reason }),
            )?,
        };
        self.schedule(tx)
    }
}
