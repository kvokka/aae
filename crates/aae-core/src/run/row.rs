//! A row of `runs` and the attempts of a run as the events of docs/design/contracts.md 1.3 show them.

use super::rejected;
use crate::{
    journal::rfc3339_ms,
    writer::{Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

/// One row of `runs`.
pub(super) struct Row {
    pub(super) id: String,
    pub(super) project: String,
    pub(super) rig: String,
    pub(super) formula: String,
    pub(super) agent: String,
    pub(super) inputs: String,
    pub(super) correlation: String,
    pub(super) depth: i64,
    pub(super) parent_run: Option<String>,
    pub(super) workdir: String,
    pub(super) state: String,
    pub(super) class: Option<String>,
    pub(super) attempt: u32,
    pub(super) prompt: Option<String>,
    pub(super) timeout_ms: Option<i64>,
    pub(super) config: Option<String>,
    pub(super) next: Option<String>,
    pub(super) created_ms: i64,
    pub(super) started_ms: Option<i64>,
}

const COLUMNS: &str = "id, project, rig, formula, agent, inputs, correlation, depth, parent_run, workdir, state, \
    class, attempt, prompt, timeout_ms, config, next, created_ms, started_ms";

impl Row {
    pub(super) fn load(tx: &Tx<'_>, run: &str) -> Result<Self, Error> {
        let sql = format!("SELECT {COLUMNS} FROM runs WHERE id = ?1");
        tx.sql()
            .query_row(&sql, [run], |r| {
                Ok(Self {
                    id: r.get(0)?,
                    project: r.get(1)?,
                    rig: r.get(2)?,
                    formula: r.get(3)?,
                    agent: r.get(4)?,
                    inputs: r.get(5)?,
                    correlation: r.get(6)?,
                    depth: r.get(7)?,
                    parent_run: r.get(8)?,
                    workdir: r.get(9)?,
                    state: r.get(10)?,
                    class: r.get(11)?,
                    attempt: r.get(12)?,
                    prompt: r.get(13)?,
                    timeout_ms: r.get(14)?,
                    config: r.get(15)?,
                    next: r.get(16)?,
                    created_ms: r.get(17)?,
                    started_ms: r.get(18)?,
                })
            })
            .optional()?
            .ok_or_else(|| rejected(format!("no run `{run}`")))
    }

    pub(super) fn live(&self) -> bool {
        !matches!(self.state.as_str(), "passed" | "failed" | "cancelled")
    }

    /// `RunView` of contracts 1.3; the generation is the attempt number until bead ownership (WP-19) needs more.
    /// A run of an exec step: it has no agent.
    pub(super) fn exec(&self) -> bool {
        self.agent.is_empty()
    }

    pub(super) fn view(&self) -> Value {
        let (agent, step_kind) = if self.exec() {
            (None, "exec")
        } else {
            (Some(&self.agent), "prompt")
        };
        json!({
            "id": self.id, "formula": self.formula, "agent": agent, "step_kind": step_kind,
            "state": self.state, "class": self.class, "attempt": self.attempt, "generation": self.attempt,
            "bead": null, "workdir": self.workdir, "depth": self.depth, "parent_run": self.parent_run,
            "created_at": rfc3339_ms(self.created_ms), "started_at": self.started_ms.map(rfc3339_ms),
        })
    }

    pub(super) fn emit(&self, tx: &mut Tx<'_>, kind: &str, data: Value) -> Result<i64, Error> {
        tx.emit(&Event {
            kind: kind.to_owned(),
            project: Some(self.project.clone()),
            rig: Some(self.rig.clone()),
            run: Some(self.id.clone()),
            correlation: Some(self.correlation.clone()),
            data,
        })
    }

    pub(super) fn set_state(&mut self, tx: &mut Tx<'_>, to: &str, reason: Option<&str>) -> Result<(), Error> {
        tx.sql()
            .execute("UPDATE runs SET state = ?2 WHERE id = ?1", params![self.id, to])?;
        let data = json!({ "from": self.state, "to": to, "attempt": self.attempt, "generation": self.attempt,
            "reason": reason });
        self.state = to.to_owned();
        self.emit(tx, "run.state_changed", data).map(drop)
    }

    pub(super) fn deadline(&self, what: &str) -> String {
        format!("{}:{what}", self.id)
    }
}

/// `Attempts` of contracts 1.3: the last 20 attempts of a run, newest last.
pub(super) fn attempts(tx: &Tx<'_>, run: &str) -> Result<Vec<Value>, Error> {
    let mut stmt = tx.sql().prepare(
        "SELECT n, kind, started_ms, ended_ms, json_extract(failure, '$.kind') FROM \
         (SELECT * FROM attempts WHERE run = ?1 ORDER BY n DESC LIMIT 20) ORDER BY n",
    )?;
    let rows = stmt.query_map([run], |r| {
        Ok(json!({
            "n": r.get::<_, i64>(0)?, "generation": r.get::<_, i64>(0)?, "kind": r.get::<_, String>(1)?,
            "started_at": rfc3339_ms(r.get(2)?), "ended_at": r.get::<_, Option<i64>>(3)?.map(rfc3339_ms),
            "failure_kind": r.get::<_, Option<String>>(4)?,
        }))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
