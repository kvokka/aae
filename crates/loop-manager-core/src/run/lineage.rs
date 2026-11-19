//! The prompt of the step a run enters (docs/design/contracts.md 2.3, 5): rendered at once, or, when it shows the
//! comments or the parents of the run's bead, once the tracker has given them as they are now. The read is an effect
//! on the queue of the project's tracker, and the run waits for it in `preparing`, as it does for its claim.

use super::{Msg, RunHandler, beads::tracker_key, row::Row};
use crate::{
    config::Prompt,
    tracker::{Lineage, Tracker, TrackerError},
    writer::{CoreHandle, Error, Tx},
};
use rusqlite::params;
use serde_json::{Value, json};

impl RunHandler {
    /// Renders the prompt of the step `row` enters and asks the `run.preparing` hooks; a prompt that shows the
    /// comments or the parents of the run's bead reads them first.
    pub(super) fn render_and_prepare(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
        let shows =
            row.step_kind == "ai" && (self.config.prompt(&row.project, &row.agent)).is_ok_and(Prompt::shows_lineage);
        let Some(bead) = row.bead.as_ref().filter(|_| shows) else {
            return self.render_now(tx, row);
        };
        // The step and the attempt name this entry of the step: an answer to another one is dropped.
        let payload = json!({ "op": "tracker", "call": "lineage", "project": row.project, "bead": bead, "run": row.id,
            "step": row.step, "attempt": row.attempt });
        let dedupe = format!("lineage:{}:{}:{}", row.id, row.step, row.attempt);
        tx.effect(&tracker_key(&row.project), &dedupe, &payload)?;
        tx.set_deadline(
            &row.deadline("state"),
            tx.now_ms() + self.config.watchdog.start_timeout_ms,
        )
    }

    /// Renders the prompt and asks the `run.preparing` hooks.
    fn render_now(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        let prepared = self.render(tx, &row);
        let (prompt, timeout_ms, config) = match prepared {
            Ok(Some(p)) => p,
            Ok(None) => return self.skip(tx, row),
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

    /// The tracker answered the read of `payload`: the bead as it is now replaces the run's snapshot of it and the
    /// prompt renders. An answer to another entry of a step, or to a run that prepares no more, is dropped.
    pub(super) fn lineage(
        &mut self,
        tx: &mut Tx<'_>,
        payload: &Value,
        result: Result<Lineage, TrackerError>,
    ) -> Result<(), Error> {
        let row = Row::load(tx, payload["run"].as_str().unwrap_or_default())?;
        if row.state != "preparing" || payload["step"] != json!(row.step) || payload["attempt"] != json!(row.attempt) {
            return Ok(());
        }
        match result {
            Ok(lineage) => {
                tx.clear_deadline(&row.deadline("state"))?;
                tx.sql().execute(
                    "UPDATE runs SET bead_data = ?2 WHERE id = ?1",
                    params![row.id, json!(lineage).to_string()],
                )?;
                self.render_now(tx, Row::load(tx, &row.id)?)
            }
            // Reported and tried again by its queue; the run waits on, up to `start_timeout`.
            Err(e @ TrackerError::Unavailable { .. }) => self.tracked(tx, payload, Err(e)),
            Err(e) => self.terminal(
                tx,
                row,
                "failed",
                Some("infrastructure"),
                &format!("bead_unreadable: {e}"),
            ),
        }
    }
}

/// Runs the read of the effect `payload` and reports it; `false` retries it (the tracker was unavailable).
pub(super) async fn read(tracker: &dyn Tracker, core: &CoreHandle<Msg>, payload: &Value) -> bool {
    let result = tracker.lineage(payload["bead"].as_str().unwrap_or_default()).await;
    let retry = matches!(result, Err(TrackerError::Unavailable { .. }));
    // A stopped writer drops the message; the effect is still queued and runs again at the next start.
    let _ = core
        .send(
            None,
            Msg::Lineage {
                payload: payload.clone(),
                result,
            },
        )
        .await;
    !retry
}
