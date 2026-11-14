//! The transitions of docs/design/contracts.md 6.2 that come from the harness, the operator and the clock: driver
//! events of the current attempt, answers to requests, cancel and the deadlines of 6.1.

use super::{
    RunHandler, answer_json, rejected,
    row::{Row, attempts},
};
use crate::{
    driver::{Answer, DriverEvent, Failure, FailureKind, InstanceHandle, Normalized, SessionRef, SessionState, Source},
    journal::rfc3339_ms,
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

/// States in which an attempt is live and its session may report.
pub(super) fn attempt_live(state: &str) -> bool {
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

    pub(super) fn instance(tx: &mut Tx<'_>, run: &str, attempt: u32, handle: &InstanceHandle) -> Result<(), Error> {
        // Kept for an ended attempt too: its session, if one still starts, is closed through it.
        tx.sql().execute(
            "UPDATE attempts SET handle = ?3 WHERE run = ?1 AND n = ?2",
            params![run, attempt, json!(handle).to_string()],
        )?;
        Ok(())
    }

    pub(super) fn session(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        attempt: u32,
        session: &SessionRef,
    ) -> Result<(), Error> {
        if Self::current(tx, run, attempt)?.is_none() {
            // The attempt ended while its session was starting: the session is closed, its instance stays shared.
            let (harness, handle): (String, Option<String>) = tx.sql().query_row(
                "SELECT harness, handle FROM attempts WHERE run = ?1 AND n = ?2",
                params![run, attempt],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let handle: Value = handle.and_then(|h| serde_json::from_str(&h).ok()).unwrap_or_default();
            let payload = json!({ "op": "stop", "harness": harness, "handle": handle, "session": session.0 });
            return tx.effect(run, &format!("stop:{attempt}:late"), &payload);
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

    /// An event goes to the live attempts of its instance: of its session, or all of them for an instance-wide
    /// failure or exit. Events no live attempt owns are late and dropped.
    pub(super) fn driver_event(&mut self, tx: &mut Tx<'_>, event: DriverEvent) -> Result<(), Error> {
        if matches!(event.event, Normalized::Exited { .. }) {
            Self::instance_exited(tx, &event.instance)?;
        }
        let live: Vec<(String, u32, Option<String>)> = {
            let mut stmt = tx
                .sql()
                .prepare("SELECT run, n, session FROM attempts WHERE instance = ?1 AND ended_ms IS NULL")?;
            let rows = stmt.query_map([&event.instance], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let session = event.session.as_ref().map(|s| s.0.to_string());
        let wide = session.is_none() && matches!(event.event, Normalized::Failure(_) | Normalized::Exited { .. });
        let mut owners: Vec<_> = live
            .iter()
            .filter(|(_, _, s)| wide || (session.is_some() && *s == session))
            .collect();
        // A session-per-process harness may report before its session is known; an attempt with a known
        // session never takes another session's event.
        if owners.is_empty() && live.len() == 1 {
            owners = live.iter().filter(|(_, _, s)| s.is_none()).collect();
        }
        for (run, attempt, _) in owners {
            self.attempt_event(tx, run, *attempt, event.event.clone())?;
        }
        Ok(())
    }

    fn attempt_event(&mut self, tx: &mut Tx<'_>, run: &str, attempt: u32, event: Normalized) -> Result<(), Error> {
        let Some(mut row) = Self::current(tx, run, attempt)? else {
            return Ok(());
        };
        let w = self.config.watchdog;
        let silence = |tx: &mut Tx<'_>, row: &Row, ms: i64| tx.set_deadline(&row.deadline("silence"), tx.now_ms() + ms);
        if let Normalized::PromptAcked { key } = &event {
            super::prompts::advance(tx, &row.id, attempt, Some(*key), "acked")?;
        }
        match event {
            Normalized::PromptAcked { key } if row.state == "starting" => {
                if super::prompts::first(tx, &row.id, attempt)? != Some(key.0.to_string()) {
                    return Ok(());
                }
                row.set_state(tx, "running", None)?;
                let timeout = row.timeout_ms.unwrap_or(super::render::STEP_TIMEOUT_MS);
                tx.set_deadline(&row.deadline("state"), tx.now_ms().saturating_add(timeout))?;
                silence(tx, &row, w.silence_ms)
            }
            // A failure or an exit ends the attempt in every live state (contracts 6.2, invariant 5).
            Normalized::Failure(failure) => self.attempt_failed(tx, row, failure),
            Normalized::Exited { .. } => {
                self.attempt_failed(tx, row, Failure::new(FailureKind::HarnessCrash, Source::Adapter))
            }
            _ if row.state != "running" => Ok(()),
            Normalized::Activity { tool_running: true } => silence(tx, &row, w.tool_silence_ms),
            Normalized::TurnEnded { .. } => {
                super::prompts::advance(tx, &row.id, attempt, None, "settled")?;
                tx.sql().execute(
                    "UPDATE attempts SET turn_ended = 1 WHERE run = ?1 AND n = ?2",
                    params![row.id, attempt],
                )?;
                silence(tx, &row, w.silence_ms)
            }
            Normalized::State {
                state: SessionState::Idle,
                background: true,
            } => silence(tx, &row, w.background_silence_ms),
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
                Self::pause_step(tx, &row)?;
                tx.sql().execute(
                    "UPDATE attempts SET request = ?3 WHERE run = ?1 AND n = ?2",
                    params![row.id, attempt, request.to_string()],
                )?;
                row.set_state(tx, "requires_action", None)?;
                let at = tx.now_ms() + w.human_timeout_ms;
                tx.set_deadline(&row.deadline("human"), at)?;
                let seq = row.emit(
                    tx,
                    "run.requires_action",
                    json!({ "run": row.view(), "request": request, "deadline": rfc3339_ms(at) }),
                )?;
                // Hooks may answer first; a human may answer any time until `human_timeout` (contracts 3.3).
                if !self.hooks(Some(&row.project), "run.requires_action").is_empty() {
                    tx.sql()
                        .execute("UPDATE runs SET waiting = ?2 WHERE id = ?1", params![row.id, seq])?;
                }
                Ok(())
            }
            _ => silence(tx, &row, w.silence_ms),
        }
    }

    /// Idle without background: a turn that ended settles; one that has not ended yet is no news.
    fn turn_settled(&mut self, tx: &mut Tx<'_>, mut row: Row, attempt: u32) -> Result<(), Error> {
        let ended: bool = tx.sql().query_row(
            "SELECT turn_ended FROM attempts WHERE run = ?1 AND n = ?2",
            params![row.id, attempt],
            |r| r.get(0),
        )?;
        if !ended {
            return Ok(());
        }
        let (outcome, prompts): (Option<String>, i64) = tx.sql().query_row(
            "UPDATE attempts SET turn_ended = 0 WHERE run = ?1 AND n = ?2 RETURNING outcome, prompts",
            params![row.id, attempt],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // Between turns nothing works: the step timeout pauses until the next prompt (contracts 6.1).
        Self::pause_step(tx, &row)?;
        let remarks = prompts - 1;
        if let Some(outcome) = outcome {
            row.set_state(tx, "finishing", None)?;
            let outcome: serde_json::Value = serde_json::from_str(&outcome).unwrap_or_default();
            let data = json!({ "run": row.view(), "outcome": outcome, "remarks": remarks });
            return self.deciding(tx, row, "run.finishing", data);
        }
        let data = json!({ "run": row.view(), "stop_reason": "completed", "remarks": remarks,
            "attempts": attempts(tx.sql(), &row.id)? });
        self.deciding(tx, row, "run.turn_ended_without_outcome", data)
    }

    /// Keeps what is left of the step timeout in `paused_ms` and stops the step and silence clocks.
    pub(super) fn pause_step(tx: &mut Tx<'_>, row: &Row) -> Result<(), Error> {
        let state_at: Option<i64> = tx
            .sql()
            .query_row(
                "SELECT at_ms FROM deadlines WHERE id = ?1",
                [row.deadline("state")],
                |r| r.get(0),
            )
            .optional()?;
        tx.sql().execute(
            "UPDATE attempts SET paused_ms = ?3 WHERE run = ?1 AND n = ?2",
            params![row.id, row.attempt, state_at.map(|at| at - tx.now_ms())],
        )?;
        tx.clear_deadline(&row.deadline("state"))?;
        tx.clear_deadline(&row.deadline("silence"))
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
        // `RETURNING` would give the value after the update: read what was left first.
        let paused: Option<i64> = tx.sql().query_row(
            "SELECT paused_ms FROM attempts WHERE run = ?1 AND n = ?2",
            params![run, row.attempt],
            |r| r.get(0),
        )?;
        tx.sql().execute(
            "UPDATE attempts SET request = NULL, paused_ms = NULL WHERE run = ?1 AND n = ?2",
            params![run, row.attempt],
        )?;
        tx.clear_deadline(&row.deadline("human"))?;
        tx.sql()
            .execute("UPDATE runs SET waiting = NULL WHERE id = ?1", [run])?;
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
        tx.set_deadline(&row.deadline("silence"), tx.now_ms() + self.config.watchdog.silence_ms)
    }

    pub(super) fn on_deadline(&mut self, tx: &mut Tx<'_>, run: &str, what: &str) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        if !row.live() {
            return Ok(());
        }
        let stall = |kind| Failure::new(kind, Source::Core);
        match (what, row.state.as_str()) {
            ("state", "queued") => self.terminal(tx, row, "failed", Some("infrastructure"), "queue_timeout"),
            ("state", "parked") => self.terminal(tx, row, "failed", Some("infrastructure"), "park_timeout"),
            ("state", "preparing") => self.terminal(tx, row, "failed", Some("infrastructure"), "tracker_unavailable"),
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
                match self.answer(tx, run, &request.unwrap_or_default(), &deny) {
                    // Nothing to answer through: the run may not stay in `requires_action` without a deadline.
                    Err(Error::Rejected(_)) => self.attempt_failed(tx, row, stall(FailureKind::Unknown)),
                    other => other,
                }
            }
            ("decide", _) => self.decide_deadline(tx, row),
            _ => Ok(()),
        }
    }
}
