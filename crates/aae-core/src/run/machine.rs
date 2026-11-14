//! The run states and transitions of docs/design/contracts.md 6.1–6.2 that do not come from a harness:
//! preparation, attempts and the terminal states.

use super::{
    RunHandler,
    row::{Row, attempts},
};
use crate::{
    config::{Sourced, Step},
    driver::{Failure, FailureKind, PromptKey, SessionPlan, SessionRef, Source},
    journal::rfc3339_ms,
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

pub(super) const STEP_TIMEOUT_MS: i64 = 45 * 60_000;
/// The prompt of a resumed attempt (contracts 6.4).
const RECONCILE: &str =
    "Your previous turn was interrupted. Check the current state of the work and continue toward the goal.";

impl RunHandler {
    pub(super) fn prepare(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        tx.clear_deadline(&row.deadline("state"))?;
        row.started_ms = Some(tx.now_ms());
        tx.sql().execute(
            "UPDATE runs SET started_ms = ?2 WHERE id = ?1",
            params![row.id, row.started_ms],
        )?;
        row.set_state(tx, "preparing", None)?;
        row.emit(tx, "run.started", json!({ "run": row.view() }))?;
        match row.bead.clone() {
            // The prompt may show the bead: it renders once the claim returned it.
            Some(bead) if row.bead_data.is_none() => self.claim(tx, &row, &bead),
            _ => self.render_and_prepare(tx, row),
        }
    }

    /// Renders the prompt and asks the `run.preparing` hooks.
    pub(super) fn render_and_prepare(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        let prepared = self.render(&row);
        let (prompt, timeout_ms, config) = match prepared {
            Ok(p) => p,
            Err(reason) => return self.terminal(tx, row, "failed", Some("infrastructure"), &reason),
        };
        let env_keys: Vec<&String> = config["env"].as_object().into_iter().flat_map(|e| e.keys()).collect();
        let data = json!({ "run": row.view(), "workdir": row.workdir, "env_keys": env_keys, "prompt_append": null });
        tx.sql().execute(
            "UPDATE runs SET prompt = ?2, timeout_ms = ?3, config = ?4 WHERE id = ?1",
            params![row.id, prompt, timeout_ms, config.to_string()],
        )?;
        row.config = Some(config.to_string());
        row.prompt = prompt;
        row.timeout_ms = Some(timeout_ms);
        self.deciding(tx, row, "run.preparing", data)
    }

    /// Applies the `run.preparing` modifications of the hooks (contracts 3.2), then starts the first attempt.
    pub(super) fn prepared(
        &mut self,
        tx: &mut Tx<'_>,
        mut row: Row,
        workdir: Option<PathBuf>,
        env: BTreeMap<String, Option<Sourced<String>>>,
        prompt_append: Option<String>,
    ) -> Result<(), Error> {
        let moved = workdir.is_some();
        if let Some(dir) = workdir {
            row.workdir = dir.to_string_lossy().into_owned();
        }
        // An exec step has no prompt to append to.
        if let (Some(text), Some(prompt)) = (prompt_append, &row.prompt) {
            row.prompt = Some(format!("{prompt}\n\n{text}"));
        }
        let config = row.config.as_deref().map(serde_json::from_str::<Value>);
        if let (Some(Ok(mut config)), false) = (config, env.is_empty()) {
            if let Some(vars) = config["env"].as_object_mut() {
                for (name, value) in env {
                    match value {
                        Some(value) => vars.insert(name, json!(value)),
                        None => vars.remove(&name),
                    };
                }
            }
            row.config = Some(config.to_string());
        }
        tx.sql().execute(
            "UPDATE runs SET workdir = ?2, prompt = ?3, config = ?4 WHERE id = ?1",
            params![row.id, row.workdir, row.prompt, row.config],
        )?;
        self.start_attempt(tx, row, "new_session")?;
        // The workdir the run held while preparing (the rig's, spec §9.3) is free once a hook moved the run: a run
        // waiting for it may start (`worktree`, spec §11.5).
        if moved { self.schedule(tx) } else { Ok(()) }
    }

    /// The prompt, the step timeout and the config snapshot of a run, from the config of now: the effective config
    /// of a prompt step; `{"command", "env"}` of an exec step.
    fn render(&self, row: &Row) -> Result<(Option<String>, i64, Value), String> {
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let project = project.ok_or(format!("project `{}` is gone from the config", row.project))?;
        let rig = project
            .rigs
            .get(&row.rig)
            .ok_or(format!("rig `{}` is gone from the config", row.rig))?;
        let (_, formula) = project.formula(&row.formula)?;
        let timeout_ms = |t: &Option<std::time::Duration>| {
            t.map_or(STEP_TIMEOUT_MS, |t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX))
        };
        let (agent, prompt, timeout) = match (&formula.step, row.exec()) {
            (Step::Exec { command, timeout }, true) => {
                // The tracker by name, as in a session (spec §10.2).
                let env: serde_json::Map<String, Value> = (project.tracker.env.iter())
                    .map(|(name, value)| (name.clone(), json!({ "value": value })))
                    .collect();
                return Ok((None, timeout_ms(timeout), json!({ "command": command, "env": env })));
            }
            (Step::Prompt { agent, prompt, timeout }, false) => (agent, prompt, timeout),
            _ => return Err(format!("`{}` changed its step kind", row.formula)),
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
            "bead": row.bead_data.as_deref().map(serde_json::from_str::<Value>).transpose().ok().flatten(),
            "project": { "name": project.name, "path": project.root },
        });
        let text = prompt.render(&context).map_err(|e| format!("prompt: {e}"))?;
        Ok((Some(text), timeout_ms(timeout), json!(config)))
    }

    /// A new attempt (contracts 6.2): a new or resumed session on an instance of its own, then the prompt.
    pub(super) fn start_attempt(&mut self, tx: &mut Tx<'_>, mut row: Row, kind: &str) -> Result<(), Error> {
        // Every automatic start reads the gates, the operator's among them (invariant 18), and a drain: both park.
        if let Some(gate) = self.held(tx, &row)? {
            return self.park(tx, row, gate, kind);
        }
        if row.exec() {
            return self.start_exec(tx, row);
        }
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
        // E.g. the worktree of a continued run, removed when `worktree` no longer restores it.
        if !std::path::Path::new(&row.workdir).is_dir() {
            let reason = format!("the workdir `{}` does not exist", row.workdir);
            return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
        }
        let last_session = |tx: &Tx<'_>, run: &str| {
            tx.sql()
                .query_row(
                    "SELECT session, harness FROM attempts WHERE run = ?1 AND session IS NOT NULL ORDER BY n DESC LIMIT 1",
                    [run],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()
        };
        let own = last_session(tx, &row.id)?;
        // A run that continues the session of another run (spec §9.3) starts there, and resumes there while it has
        // no session of its own.
        let continued: Option<String> =
            tx.sql()
                .query_row("SELECT continue_from FROM runs WHERE id = ?1", [&row.id], |r| r.get(0))?;
        let from = continued.as_ref().map(|f| last_session(tx, f)).transpose()?.flatten();
        if let Some((_, harness)) = from.as_ref().filter(|(_, h)| *h != config.harness) {
            let reason = format!("the continued session is of harness `{harness}`");
            return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
        }
        let session = match (row.attempt, kind) {
            (0, _) => from,
            (_, "resume") => own.or(from),
            _ => None,
        };
        let resume = session.and_then(|(s, _)| serde_json::from_str::<SessionRef>(&s).ok());
        let Some(token) = super::new_token() else {
            return self.terminal(
                tx,
                row,
                "failed",
                Some("infrastructure"),
                "no randomness for the attempt token",
            );
        };
        let (run, attempt, workdir) = (row.id.clone(), row.attempt + 1, PathBuf::from(&row.workdir));
        let token_sha256 = super::token_sha256(&token);
        let plan = match resume.clone() {
            Some(session) => SessionPlan::Resume {
                session,
                workdir,
                run,
                attempt,
                token,
            },
            None => SessionPlan::New {
                workdir,
                run,
                attempt,
                token,
            },
        };
        let (config, services) = Self::bind_services(tx, &row, config, &self.config)?;
        let split = driver.split(&config, &plan);
        // The account comes from the config before the start (spec §5.7): a closed account gate parks with no launch.
        if let Ok(split) = &split {
            let account = driver
                .account(&split.process)
                .map(|a| format!("account:{}/{a}", config.harness));
            Self::learn_keys(tx, &row, account, None)?;
            if let Some(gate) = Self::closed_gate(tx, &row)? {
                return self.park(tx, row, gate, kind);
            }
        }
        row.attempt = attempt;
        tx.sql().execute(
            "UPDATE runs SET attempt = ?2, next = NULL WHERE id = ?1",
            params![row.id, row.attempt],
        )?;
        tx.sql().execute(
            "INSERT INTO attempts (run, n, kind, harness, started_ms, token_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            // The kind that happens: a resume with no session to resume is a new session.
            params![
                row.id,
                row.attempt,
                if resume.is_some() { "resume" } else { "new_session" },
                config.harness,
                tx.now_ms(),
                token_sha256
            ],
        )?;
        row.set_state(tx, "starting", None)?;
        let split = match split {
            Ok(split) => split,
            Err(failure) => return self.attempt_failed(tx, row, failure),
        };
        let caps = driver.capabilities(&split.process);
        let Some((instance, handle)) = Self::acquire(tx, &config.harness, &split, &caps)? else {
            return self.attempt_failed(tx, row, Failure::new(FailureKind::Unknown, Source::Core));
        };
        tx.sql().execute(
            "UPDATE attempts SET instance = ?3 WHERE run = ?1 AND n = ?2",
            params![row.id, row.attempt, instance],
        )?;
        Self::use_services(tx, &row.id, row.attempt, &services)?;
        let after = Self::old_instances(tx, &row.id, continued.as_deref())?;
        let prompt = match resume {
            Some(_) if row.attempt > 1 => RECONCILE.to_owned(),
            _ => row.prompt.clone().unwrap_or_default(),
        };
        let key = PromptKey::new(&row.id, row.attempt, 1);
        super::prompts::intent(tx, &row.id, row.attempt, 1, key, &prompt)?;
        let retry = match resume {
            Some(_) => super::prompts::unsettled(tx, &row.id, row.attempt)?,
            None => None,
        };
        let payload = json!({
            "op": "launch", "run": row.id, "attempt": row.attempt, "harness": config.harness,
            "instance": instance, "handle": handle, "after": after, "process": split.process.0, "session": split.session.0, "resume": resume.map(|s| s.0),
            "prompt": prompt, "key": key.0.to_string(), "services": services, "retry": retry.map(|(key, text)| json!({ "key": key, "text": text })),
        });
        tx.effect(&row.id, &format!("launch:{}", row.attempt), &payload)?;
        tx.set_deadline(
            &row.deadline("state"),
            tx.now_ms() + self.config.watchdog.start_timeout_ms,
        )
    }

    /// Ends the current attempt with `failure` and asks for a decision on it (`interrupted`).
    pub(super) fn attempt_failed(&mut self, tx: &mut Tx<'_>, mut row: Row, failure: Failure) -> Result<(), Error> {
        let harness: Option<String> = tx
            .sql()
            .query_row(
                "SELECT harness FROM attempts WHERE run = ?1 AND n = ?2",
                params![row.id, row.attempt],
                |r| r.get(0),
            )
            .optional()?;
        let account = (failure.account.as_ref().zip(harness)).map(|(a, h)| format!("account:{h}/{a}"));
        Self::learn_keys(
            tx,
            &row,
            account,
            failure.provider_model.as_ref().map(|m| format!("model:{m}")),
        )?;
        let failure = super::failure_json(&failure);
        self.end_attempt(tx, &row, Some(&failure))?;
        row.set_state(tx, "interrupted", failure["kind"].as_str())?;
        let attempts = attempts(tx.sql(), &row.id)?;
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
        let data = json!({ "run": row.view(), "failure": failure, "attempts": attempts, "gates": Self::gates_of(tx, &row)?,
            "keys": Self::keys(tx, &row)?, "agent_failures_1h": agent_failures });
        self.deciding(tx, row, "run.attempt_failed", data)
    }

    /// Closes the live attempt of `row`, if any: records its end and stops its session and instance.
    fn end_attempt(&mut self, tx: &mut Tx<'_>, row: &Row, failure: Option<&Value>) -> Result<(), Error> {
        for what in ["state", "silence", "human"] {
            tx.clear_deadline(&row.deadline(what))?;
        }
        type Live = (String, Option<String>, Option<String>, Option<String>);
        let live: Option<Live> = tx
            .sql()
            .query_row(
                "SELECT harness, handle, session, instance FROM attempts WHERE run = ?1 AND n = ?2 AND ended_ms IS NULL",
                params![row.id, row.attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((harness, handle, session, instance)) = live else {
            return Ok(());
        };
        if row.exec() {
            tx.effect(
                &row.id,
                &format!("stop:{}", row.attempt),
                &json!({ "op": "stop_exec", "run": row.id }),
            )?;
        }
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
        Self::release_services(tx, &row.id, row.attempt)?;
        match instance {
            Some(id) => Self::release(tx, &id),
            None => Ok(()),
        }
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
        let outcome = Self::outcome(tx, &row)?;
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
        self.let_go_bead(tx, &row, state, reason)?;
        match state {
            "passed" => row.emit(tx, "run.passed", json!({ "run": row.view(), "outcome": outcome }))?,
            "failed" => row.emit(
                tx,
                "run.failed",
                json!({ "run": row.view(), "class": class, "reason": reason, "outcome": outcome }),
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
