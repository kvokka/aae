//! Deciding events (docs/design/contracts.md 3.2–3.4) as the run machine waits on them: their deadline, the
//! decisions it applies and the retry of an interrupted run.

use super::{Decision, MAX_ATTEMPTS, RunHandler, rejected, row::Row};
use crate::{
    journal::rfc3339_ms,
    writer::{Error, Tx},
};
use rusqlite::params;
use serde_json::{Value, json};

/// The slack of a deciding event's deadline (contracts 3.3).
const DECIDE_SLACK_MS: i64 = 5_000;

impl RunHandler {
    /// Emits a deciding event; with no hook on its type the "no plugin decision" outcome applies at once, else at
    /// the deadline of contracts 3.3 unless a decision comes first.
    pub(super) fn deciding(&mut self, tx: &mut Tx<'_>, row: Row, kind: &str, mut data: Value) -> Result<(), Error> {
        let timeouts: Vec<i64> = self
            .hooks(Some(&row.project), kind)
            .iter()
            .filter_map(|h| h["timeout_ms"].as_i64())
            .collect();
        let at = tx.now_ms() + timeouts.iter().sum::<i64>() + DECIDE_SLACK_MS * i64::from(!timeouts.is_empty());
        data["deadline"] = json!(rfc3339_ms(at));
        let seq = row.emit(tx, kind, data)?;
        if timeouts.is_empty() {
            return self.apply(tx, row, None);
        }
        tx.sql()
            .execute("UPDATE runs SET waiting = ?2 WHERE id = ?1", params![row.id, seq])?;
        tx.set_deadline(&row.deadline("decide"), at)
    }

    pub(super) fn decide(&mut self, tx: &mut Tx<'_>, run: &str, seq: i64, decision: Decision) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        let waiting: Option<i64> = tx
            .sql()
            .query_row("SELECT waiting FROM runs WHERE id = ?1", [run], |r| r.get(0))?;
        if waiting != Some(seq) {
            return Err(rejected(format!("`{run}` does not wait for a decision on event {seq}")));
        }
        self.apply(tx, row, Some(decision).filter(|d| *d != Decision::Abstain))
    }

    /// The deadline of a deciding event passed with no word from the hook runner: its hooks count as failed, which
    /// blocks on the veto events and is "no plugin decision" on the others (contracts 3.3).
    pub(super) fn decide_deadline(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
        match row.state.as_str() {
            "preparing" => self.terminal(tx, row, "failed", Some("check"), "hook_deadline"),
            "finishing" => self.finished(tx, row, Some("hook_deadline")),
            _ => self.apply(tx, row, None),
        }
    }

    /// Applies a decision, `None` being "no plugin decision", to the deciding event the run's state waits on.
    pub(super) fn apply(&mut self, tx: &mut Tx<'_>, row: Row, decision: Option<Decision>) -> Result<(), Error> {
        tx.sql()
            .execute("UPDATE runs SET waiting = NULL WHERE id = ?1", [&row.id])?;
        tx.clear_deadline(&row.deadline("decide"))?;
        match (row.state.as_str(), decision) {
            ("preparing", None) => self.start_attempt(tx, row, "new_session"),
            (
                "preparing",
                Some(Decision::Proceed {
                    workdir,
                    env,
                    prompt_append,
                }),
            ) => self.prepared(tx, row, workdir, env, prompt_append),
            // A hook without an answer leaves the request to a human until `human_timeout`.
            ("requires_action", None) => Ok(()),
            ("requires_action", Some(Decision::Answer(answer))) => {
                let request: String = tx.sql().query_row(
                    "SELECT json_extract(request, '$.id') FROM attempts WHERE run = ?1 AND n = ?2",
                    params![row.id, row.attempt],
                    |r| r.get(0),
                )?;
                self.answer(tx, &row.id, &request, &answer)
            }
            ("preparing", Some(Decision::Deny { reason })) => self.terminal(tx, row, "failed", Some("check"), &reason),
            ("interrupted", None) => self.terminal(tx, row, "failed", Some("infrastructure"), "no_decision"),
            ("interrupted", Some(Decision::Fail { reason })) => {
                self.terminal(tx, row, "failed", Some("infrastructure"), &reason)
            }
            ("interrupted", Some(Decision::Resume { after })) => self.retry(tx, row, "resume", after),
            ("interrupted", Some(Decision::NewSession { after })) => self.retry(tx, row, "new_session", after),
            ("running", None) => self.terminal(tx, row, "failed", Some("hollow_done"), "no_decision"),
            ("running", Some(Decision::Fail { reason })) => {
                self.terminal(tx, row, "failed", Some("hollow_done"), &reason)
            }
            ("running", Some(Decision::Continue { message })) => self.next_prompt(tx, row, &message, "hollow_done"),
            ("finishing", None | Some(Decision::Accept)) => self.finished(tx, row, None),
            ("finishing", Some(Decision::Veto { reason })) => self.finished(tx, row, Some(&reason)),
            ("finishing", Some(Decision::Remark { message })) => self.next_prompt(tx, row, &message, "check"),
            (state, Some(d)) => Err(rejected(format!("{d:?} does not apply to a run in `{state}`"))),
            (state, None) => Err(rejected(format!("a run in `{state}` waits for no decision"))),
        }
    }

    /// Schedules the next attempt of an `interrupted` run after `after`, within the attempts ceiling.
    fn retry(&mut self, tx: &mut Tx<'_>, row: Row, kind: &str, after: std::time::Duration) -> Result<(), Error> {
        if row.attempt >= MAX_ATTEMPTS {
            return self.terminal(tx, row, "failed", Some("infrastructure"), "attempts_ceiling");
        }
        tx.sql()
            .execute("UPDATE runs SET next = ?2 WHERE id = ?1", params![row.id, kind])?;
        let after = i64::try_from(after.as_millis()).unwrap_or(i64::MAX);
        tx.set_deadline(&row.deadline("state"), tx.now_ms().saturating_add(after))
    }
}
