//! The finisher and the end of a run (docs/design/contracts.md 6.3, spec §9.4): the declared outcome, the decisions
//! on `run.finishing` and the next prompt of an attempt after a remark or a `continue`.

use super::{Outcome, RunHandler, Verdict, events::attempt_live, row::Row, token_sha256};
use crate::{
    driver::{Failure, FailureKind, PromptKey, Source},
    writer::{Error, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

/// Ceiling of the core: remarks per attempt, `continue` and `remark` alike (invariant 16).
pub const MAX_REMARKS: i64 = 10;
const MAX_SUMMARY: usize = 4 * 1024;
const MAX_DATA: usize = 64 * 1024;

impl RunHandler {
    pub(super) fn finish(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        token: &str,
        outcome: &Outcome,
    ) -> Result<Value, Error> {
        let answer = |a: &str| Ok(json!({ "answer": a }));
        let row = match Row::load(tx, run) {
            Ok(row) => row,
            Err(Error::Rejected(_)) => return answer("invalid"),
            Err(e) => return Err(e),
        };
        let n: Option<u32> = tx
            .sql()
            .query_row(
                "SELECT n FROM attempts WHERE run = ?1 AND token_sha256 = ?2",
                params![run, token_sha256(token)],
                |r| r.get(0),
            )
            .optional()?;
        let Some(n) = n else { return answer("invalid") };
        if !row.live() {
            return answer("run_terminal");
        }
        if n != row.attempt || !(attempt_live(&row.state) || row.state == "finishing") {
            return answer("stale_attempt");
        }
        let data_len = outcome.data.as_ref().map_or(0, |d| d.to_string().len());
        // In `finishing` the declared outcome is under check; a `running` run that waits for a decision has ended
        // its turn without one (contracts 6.3). Either way nothing may be declared now.
        let waiting: Option<i64> = tx
            .sql()
            .query_row("SELECT waiting FROM runs WHERE id = ?1", [run], |r| r.get(0))?;
        let closed = row.state == "finishing" || (row.state == "running" && waiting.is_some());
        if closed || outcome.summary.len() > MAX_SUMMARY || data_len > MAX_DATA {
            return answer("invalid");
        }
        tx.sql().execute(
            "UPDATE attempts SET outcome = ?3 WHERE run = ?1 AND n = ?2",
            params![run, n, json!(outcome).to_string()],
        )?;
        answer("accepted")
    }

    /// The outcome of the current attempt as it was declared, if it was.
    pub(super) fn outcome(tx: &Tx<'_>, row: &Row) -> Result<Option<Outcome>, Error> {
        let text: Option<String> = tx
            .sql()
            .query_row(
                "SELECT outcome FROM attempts WHERE run = ?1 AND n = ?2",
                params![row.id, row.attempt],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
    }

    /// Accept or veto on `run.finishing`; a veto on a declared `fail` changes nothing (contracts 3.3).
    pub(super) fn finished(&mut self, tx: &mut Tx<'_>, row: Row, veto: Option<&str>) -> Result<(), Error> {
        let outcome = Self::outcome(tx, &row)?;
        match (outcome.map(|o| (o.result, o.summary)), veto) {
            (Some((Verdict::Pass, _)), None) => self.advance(tx, row),
            (Some((Verdict::Pass, _)), Some(reason)) => self.terminal(tx, row, "failed", Some("check"), reason),
            (Some((Verdict::Fail, summary)), _) => self.terminal(tx, row, "failed", Some("agent"), &summary),
            (None, _) => self.terminal(tx, row, "failed", Some("infrastructure"), "outcome lost"),
        }
    }

    /// After a passed step: the next step of the formula starts at once (spec §9.1); after the last, the run passes.
    fn advance(&mut self, tx: &mut Tx<'_>, mut row: Row) -> Result<(), Error> {
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let formula = project.map(|p| p.formula(&row.formula)).transpose();
        let next = match formula {
            Ok(Some((_, f))) if f.step(&row.step).is_some() => f.next(&row.step).cloned(),
            _ => {
                let reason = format!("step `{}` is gone from `{}`", row.step, row.formula);
                return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
            }
        };
        let Some(next) = next else {
            return self.terminal(tx, row, "passed", None, "pass");
        };
        let agent = self.step_agent(&row.project, &row.rig, &row.formula, &next);
        let agent = match agent {
            Ok((agent, _)) => agent,
            Err(reason) => return self.terminal(tx, row, "failed", Some("infrastructure"), &reason),
        };
        let outcome = Self::outcome(tx, &row)?;
        self.end_attempt(tx, &row, None)?;
        row.emit(
            tx,
            "run.step_finished",
            json!({ "run": row.view(), "outcome": outcome }),
        )?;
        tx.sql().execute(
            "UPDATE runs SET step = ?2, agent = ?3 WHERE id = ?1",
            params![row.id, next.name, agent],
        )?;
        (row.step, row.agent) = (next.name, agent);
        row.set_state(tx, "preparing", None)?;
        self.render_and_prepare(tx, row)
    }

    /// Sends `text` as the next prompt of the current attempt: the declared outcome is cleared, the token stays
    /// valid, the step timeout resumes with what it had left. Past the remarks ceiling the run fails with `class`;
    /// with no session to take the prompt, the attempt fails with `daemon_crash`.
    pub(super) fn next_prompt(&mut self, tx: &mut Tx<'_>, mut row: Row, text: &str, class: &str) -> Result<(), Error> {
        let (prompts, paused, harness, handle, session): (i64, Option<i64>, String, Option<String>, Option<String>) =
            tx.sql().query_row(
                "SELECT prompts, paused_ms, harness, handle, session FROM attempts WHERE run = ?1 AND n = ?2",
                params![row.id, row.attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
        if prompts > MAX_REMARKS {
            return self.terminal(tx, row, "failed", Some(class), "remarks_ceiling");
        }
        let n = u32::try_from(prompts + 1).unwrap_or(u32::MAX);
        let key = PromptKey::new(&row.id, row.attempt, n);
        super::prompts::intent(tx, &row.id, row.attempt, n, key, text)?;
        // The session of the turn died with the daemon that ran it (recovery let go of its handle), or dies with the
        // one that is stopping now: no next turn starts, the attempt fails for the policy to decide (spec §5.6), and
        // the prompt left as an intent goes to the session a resumed attempt brings back (contracts 6.4).
        let (Some(handle), Some(session), None) = (handle, session, &self.stopping) else {
            return self.attempt_failed(tx, row, Failure::new(FailureKind::DaemonCrash, Source::Core));
        };
        tx.sql().execute(
            "UPDATE attempts SET outcome = NULL, paused_ms = NULL, prompts = ?3 WHERE run = ?1 AND n = ?2",
            params![row.id, row.attempt, prompts + 1],
        )?;
        if let Some(left) = paused {
            tx.set_deadline(&row.deadline("state"), tx.now_ms() + left)?;
        }
        tx.set_deadline(&row.deadline("silence"), tx.now_ms() + self.config.watchdog.silence_ms)?;
        if row.state != "running" {
            row.set_state(tx, "running", None)?;
        }
        let payload = json!({
            "op": "prompt", "run": row.id, "attempt": row.attempt, "harness": harness, "text": text,
            "handle": serde_json::from_str::<Value>(&handle).unwrap_or_default(),
            "session": serde_json::from_str::<Value>(&session).unwrap_or_default(),
            "key": key.0.to_string(),
        });
        tx.effect(&row.id, &format!("prompt:{}:{}", row.attempt, prompts + 1), &payload)
    }
}
