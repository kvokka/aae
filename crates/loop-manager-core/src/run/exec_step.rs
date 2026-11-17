//! The exec step (docs/design/config.md, docs/design/contracts.md 6.2): a command the daemon runs in the workdir of the
//! run, with its outcome taken from the exit code: 0 — `pass`, 75 (`EX_TEMPFAIL`) — an attempt failure, else `fail`.

use super::{Exit, Msg, Outcome, RunHandler, Verdict, row::Row};
use crate::{
    driver::{Failure, FailureKind, Scope, Source},
    process::{self, Spec},
    writer::{Error, Tx},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::sync::Notify;

/// The output tail an exec step keeps, per stream.
const TAIL: usize = 4 * 1024;
/// The runner returns within the timeout + 4 s; past this the core takes the command for lost.
const LOST_SLACK_MS: i64 = 10_000;

impl RunHandler {
    /// The one attempt kind of an exec step: the command again, from the start.
    pub(super) fn start_exec(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        // No session to start: the command runs from the first moment.
        Self::run_attempt(tx, &mut row, "exec")?;
        let config: Value = (row.config.as_deref())
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        self.command(tx, row, &config["command"], false)
    }

    /// Runs `command` in the workdir of `row` with the environment of its step: the command of an exec step, or
    /// with `check` the check script of any step, which also sees the outcome of its step (docs/design/config.md).
    pub(super) fn command(&mut self, tx: &mut Tx<'_>, row: Row, command: &Value, check: bool) -> Result<(), Error> {
        let config: Value = (row.config.as_deref())
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        // The `env` values as the config gives them: the runner resolves their references, so no value of the
        // daemon's environment is ever stored (decision 137). A step of another kind keeps them under `exec`.
        let exec = config.get("exec").unwrap_or(&config);
        let env: serde_json::Map<String, Value> = (exec["env"].as_object().into_iter().flatten())
            .map(|(k, v)| (k.clone(), v["value"].clone()))
            .collect();
        // The tracker actor of the attempt (contracts 5.1), as for a session: the tracker attributes the command's
        // writes to the run.
        let mut context: Vec<(String, Value)> = (self.config.actor(&row.project, &row.id, row.attempt))
            .map(|(name, value)| (name, json!(value)))
            .into_iter()
            .collect();
        // The context of the run goes in the environment only, never into the command (decision 47).
        let inputs: serde_json::Map<String, Value> = serde_json::from_str(&row.inputs).unwrap_or_default();
        context.push(("LOOP_MANAGER_RUN".to_owned(), json!(row.id)));
        context.extend(
            row.bead
                .as_ref()
                .map(|bead| ("LOOP_MANAGER_BEAD".to_owned(), json!(bead))),
        );
        for (name, value) in inputs {
            let text = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
            context.push((crate::config::input_variable(&name), json!(text)));
        }
        // The outcomes of the earlier steps (docs/design/config.md): `LOOP_MANAGER_STEP_<NAME>_RESULT`, `_SUMMARY` and `_DATA`
        // (JSON).
        let except = if check { "" } else { row.step.as_str() };
        for (step, outcome) in super::render::step_results(tx.sql(), &row.id, except)? {
            let prefix = format!("LOOP_MANAGER_STEP_{}_", crate::config::upper(&step));
            for (field, value) in [("RESULT", &outcome["result"]), ("SUMMARY", &outcome["summary"])] {
                context.push((format!("{prefix}{field}"), value.clone()));
            }
            if !outcome["data"].is_null() {
                context.push((format!("{prefix}DATA"), json!(outcome["data"].to_string())));
            }
        }
        let remarks = super::render::remarks(tx.sql(), &row.id)?;
        context.extend((!remarks.is_empty()).then(|| ("LOOP_MANAGER_REMARKS".to_owned(), json!(remarks))));
        let timeout_ms = row.timeout_ms.unwrap_or(super::render::STEP_TIMEOUT_MS);
        // Run ids repeat across daemons; the marker that recovery kills the command by must not.
        let Some(token) = super::new_token() else {
            return self.terminal(
                tx,
                row,
                "failed",
                Some("infrastructure"),
                "no randomness for the marker",
            );
        };
        let payload = json!({
            "op": "exec", "run": row.id, "attempt": row.attempt, "command": command, "check": check,
            "cwd": row.workdir, "env": env, "inherit_env": exec["inherit_env"] == true, "context": context,
            "timeout_ms": timeout_ms, "marker": format!("x-{}", &token[..16]),
        });
        // Its own key: a `stop_exec` on the run's key must not queue behind the running command.
        let dedupe = if check {
            format!("check:{}", row.attempt)
        } else {
            row.attempt.to_string()
        };
        tx.effect(&format!("exec:{}", row.id), &dedupe, &payload)?;
        tx.set_deadline(&row.deadline("state"), tx.now_ms() + timeout_ms + LOST_SLACK_MS)
    }

    pub(super) fn exec_done(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        (attempt, check): (u32, bool),
        exit: Exit,
        stdout: &str,
        stderr: &str,
    ) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        if check {
            return self.check_done(tx, row, attempt, exit, (stdout, stderr));
        }
        if row.attempt != attempt || row.state != "running" {
            return Ok(());
        }
        let failure = |kind, text: Option<String>| {
            let mut failure = Failure::new(kind, Source::Core);
            failure.text = text;
            failure.raw = stderr.to_owned();
            failure
        };
        let (result, summary, code) = match exit {
            Exit::TimedOut => return self.attempt_failed(tx, row, failure(FailureKind::RunawayTurn, None)),
            Exit::SpawnFailed(text) => {
                return self.attempt_failed(tx, row, failure(FailureKind::WorkspaceFailure, Some(text)));
            }
            Exit::Code(75) => {
                let mut tempfail = failure(FailureKind::Unknown, Some("exit 75 (EX_TEMPFAIL)".to_owned()));
                tempfail.scope = Scope::Run;
                return self.attempt_failed(tx, row, tempfail);
            }
            Exit::Code(0) => (Verdict::Pass, "exit 0".to_owned(), Some(0)),
            Exit::Code(code) => (Verdict::Fail, format!("exit {code}"), Some(code)),
            Exit::Signal(signal) => (Verdict::Fail, format!("signal {signal}"), None),
        };
        let outcome = Outcome {
            result,
            summary,
            data: Some(json!({ "exit_code": code, "stdout": stdout, "stderr": stderr })),
        };
        self.step_done(tx, row, &outcome)
    }

    /// A new attempt of a step with no session (`harness` is `exec` or `drain`), `running` from its start.
    pub(super) fn run_attempt(tx: &mut Tx<'_>, row: &mut Row, harness: &str) -> Result<(), Error> {
        row.attempt += 1;
        tx.sql().execute(
            "UPDATE runs SET attempt = ?2, next = NULL WHERE id = ?1",
            params![row.id, row.attempt],
        )?;
        tx.sql().execute(
            "INSERT INTO attempts (run, n, kind, harness, started_ms, step) \
             VALUES (?1, ?2, 'new_session', ?3, ?4, ?5)",
            params![row.id, row.attempt, harness, tx.now_ms(), row.step],
        )?;
        row.set_state(tx, "running", None)
    }

    /// The step of a run with no session declared `outcome`: the run asks the hooks on `run.finishing`.
    pub(super) fn step_done(&mut self, tx: &mut Tx<'_>, mut row: Row, outcome: &Outcome) -> Result<(), Error> {
        tx.sql().execute(
            "UPDATE attempts SET outcome = ?3 WHERE run = ?1 AND n = ?2",
            params![row.id, row.attempt, json!(outcome).to_string()],
        )?;
        tx.clear_deadline(&row.deadline("state"))?;
        row.set_state(tx, "finishing", None)?;
        let data = json!({ "run": row.view(), "outcome": outcome, "remarks": 0 });
        self.deciding(tx, row, "run.finishing", data)
    }
}

/// The running commands of exec steps by `<run>:<attempt>`, for `stop_exec`.
pub(super) type Running = Arc<Mutex<HashMap<String, Arc<Notify>>>>;

/// Runs the command of an `exec` effect to its end and tells the core how it ended.
pub(super) async fn run(running: &Running, payload: &Value, socket: Option<&Path>) -> Msg {
    let command: Vec<String> = serde_json::from_value(payload["command"].clone()).unwrap_or_default();
    // The order of decision 137: the base or the daemon's whole environment, `env`, then loop-manager's own variables; a
    // later pair wins over an earlier of its name.
    let mut env = if payload["inherit_env"] == true {
        // shortcut: a variable of the daemon whose name or value is not UTF-8 is not inherited; pass `OsString`s
        // through `Spec` if one ever matters.
        std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect()
    } else {
        process::base_env()
    };
    let given: BTreeMap<String, String> = serde_json::from_value(payload["env"].clone()).unwrap_or_default();
    for (name, value) in given {
        match crate::config::env_value(&value, |n| std::env::var(n).ok()) {
            Ok(value) => env.push((name, value)),
            Err(e) => return refused(payload, format!("env `{name}`: {e}")),
        }
    }
    let context: Vec<(String, String)> = serde_json::from_value(payload["context"].clone()).unwrap_or_default();
    env.extend(context);
    let marker = payload["marker"].as_str().unwrap_or_default();
    env.push((crate::orphans::INSTANCE_ENV.to_owned(), marker.to_owned()));
    env.extend(socket.map(|s| ("LOOP_MANAGER_SOCKET".to_owned(), s.to_string_lossy().into_owned())));
    let stop = Arc::new(Notify::new());
    let lock = || running.lock().unwrap_or_else(PoisonError::into_inner);
    let key = key(payload);
    lock().insert(key.clone(), stop.clone());
    let out = process::run(Spec {
        command: &command,
        cwd: Path::new(payload["cwd"].as_str().unwrap_or("/")),
        env: &env,
        stdin: b"",
        timeout: Duration::from_millis(payload["timeout_ms"].as_u64().unwrap_or_default()),
        keep: TAIL,
        stop: Some(&stop),
    })
    .await;
    lock().remove(&key);
    done(payload, out.exit, &out.stdout, &out.stderr)
}

/// The message for a command that cannot start because its environment cannot be made: the step's attempt fails as
/// `config_error`; a check script fails the run as `config_error` (docs/design/config.md).
fn refused(payload: &Value, text: String) -> Msg {
    let run = payload["run"].as_str().unwrap_or_default().to_owned();
    let attempt = super::exec::attempt(payload);
    if payload["check"] == true {
        return Msg::CheckRefused { run, attempt, text };
    }
    let mut failure = Failure::new(FailureKind::ConfigError, Source::Core);
    failure.text = Some(text);
    Msg::AttemptFailed { run, attempt, failure }
}

/// The message that tells the core how the command of an `exec` effect ended.
pub(super) fn done(payload: &Value, exit: Exit, stdout: &[u8], stderr: &[u8]) -> Msg {
    Msg::ExecDone {
        run: payload["run"].as_str().unwrap_or_default().to_owned(),
        attempt: super::exec::attempt(payload),
        check: payload["check"] == true,
        exit,
        stdout: String::from_utf8_lossy(stdout).into_owned(),
        stderr: String::from_utf8_lossy(stderr).into_owned(),
    }
}

/// Ends the running command of the attempt a `stop_exec` effect names, if any. A `stop_exec` queued by a daemon
/// before attempts were named in it finds nothing: its command runs to its timeout, or recovery sweeps it.
// shortcut: a stop that comes between the commit of the `exec` effect and its start finds nothing, and the command
// runs to its end with its result dropped; the window is one task spawn — close it if recovery (WP-20) widens it.
pub(super) fn stop(running: &Running, payload: &Value) {
    if let Some(stop) = running
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key(payload))
    {
        stop.notify_one();
    }
}

/// The key of an attempt in [`Running`]: by attempt, so a late stop of the run's last attempt cannot end the
/// command of its next one.
fn key(payload: &Value) -> String {
    format!(
        "{}:{}",
        payload["run"].as_str().unwrap_or_default(),
        super::exec::attempt(payload)
    )
}

#[cfg(test)]
mod tests;
