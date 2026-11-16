//! Repeats of a step and its check script (docs/design/config.md): a step that did not pass runs again while it has
//! executions left of its `max_attempts`; a check script, when the step has one, decides whether a step that passed on
//! its own did. A repeat enters the same step again within the round, so it spends no round and no attempt of the
//! session.

use super::{Exit, Outcome, RunHandler, Verdict, finish::remark, row::Row};
use crate::{
    config::Step,
    writer::{Error, Tx},
};
use rusqlite::params;
use serde_json::json;

/// The exit code of a check script that fails its step with no repeat.
const FATAL: i32 = 75;

impl RunHandler {
    /// The step of `row` passed on its own and the hooks accepted it: its check script decides, if it has one. The run
    /// waits for the script in `finishing`, with no decision pending.
    pub(super) fn check(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
        match self.step(&row).and_then(|s| s.check) {
            None => self.advance(tx, row, None),
            Some(script) => self.command(tx, row, &json!([script]), true),
        }
    }

    /// The check script of the step of `row` ended: 0 passes the step, 75 fails it for good, anything else fails
    /// this execution. Its word becomes the outcome of the execution, for the remark of a repeat and later steps.
    pub(super) fn check_done(
        &mut self,
        tx: &mut Tx<'_>,
        row: Row,
        attempt: u32,
        exit: Exit,
        (stdout, stderr): (&str, &str),
    ) -> Result<(), Error> {
        let waiting: Option<i64> = tx
            .sql()
            .query_row("SELECT waiting FROM runs WHERE id = ?1", [&row.id], |r| r.get(0))?;
        if row.attempt != attempt || row.state != "finishing" || waiting.is_some() {
            return Ok(());
        }
        tx.clear_deadline(&row.deadline("state"))?;
        let (said, code) = match exit {
            Exit::Code(0) => return self.advance(tx, row, None),
            Exit::SpawnFailed(text) => {
                let reason = format!("check script: {text}");
                return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
            }
            Exit::Code(code) => (format!("exit {code}"), Some(code)),
            Exit::Signal(signal) => (format!("signal {signal}"), None),
            Exit::TimedOut => ("timed out".to_owned(), None),
        };
        let outcome = Outcome {
            result: Verdict::Fail,
            summary: format!("check script: {said}"),
            data: Some(json!({ "exit_code": code, "stdout": stdout, "stderr": stderr })),
        };
        tx.sql().execute(
            "UPDATE attempts SET outcome = ?3 WHERE run = ?1 AND n = ?2",
            params![row.id, row.attempt, json!(outcome).to_string()],
        )?;
        self.repeat(tx, row, "check", code == Some(FATAL))
    }

    /// The execution of the step of `row` did not pass: the step runs again, with the remark of this execution,
    /// while it has executions left and the failure is not `fatal`; else it fails with `class`. A `fatal` failure ends
    /// the run at once.
    pub(super) fn repeat(&mut self, tx: &mut Tx<'_>, row: Row, class: &str, fatal: bool) -> Result<(), Error> {
        let outcome = Self::outcome(tx, &row)?.unwrap_or_else(|| Outcome {
            result: Verdict::Fail,
            summary: "outcome lost".to_owned(),
            data: None,
        });
        match self.step(&row) {
            // A failure for good is no work to send back either: it skips the rounds of a check step's `back_to`.
            _ if fatal => self.terminal(tx, row, "failed", Some(class), &outcome.summary),
            Some(step) if row.step_attempt < step.max_attempts => {
                let again = remark(&row.step, &outcome);
                self.enter(tx, row, step, Some(&again))
            }
            _ => self.advance(tx, row, Some((class, &outcome.summary))),
        }
    }

    fn step(&self, row: &Row) -> Option<Step> {
        let project = self.config.projects.iter().find(|p| p.name == row.project)?;
        let (_, formula) = project.formula(&row.formula).ok()?;
        formula.steps.iter().find(|s| s.name == row.step).cloned()
    }
}
