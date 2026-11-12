//! Deciding events (docs/design/contracts.md 3.2–3.4) as the run machine waits on them: their deadline, the
//! decisions hooks may give and the run machine applies, and the retry of an interrupted run.

use super::{Decision, GateSpec, MAX_ATTEMPTS, RunHandler, answer_from, gates::valid_key, rejected, row::Row};
use crate::{
    config::{Layer, Sourced},
    journal::{parse_rfc3339_ms, rfc3339_ms},
    writer::{Error, Tx},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

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
            ("interrupted", Some(Decision::Fail { reason, gate })) => {
                self.policy_gate(tx, gate.as_ref())?;
                self.terminal(tx, row, "failed", Some("infrastructure"), &reason)
            }
            ("interrupted", Some(Decision::Resume { after, gate })) => {
                self.policy_gate(tx, gate.as_ref())?;
                self.retry(tx, row, "resume", Some(after))
            }
            ("interrupted", Some(Decision::NewSession { after, gate })) => {
                self.policy_gate(tx, gate.as_ref())?;
                self.retry(tx, row, "new_session", Some(after))
            }
            ("interrupted", Some(Decision::Park { gate, then })) => {
                self.policy_gate(tx, Some(&gate))?;
                self.retry(tx, row, &then, None)
            }
            ("running", None) => self.terminal(tx, row, "failed", Some("hollow_done"), "no_decision"),
            ("running", Some(Decision::Fail { reason, .. })) => {
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

    /// The gate a decision on `run.attempt_failed` carries, closed by the policy.
    fn policy_gate(&mut self, tx: &mut Tx<'_>, gate: Option<&GateSpec>) -> Result<(), Error> {
        gate.map_or(Ok(()), |gate| self.close_gate(tx, gate, "policy"))
    }

    /// Schedules the next attempt of an `interrupted` run after `after` (`None`: parks it on its closed gate),
    /// within the attempts ceiling.
    fn retry(
        &mut self,
        tx: &mut Tx<'_>,
        row: Row,
        kind: &str,
        after: Option<std::time::Duration>,
    ) -> Result<(), Error> {
        if row.attempt >= MAX_ATTEMPTS {
            return self.terminal(tx, row, "failed", Some("infrastructure"), "attempts_ceiling");
        }
        let Some(after) = after else {
            return self.start_attempt(tx, row, kind);
        };
        tx.sql()
            .execute("UPDATE runs SET next = ?2 WHERE id = ?1", params![row.id, kind])?;
        let after = i64::try_from(after.as_millis()).unwrap_or(i64::MAX);
        tx.set_deadline(&row.deadline("state"), tx.now_ms().saturating_add(after))
    }
}

/// A hook's JSON decision on `kind`; `None` when it is not allowed for the event (contracts 3.2).
pub(super) fn parse(kind: &str, v: &Value, plugin: &str, event: &Value) -> Option<Decision> {
    let text = |key: &str| v[key].as_str().map(str::to_owned);
    let after = || match &v["after"] {
        Value::Null => Some(Duration::ZERO),
        after => crate::config::duration(after.as_str()?).ok(),
    };
    Some(match (kind, v["decision"].as_str()?) {
        ("run.preparing", "proceed") => {
            let workdir = match &v["workdir"] {
                Value::Null => None,
                dir => Some(PathBuf::from(dir.as_str()?)),
            };
            if workdir.as_ref().is_some_and(|d| !d.is_absolute()) {
                return None;
            }
            let mut env = BTreeMap::new();
            for (name, value) in v["env"].as_object().into_iter().flatten() {
                let value = match value {
                    Value::Null => None,
                    value => Some(Sourced {
                        value: value.as_str()?.to_owned(),
                        from: Layer::Plugin(plugin.to_owned()),
                    }),
                };
                env.insert(name.clone(), value);
            }
            Decision::Proceed {
                workdir,
                env,
                prompt_append: match &v["prompt_append"] {
                    Value::Null => None,
                    append => Some(append.as_str()?.to_owned()),
                },
            }
        }
        ("run.preparing", "deny") => Decision::Deny {
            reason: text("reason")?,
        },
        ("run.requires_action", "answer") if event["data"]["request"]["kind"] != "question" => return None,
        ("run.requires_action", _) => Decision::Answer(answer_from(v)?),
        ("run.attempt_failed", "resume") => Decision::Resume {
            after: after()?,
            gate: gate_spec(&v["gate"])?,
        },
        ("run.attempt_failed", "new_session") => Decision::NewSession {
            after: after()?,
            gate: gate_spec(&v["gate"])?,
        },
        ("run.attempt_failed", "park") => Decision::Park {
            gate: gate_spec(&v["gate"])??,
            then: match v["then"].as_str()? {
                then @ ("resume" | "new_session") => then.to_owned(),
                _ => return None,
            },
        },
        ("run.attempt_failed", "fail") => Decision::Fail {
            reason: text("reason")?,
            gate: gate_spec(&v["gate"])?,
        },
        ("run.turn_ended_without_outcome", "fail") => Decision::Fail {
            reason: text("reason")?,
            gate: None,
        },
        ("run.turn_ended_without_outcome", "continue") => Decision::Continue {
            message: text("message")?,
        },
        ("run.finishing", "accept") => Decision::Accept,
        ("run.finishing", "veto") => Decision::Veto {
            reason: text("reason")?,
        },
        // An exec step has no session to take a remark (contracts 6.2).
        ("run.finishing", "remark") if event["data"]["run"]["step_kind"] == "exec" => return None,
        ("run.finishing", "remark") => Decision::Remark {
            message: text("message")?,
        },
        _ => return None,
    })
}

/// The `gate` of a decision: `Some(None)` when absent, `None` when malformed (contracts 3.2).
fn gate_spec(v: &Value) -> Option<Option<GateSpec>> {
    if v.is_null() {
        return Some(None);
    }
    let key = v["key"].as_str().filter(|k| valid_key(k))?.to_owned();
    let until_ms = match &v["until"] {
        Value::Null => None,
        until => Some(parse_rfc3339_ms(until.as_str()?)?),
    };
    let reason = v["reason"].as_str()?.to_owned();
    Some(Some(GateSpec { key, until_ms, reason }))
}
