//! The transitions of docs/design/contracts.md 6.2 that come from the harness, the operator and the clock: driver
//! events of the current attempt, answers to requests, cancel and the deadlines of 6.1.

use super::{
    RunHandler, answer_json, rejected,
    row::{Row, attempts},
};
use crate::{
    driver::{
        Answer, DriverEvent, Failure, FailureKind, InstanceHandle, Normalized, PromptKey, SessionRef, SessionState,
        Source,
    },
    journal::rfc3339_ms,
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::json;

/// Silence budgets of `running` (contracts 6.1).
const SILENCE_MS: i64 = 5 * 60_000;
const TOOL_SILENCE_MS: i64 = 10 * 60_000;
const BACKGROUND_SILENCE_MS: i64 = 30 * 60_000;
const HUMAN_TIMEOUT_MS: i64 = 3_600_000;

/// States in which an attempt is live and its session may report.
fn attempt_live(state: &str) -> bool {
    matches!(state, "starting" | "running" | "requires_action")
}

impl RunHandler {
    pub(super) fn cancel(&mut self, tx: &mut Tx<'_>, run: &str, reason: &str) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        if !row.live() {
            return Err(rejected(format!("`{run}` is already `{}`", row.state)));
        }
        self.terminal(tx, row, "cancelled", None, reason)
    }

    /// The current attempt of `run` is `attempt` and still live.
    fn current(tx: &Tx<'_>, run: &str, attempt: u32) -> Result<Option<Row>, Error> {
        let row = Row::load(tx, run)?;
        Ok((row.attempt == attempt && attempt_live(&row.state)).then_some(row))
    }

    pub(super) fn instance(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        attempt: u32,
        handle: &InstanceHandle,
    ) -> Result<(), Error> {
        let handle_json = json!(handle).to_string();
        if Self::current(tx, run, attempt)?.is_some() {
            tx.sql().execute(
                "UPDATE attempts SET instance = ?3, handle = ?4 WHERE run = ?1 AND n = ?2",
                params![run, attempt, handle.id, handle_json],
            )?;
            return Ok(());
        }
        // The attempt ended while its instance was starting: nothing may keep running for it.
        let harness: String = tx.sql().query_row(
            "SELECT harness FROM attempts WHERE run = ?1 AND n = ?2",
            params![run, attempt],
            |r| r.get(0),
        )?;
        let payload = json!({ "op": "stop", "harness": harness, "handle": handle, "session": null });
        tx.effect(run, &format!("stop:{attempt}:late"), &payload)
    }

    pub(super) fn session(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        attempt: u32,
        session: &SessionRef,
    ) -> Result<(), Error> {
        if Self::current(tx, run, attempt)?.is_none() {
            return Ok(());
        }
        tx.sql().execute(
            "UPDATE attempts SET session = ?3 WHERE run = ?1 AND n = ?2",
            params![run, attempt, session.0.to_string()],
        )?;
        Ok(())
    }

    pub(super) fn launch_failed(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        attempt: u32,
        failure: Failure,
    ) -> Result<(), Error> {
        match Self::current(tx, run, attempt)? {
            Some(row) => self.attempt_failed(tx, row, failure),
            None => Ok(()),
        }
    }

    /// Events of an instance the current attempt of no run owns are dropped: they are late.
    pub(super) fn driver_event(&mut self, tx: &mut Tx<'_>, event: DriverEvent) -> Result<(), Error> {
        let owner: Option<(String, u32)> = tx
            .sql()
            .query_row(
                "SELECT run, n FROM attempts WHERE instance = ?1 AND ended_ms IS NULL",
                [&event.instance],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((run, attempt)) = owner else { return Ok(()) };
        let Some(mut row) = Self::current(tx, &run, attempt)? else {
            return Ok(());
        };
        let silence = |tx: &mut Tx<'_>, row: &Row, ms: i64| tx.set_deadline(&row.deadline("silence"), tx.now_ms() + ms);
        match event.event {
            Normalized::PromptAcked { key } if row.state == "starting" => {
                if key != PromptKey::new(&row.id, attempt, 1) {
                    return Ok(());
                }
                row.set_state(tx, "running", None)?;
                let timeout = row.timeout_ms.unwrap_or(super::machine::STEP_TIMEOUT_MS);
                tx.set_deadline(&row.deadline("state"), tx.now_ms().saturating_add(timeout))?;
                silence(tx, &row, SILENCE_MS)
            }
            _ if row.state != "running" => Ok(()),
            Normalized::Failure(failure) => self.attempt_failed(tx, row, failure),
            Normalized::Exited { .. } => {
                self.attempt_failed(tx, row, Failure::new(FailureKind::HarnessCrash, Source::Adapter))
            }
            Normalized::Activity { tool_running: true } => silence(tx, &row, TOOL_SILENCE_MS),
            Normalized::TurnEnded { .. } => {
                tx.sql().execute(
                    "UPDATE attempts SET turn_ended = 1 WHERE run = ?1 AND n = ?2",
                    params![row.id, attempt],
                )?;
                silence(tx, &row, SILENCE_MS)
            }
            Normalized::State {
                state: SessionState::Idle,
                background: true,
            } => silence(tx, &row, BACKGROUND_SILENCE_MS),
            Normalized::State {
                state: SessionState::Idle,
                background: false,
            } => self.turn_settled(tx, row, attempt),
            Normalized::PermissionRequested {
                request_id,
                kind,
                tool,
                input,
                text,
            } => {
                let kind = match kind {
                    crate::driver::RequestKind::Permission => "permission",
                    crate::driver::RequestKind::Question => "question",
                };
                let request = json!({ "id": request_id, "kind": kind, "tool": tool, "input": input, "text": text });
                // Waiting for an answer is not working: the step timeout pauses until the answer.
                let state_at: Option<i64> = tx
                    .sql()
                    .query_row(
                        "SELECT at_ms FROM deadlines WHERE id = ?1",
                        [row.deadline("state")],
                        |r| r.get(0),
                    )
                    .optional()?;
                tx.sql().execute(
                    "UPDATE attempts SET request = ?3, paused_ms = ?4 WHERE run = ?1 AND n = ?2",
                    params![
                        row.id,
                        attempt,
                        request.to_string(),
                        state_at.map(|at| at - tx.now_ms())
                    ],
                )?;
                tx.clear_deadline(&row.deadline("state"))?;
                tx.clear_deadline(&row.deadline("silence"))?;
                row.set_state(tx, "requires_action", None)?;
                let at = tx.now_ms() + HUMAN_TIMEOUT_MS;
                tx.set_deadline(&row.deadline("human"), at)?;
                // shortcut: the event waits for a human only; hook decisions on it come with the hook runner (WP-15).
                row.emit(
                    tx,
                    "run.requires_action",
                    json!({ "run": row.view(), "request": request, "deadline": rfc3339_ms(at) }),
                )
                .map(drop)
            }
            _ => silence(tx, &row, SILENCE_MS),
        }
    }

    /// Idle without background: a turn that ended settles; one that has not ended yet is no news.
    fn turn_settled(&mut self, tx: &mut Tx<'_>, row: Row, attempt: u32) -> Result<(), Error> {
        let ended: bool = tx.sql().query_row(
            "SELECT turn_ended FROM attempts WHERE run = ?1 AND n = ?2",
            params![row.id, attempt],
            |r| r.get(0),
        )?;
        if !ended {
            return Ok(());
        }
        tx.sql().execute(
            "UPDATE attempts SET turn_ended = 0 WHERE run = ?1 AND n = ?2",
            params![row.id, attempt],
        )?;
        for what in ["state", "silence"] {
            tx.clear_deadline(&row.deadline(what))?;
        }
        let data = json!({ "run": row.view(), "stop_reason": "completed", "remarks": 0,
            "attempts": attempts(tx, &row.id)? });
        self.deciding(tx, row, "run.turn_ended_without_outcome", data)
    }

    pub(super) fn answer(&mut self, tx: &mut Tx<'_>, run: &str, request: &str, answer: &Answer) -> Result<(), Error> {
        let mut row = Row::load(tx, run)?;
        let open: Option<(String, String, String, String)> = tx
            .sql()
            .query_row(
                "SELECT json_extract(request, '$.id'), harness, handle, session FROM attempts \
                 WHERE run = ?1 AND n = ?2 AND request IS NOT NULL AND handle IS NOT NULL AND session IS NOT NULL",
                params![run, row.attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((id, harness, handle, session)) = open.filter(|o| row.state == "requires_action" && o.0 == request)
        else {
            return Err(rejected(format!("`{run}` has no open request `{request}`")));
        };
        let paused: Option<i64> = tx.sql().query_row(
            "UPDATE attempts SET request = NULL, paused_ms = NULL WHERE run = ?1 AND n = ?2 RETURNING paused_ms",
            params![run, row.attempt],
            |r| r.get(0),
        )?;
        tx.clear_deadline(&row.deadline("human"))?;
        if let Some(left) = paused {
            tx.set_deadline(&row.deadline("state"), tx.now_ms() + left)?;
        }
        let payload = json!({
            "op": "respond", "run": run, "attempt": row.attempt, "harness": harness, "request": id,
            "handle": serde_json::from_str::<serde_json::Value>(&handle).unwrap_or_default(),
            "session": serde_json::from_str::<serde_json::Value>(&session).unwrap_or_default(),
            "answer": answer_json(answer),
        });
        tx.effect(run, &format!("respond:{}:{id}", row.attempt), &payload)?;
        row.set_state(tx, "running", None)?;
        tx.set_deadline(&row.deadline("silence"), tx.now_ms() + SILENCE_MS)
    }

    pub(super) fn on_deadline(&mut self, tx: &mut Tx<'_>, run: &str, what: &str) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        if !row.live() {
            return Ok(());
        }
        let stall = |kind| Failure::new(kind, Source::Core);
        match (what, row.state.as_str()) {
            ("state", "queued") => self.terminal(tx, row, "failed", Some("infrastructure"), "queue_timeout"),
            ("state", "starting") | ("silence", "running") => self.attempt_failed(tx, row, stall(FailureKind::Stall)),
            ("state", "running") => self.attempt_failed(tx, row, stall(FailureKind::RunawayTurn)),
            ("state", "interrupted") => {
                let kind = row.next.clone().unwrap_or_else(|| "new_session".to_owned());
                self.start_attempt(tx, row, &kind)
            }
            ("human", "requires_action") => {
                let request: Option<String> = tx.sql().query_row(
                    "SELECT json_extract(request, '$.id') FROM attempts WHERE run = ?1 AND n = ?2",
                    params![run, row.attempt],
                    |r| r.get(0),
                )?;
                let deny = Answer::Deny {
                    message: "No answer came in time.".to_owned(),
                };
                self.answer(tx, run, &request.unwrap_or_default(), &deny)
            }
            ("decide", _) => self.apply(tx, row, None),
            _ => Ok(()),
        }
    }
}
