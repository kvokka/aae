//! The finisher and the end of a run (docs/design/contracts.md 6.3, spec §9.4): the declared outcome, the decisions
//! on `run.finishing` and the next prompt of an attempt after a remark or a `continue`.

use super::{Outcome, RunHandler, Verdict, events::attempt_live, row::Row, token_sha256};
use crate::{
    config::Step,
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
        Self::stop_background(tx, &row)?;
        answer("accepted")
    }

    /// Once the outcome is declared and the turn has ended, the commands the session left running go: the adapter
    /// stops them, and the session's idle without background then moves the run to `finishing` (spec §9.4).
    pub(super) fn stop_background(tx: &mut Tx<'_>, row: &Row) -> Result<(), Error> {
        let found: Option<(String, String, String)> = tx
            .sql()
            .query_row(
                "SELECT harness, handle, session FROM attempts WHERE run = ?1 AND n = ?2 AND turn_ended != 0 \
                 AND outcome IS NOT NULL AND handle IS NOT NULL AND session IS NOT NULL",
                params![row.id, row.attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((harness, handle, session)) = found else {
            return Ok(());
        };
        let payload = json!({
            "op": "stop_background", "run": row.id, "attempt": row.attempt, "harness": harness,
            "handle": serde_json::from_str::<Value>(&handle).unwrap_or_default(),
            "session": serde_json::from_str::<Value>(&session).unwrap_or_default(),
        });
        tx.effect(&row.id, &format!("stop_background:{}", row.attempt), &payload)
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
            (Some((Verdict::Pass, _)), None) => self.check(tx, row),
            (Some((Verdict::Pass, _)), Some(reason)) => self.terminal(tx, row, "failed", Some("check"), reason),
            // A step that fails on its own runs again with no check script (spec §9.1).
            (Some((Verdict::Fail, _)), _) => self.repeat(tx, row, "agent", false),
            (None, _) => self.terminal(tx, row, "failed", Some("infrastructure"), "outcome lost"),
        }
    }

    /// After a step that passed, or failed for the class and reason `failed` (spec §9.1): the next step starts at once; the
    /// run passes after the last. A failed step fails the run unless it is a check (`back_to`). Checks in a row all
    /// run; after the last of them, any that failed send the run back together, to the earliest step they name, for
    /// the next round; past the formula's last round the run fails with their remarks.
    pub(super) fn advance(&mut self, tx: &mut Tx<'_>, row: Row, failed: Option<(&str, &str)>) -> Result<(), Error> {
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let formula = project.and_then(|p| p.formula(&row.formula).ok()).map(|(_, f)| f);
        let at = formula.and_then(|f| f.steps.iter().position(|s| s.name == row.step));
        let (Some(f), Some(at)) = (formula, at) else {
            let reason = format!("step `{}` is gone from `{}`", row.step, row.formula);
            return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
        };
        let checks: Vec<_> = f.steps[..=at]
            .iter()
            .rev()
            .take_while(|s| s.back_to.is_some())
            .collect();
        let next = f.steps.get(at + 1);
        if let Some((class, reason)) = failed.filter(|_| checks.is_empty()) {
            return self.terminal(tx, row, "failed", Some(class), reason);
        }
        if !checks.is_empty() && next.is_none_or(|n| n.back_to.is_none()) {
            let mut sent_back = Vec::new();
            for check in checks.iter().rev() {
                let outcome: Option<String> = tx
                    .sql()
                    .query_row(
                        "SELECT outcome FROM attempts WHERE run = ?1 AND step = ?2 AND outcome IS NOT NULL \
                         ORDER BY n DESC LIMIT 1",
                        params![row.id, check.name],
                        |r| r.get(0),
                    )
                    .optional()?;
                let outcome: Option<Outcome> = outcome.and_then(|o| serde_json::from_str(&o).ok());
                if let Some(o) = outcome.filter(|o| o.result == Verdict::Fail) {
                    sent_back.push((check.back_to.clone().unwrap_or_default(), remark(&check.name, &o)));
                }
            }
            let to = f.steps.iter().find(|s| sent_back.iter().any(|(to, _)| *to == s.name));
            if let Some(to) = to {
                let remarks = sent_back.into_iter().map(|(_, r)| r).collect::<Vec<_>>().join("\n\n");
                let round: u32 = tx
                    .sql()
                    .query_row("SELECT round FROM runs WHERE id = ?1", [&row.id], |r| r.get(0))?;
                if round >= f.rounds {
                    let reason = format!("still failing after {round} rounds: {remarks}");
                    return self.terminal(tx, row, "failed", Some("agent"), &reason);
                }
                tx.sql().execute(
                    "UPDATE runs SET round = round + 1, remarks = ?2 WHERE id = ?1",
                    params![row.id, remarks],
                )?;
                return self.enter(tx, row, to.clone(), None);
            }
        }
        match next {
            Some(next) => self.enter(tx, row, next.clone(), None),
            None => self.terminal(tx, row, "passed", None, "pass"),
        }
    }

    /// The run ends its step and starts `next` at once: its first execution, or with the remark `again` the next one
    /// of the same step (spec §9.1).
    pub(super) fn enter(
        &mut self,
        tx: &mut Tx<'_>,
        mut row: Row,
        next: Step,
        again: Option<&str>,
    ) -> Result<(), Error> {
        let agent = self.step_agent(&row.project, &row.rig, &row.formula, &next, row.as_agent.as_deref());
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
            "UPDATE runs SET step = ?2, agent = ?3, step_kind = ?4, step_remarks = ?5, \
             step_attempt = CASE WHEN ?5 IS NULL THEN 1 ELSE step_attempt + 1 END WHERE id = ?1",
            params![row.id, next.name, agent, next.kind.name(), again],
        )?;
        row.step_attempt = if again.is_some() { row.step_attempt + 1 } else { 1 };
        let left = std::mem::replace(&mut row.agent, agent);
        (row.step, row.step_kind) = (next.name, next.kind.name().to_owned());
        let frees = left != row.agent && self.config.max_runs(&row.project, &left).is_some();
        row.set_state(tx, "preparing", None)?;
        self.render_and_prepare(tx, row)?;
        // The step left gives up a slot of its agent's `max_runs`: a run parked for it may go (spec §9.3).
        if frees { self.schedule(tx) } else { Ok(()) }
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

/// The remark of a failed check: its name and summary, and the output of an exec check.
pub(super) fn remark(step: &str, outcome: &Outcome) -> String {
    let data = outcome.data.as_ref();
    let output = ["stdout", "stderr"].map(|k| data.and_then(|d| d[k].as_str()).unwrap_or_default().trim());
    let mut text = format!("{step}: {}", outcome.summary);
    for part in output.into_iter().filter(|p| !p.is_empty()) {
        text = format!("{text}\n{part}");
    }
    text
}
