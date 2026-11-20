//! The run states and transitions of docs/design/contracts.md 6.1–6.2 that do not come from a harness:
//! preparation, attempts and the terminal states.

use super::{
    RunHandler,
    row::{Row, attempts},
};
use crate::{
    config::{EffectiveConfig, Sourced},
    driver::{Driver, Failure, FailureKind, PromptKey, SessionPlan, SessionRef, Source},
    journal::rfc3339_ms,
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

/// The prompt of a resumed attempt (contracts 6.4).
const RECONCILE: &str =
    "Your previous turn was interrupted. Check the current state of the work and continue toward the goal.";

/// The session and previous run a launch must reconcile, and whether the step is starting afresh.
struct AttemptSession {
    resume: Option<SessionRef>,
    continued: Option<String>,
    entering: bool,
}

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
        match (row.bead.clone(), self.task_text(&row)) {
            // The prompt may show the bead: it renders once the claim returned it.
            (Some(bead), _) if row.bead_data.is_none() => self.claim(tx, &row, &bead),
            (None, Some(text)) => self.create_root(tx, row, &text),
            _ => self.render_and_prepare(tx, row),
        }
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
        self.start_attempt(tx, row, "new_session")
    }

    /// A new attempt (contracts 6.2): a new or resumed session on an instance of its own, then the prompt.
    pub(super) fn start_attempt(&mut self, tx: &mut Tx<'_>, row: Row, kind: &str) -> Result<(), Error> {
        // Every automatic start reads the gates, the operator's among them (invariant 18), and a drain: both park.
        if let Some(gate) = self.held(tx, &row)? {
            return self.park(tx, row, gate, kind);
        }
        match row.step_kind.as_str() {
            "exec" => return self.start_exec(tx, row),
            "drain" => return self.start_drain(tx, row),
            "formula" => return self.start_formula(tx, row),
            "await" => return self.start_await(tx, row),
            _ => {}
        }
        let config = row.config.as_deref().map(serde_json::from_str::<EffectiveConfig>);
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
        // Of one step (docs/design/contracts.md 6). A session from before the step's last `new_session` attempt — a
        // new round, a repeat or the policy gave it up — is never resumed.
        let last_session = |tx: &Tx<'_>, run: &str, step: Option<&str>| {
            tx.sql()
                .query_row(
                    "SELECT session, harness FROM attempts WHERE run = ?1 AND session IS NOT NULL \
                    AND (?2 IS NULL OR (step = ?2 AND n >= (SELECT COALESCE(MAX(n), 0) FROM attempts \
                    WHERE run = ?1 AND step = ?2 AND kind = 'new_session'))) ORDER BY n DESC LIMIT 1",
                    params![run, step],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()
        };
        let own = last_session(tx, &row.id, Some(&row.step))?;
        // The first attempt of an execution of a step: at its start, when a check sends the run back to it or when
        // it runs again (docs/design/config.md, docs/design/contracts.md 6); not after an attempt of it that failed.
        let entering: bool = tx.sql().query_row(
            "SELECT NOT EXISTS (SELECT 1 FROM attempts WHERE run = ?1 AND n = ?2 AND step = ?3 \
             AND (outcome IS NULL OR failure IS NOT NULL))",
            params![row.id, row.attempt, row.step],
            |r| r.get(0),
        )?;
        // A run that continues the session of another run (docs/design/contracts.md 6) starts there, and its first step
        // resumes there while it has no session of its own and has started no new one.
        let continued: Option<String> = tx
            .sql()
            .query_row(
                "SELECT continue_from FROM runs WHERE id = ?1 \
                 AND NOT EXISTS (SELECT 1 FROM attempts WHERE run = ?1 AND (step != ?2 OR kind = 'new_session'))",
                params![row.id, row.step],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let from = continued
            .as_ref()
            .map(|f| last_session(tx, f, None))
            .transpose()?
            .flatten();
        if let Some((_, harness)) = from.as_ref().filter(|(_, h)| *h != config.harness) {
            let reason = format!("the continued session is of harness `{harness}`");
            return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
        }
        // A step gone back to or repeated starts a new session, with its prompt and the remarks; only the recovery
        // after a failed attempt resumes (docs/design/contracts.md 6).
        let session = match (row.attempt, kind) {
            (0, _) => from,
            (_, "resume") => own.or(from),
            _ => None,
        };
        let resume = session.and_then(|(s, _)| serde_json::from_str::<SessionRef>(&s).ok());
        self.launch_attempt(
            tx,
            row,
            kind,
            config,
            driver.as_ref(),
            AttemptSession {
                resume,
                continued,
                entering,
            },
        )
    }

    fn launch_attempt(
        &mut self,
        tx: &mut Tx<'_>,
        mut row: Row,
        kind: &str,
        config: EffectiveConfig,
        driver: &dyn Driver,
        session: AttemptSession,
    ) -> Result<(), Error> {
        let AttemptSession {
            resume,
            continued,
            entering,
        } = session;
        let Some(token) = super::new_token() else {
            return self.terminal(
                tx,
                row,
                "failed",
                Some("infrastructure"),
                "no randomness for the attempt token",
            );
        };
        let token_sha256 = super::token_sha256(&token);
        let plan = self.session_plan(&row, &config, token, resume.clone());
        let (config, services) = Self::bind_services(tx, &row, config, &self.config)?;
        let split = driver.split(&config, &plan);
        // The account comes from the config before the start (docs/design/contracts.md 7): a closed account gate parks
        // with no launch.
        if let Ok(split) = &split {
            let account = driver
                .account(&split.process)
                .map(|a| format!("account:{}/{a}", config.harness));
            Self::learn_keys(tx, &row, account, None)?;
            if let Some(gate) = Self::closed_gate(tx, &row)? {
                return self.park(tx, row, gate, kind);
            }
        }
        row.attempt += 1;
        tx.sql().execute(
            "UPDATE runs SET attempt = ?2, next = NULL WHERE id = ?1",
            params![row.id, row.attempt],
        )?;
        tx.sql().execute(
            "INSERT INTO attempts (run, n, kind, harness, started_ms, token_sha256, step) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            // The kind that happens: a resume with no session to resume is a new session.
            params![
                row.id,
                row.attempt,
                if resume.is_some() { "resume" } else { "new_session" },
                config.harness,
                tx.now_ms(),
                token_sha256,
                row.step
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
        let unsettled = match entering {
            true => None,
            false => super::prompts::unsettled(tx, &row.id, row.attempt)?,
        };
        let step_prompt = row.prompt.clone().unwrap_or_default();
        let (prompt, retry) = match (&resume, unsettled) {
            (Some(_), _) if entering => (step_prompt, None),
            (Some(_), unsettled) => (RECONCILE.to_owned(), unsettled.map(|(_, key, text)| (key, text))),
            // A new session knows nothing of the old one: a remark the daemon died under follows the step's prompt.
            (None, Some((n, _, text))) if n > 1 => (format!("{step_prompt}\n\n{text}"), None),
            (None, _) => (step_prompt, None),
        };
        let key = PromptKey::new(&row.id, row.attempt, 1);
        super::prompts::intent(tx, &row.id, row.attempt, 1, key, &prompt)?;
        let payload = json!({
            "op": "launch", "run": row.id, "attempt": row.attempt, "harness": config.harness,
            "instance": instance, "handle": handle, "after": after, "process": split.process.0,
            "session": split.session.0, "resume": resume.map(|s| s.0),
            "prompt": prompt, "key": key.0.to_string(), "services": services,
            "retry": retry.map(|(key, text)| json!({ "key": key, "text": text })),
        });
        tx.effect(&row.id, &format!("launch:{}", row.attempt), &payload)?;
        tx.set_deadline(
            &row.deadline("state"),
            tx.now_ms() + self.config.watchdog.start_timeout_ms,
        )
    }

    fn session_plan(
        &self,
        row: &Row,
        config: &EffectiveConfig,
        token: String,
        resume: Option<SessionRef>,
    ) -> SessionPlan {
        let (run, attempt, workdir) = (row.id.clone(), row.attempt + 1, self.session_dir(row, config));
        let (actor, bead) = (self.config.actor(&row.project, &run, attempt), row.bead.clone());
        match resume {
            Some(session) => SessionPlan::Resume {
                session,
                workdir,
                actor,
                run,
                attempt,
                token,
                bead,
            },
            None => SessionPlan::New {
                workdir,
                actor,
                run,
                attempt,
                token,
                bead,
            },
        }
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
        // The model key is the config's alone (docs/design/contracts.md 7): a model the failure names stays data of the
        // event.
        Self::learn_keys(tx, &row, account, None)?;
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
        let data = json!({ "run": row.view(), "failure": failure, "attempts": attempts,
            "gates": Self::gates_of(tx, &row)?, "rate_limit": Self::limit_of(tx, &row)?,
            "keys": Self::keys(tx.sql(), &row)?, "agent_failures_1h": agent_failures });
        self.deciding(tx, row, "run.attempt_failed", data)
    }

    /// Closes the live attempt of `row`, if any: records its end and stops its session and instance.
    pub(super) fn end_attempt(&mut self, tx: &mut Tx<'_>, row: &Row, failure: Option<&Value>) -> Result<(), Error> {
        for what in ["state", "silence"] {
            tx.clear_deadline(&row.deadline(what))?;
        }
        type Live = (String, Option<String>, Option<String>, Option<String>);
        let live: Option<Live> = tx
            .sql()
            .query_row(
                "SELECT harness, handle, session, instance FROM attempts WHERE run = ?1 AND n = ?2 \
                AND ended_ms IS NULL",
                params![row.id, row.attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((harness, handle, session, instance)) = live else {
            return Ok(());
        };
        // The command of an exec step or the check script of any step (docs/design/config.md); its own dedupe, apart
        // from the stop of a session.
        let command: bool = (tx.sql()).query_row(
            "SELECT EXISTS (SELECT 1 FROM effects WHERE key = ?1)",
            [format!("exec:{}", row.id)],
            |r| r.get(0),
        )?;
        if harness == "exec" || command {
            let payload = json!({ "op": "stop_exec", "run": row.id, "attempt": row.attempt });
            tx.effect(&row.id, &format!("stop_exec:{}", row.attempt), &payload)?;
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

    /// `passed`, `failed{class}` or `cancelled`; frees the run's concurrency slot for the queue.
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
        if state == "failed" {
            self.end_child(tx, &row)?;
        }
        self.let_go_bead(tx, &row, state, reason)?;
        let usage = super::row::usage(tx.sql(), &row.id)?;
        let data = match state {
            "passed" => json!({ "run": row.view(), "outcome": outcome, "usage": usage }),
            "failed" => json!({ "run": row.view(), "class": class, "reason": reason, "outcome": outcome,
                "usage": usage }),
            _ => json!({ "run": row.view(), "reason": reason, "usage": usage }),
        };
        row.emit(tx, &format!("run.{state}"), data)?;
        if let Some(parent) = &row.parent_run {
            self.child_ended(tx, parent, &row.id)?;
        }
        self.schedule(tx)
    }
}
