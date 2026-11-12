//! The exec step (spec §9.1, docs/design/contracts.md 6.2): a command the daemon runs in the workdir of the run,
//! with its outcome taken from the exit code: 0 — `pass`, 75 (`EX_TEMPFAIL`) — an attempt failure, else `fail`.

use super::{Exit, Msg, Outcome, RunHandler, Verdict, row::Row};
use crate::{
    driver::{Failure, FailureKind, Scope, Source},
    process::{self, Spec},
    writer::{Error, Tx},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
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
        let config: Value = row
            .config
            .as_deref()
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        row.attempt += 1;
        tx.sql().execute(
            "UPDATE runs SET attempt = ?2, next = NULL WHERE id = ?1",
            params![row.id, row.attempt],
        )?;
        tx.sql().execute(
            "INSERT INTO attempts (run, n, kind, harness, started_ms) VALUES (?1, ?2, 'new_session', 'exec', ?3)",
            params![row.id, row.attempt, tx.now_ms()],
        )?;
        // No session to start: the command runs from the first moment.
        row.set_state(tx, "running", None)?;
        let env: Vec<(&String, &Value)> = config["env"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k, &v["value"]))
            .collect();
        let timeout_ms = row.timeout_ms.unwrap_or(super::machine::STEP_TIMEOUT_MS);
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
            "op": "exec", "run": row.id, "attempt": row.attempt, "command": config["command"],
            "cwd": row.workdir, "env": env, "timeout_ms": timeout_ms, "marker": format!("x-{}", &token[..16]),
        });
        // Its own key: a `stop_exec` on the run's key must not queue behind the running command.
        tx.effect(&format!("exec:{}", row.id), &row.attempt.to_string(), &payload)?;
        tx.set_deadline(&row.deadline("state"), tx.now_ms() + timeout_ms + LOST_SLACK_MS)
    }

    pub(super) fn exec_done(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        attempt: u32,
        exit: Exit,
        stdout: &str,
        stderr: &str,
    ) -> Result<(), Error> {
        let mut row = Row::load(tx, run)?;
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
        tx.sql().execute(
            "UPDATE attempts SET outcome = ?3 WHERE run = ?1 AND n = ?2",
            params![run, attempt, json!(outcome).to_string()],
        )?;
        tx.clear_deadline(&row.deadline("state"))?;
        row.set_state(tx, "finishing", None)?;
        let data = json!({ "run": row.view(), "outcome": outcome, "remarks": 0 });
        self.deciding(tx, row, "run.finishing", data)
    }
}

/// The running commands of exec steps by run, for `stop_exec`.
pub(super) type Running = Arc<Mutex<HashMap<String, Arc<Notify>>>>;

/// Runs the command of an `exec` effect to its end and tells the core how it ended.
pub(super) async fn run(running: &Running, payload: &Value) -> Msg {
    let run = payload["run"].as_str().unwrap_or_default().to_owned();
    let command: Vec<String> = serde_json::from_value(payload["command"].clone()).unwrap_or_default();
    let mut env = process::base_env();
    let pairs: Vec<(String, String)> = serde_json::from_value(payload["env"].clone()).unwrap_or_default();
    env.retain(|(k, _)| !pairs.iter().any(|(p, _)| p == k));
    env.extend(pairs);
    let marker = payload["marker"].as_str().unwrap_or_default();
    env.push((crate::orphans::INSTANCE_ENV.to_owned(), marker.to_owned()));
    let stop = Arc::new(Notify::new());
    let lock = || running.lock().unwrap_or_else(PoisonError::into_inner);
    lock().insert(run.clone(), stop.clone());
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
    lock().remove(&run);
    Msg::ExecDone {
        run,
        attempt: super::exec::attempt(payload),
        exit: out.exit,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Ends the running command of `run`, if any.
// shortcut: a stop that comes between the commit of the `exec` effect and its start finds nothing, and the command
// runs to its end with its result dropped; the window is one task spawn — close it if recovery (WP-20) widens it.
pub(super) fn stop(running: &Running, run: &str) {
    if let Some(stop) = running.lock().unwrap_or_else(PoisonError::into_inner).get(run) {
        stop.notify_one();
    }
}
