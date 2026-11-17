//! The await step (docs/design/config.md, docs/design/contracts.md 6): an attempt with no process that waits for an
//! outcome given from outside, through the API or the CLI, until its timeout.

use super::{
    Outcome, RunHandler, Verdict,
    finish::{MAX_DATA, MAX_SUMMARY},
    render::AWAIT_TIMEOUT_MS,
    row::Row,
};
use crate::{
    journal::rfc3339_ms,
    writer::{Error, Tx},
};
use serde_json::{Value, json};

impl RunHandler {
    /// The attempt of an await step: the wait has the step's timeout as its deadline and frees the run's slot.
    pub(super) fn start_await(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        Self::run_attempt(tx, &mut row, "await")?;
        let at = tx.now_ms().saturating_add(row.timeout_ms.unwrap_or(AWAIT_TIMEOUT_MS));
        tx.set_deadline(&row.deadline("state"), at)?;
        row.emit(
            tx,
            "run.awaiting",
            json!({ "run": row.view(), "deadline": rfc3339_ms(at) }),
        )?;
        self.schedule(tx)
    }

    /// The outcome of the await step `step` of `run`, given from outside; refused when the run does not wait there.
    pub(super) fn resolve(&mut self, tx: &mut Tx<'_>, run: &str, step: &str, outcome: &Outcome) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        if row.state != "running" || row.step_kind != "await" || row.step != step {
            let at = format!("`{run}` is {} on {} step `{}`", row.state, row.step_kind, row.step);
            return Err(Error::Rejected(format!("{at}, not waiting on `{step}`")));
        }
        let data = outcome.data.as_ref().map_or(0, |d| d.to_string().len());
        if outcome.summary.len() > MAX_SUMMARY || data > MAX_DATA {
            let limits = format!("over {MAX_SUMMARY} bytes of summary or {MAX_DATA} bytes of data");
            return Err(Error::Rejected(format!("the outcome is {limits}")));
        }
        self.step_done(tx, row, outcome)
    }

    /// The end of a wait with no outcome: the run fails with no return, or the step passes as its config says.
    pub(super) fn await_timeout(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
        let config: Value = (row.config.as_deref())
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        if config["fail_on_timeout"] == false {
            let outcome = Outcome {
                result: Verdict::Pass,
                summary: "timeout".to_owned(),
                data: None,
            };
            return self.step_done(tx, row, &outcome);
        }
        self.terminal(tx, row, "failed", Some("check"), "await_timeout")
    }
}
