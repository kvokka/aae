//! Daemon start (spec §5.6, docs/design/contracts.md 6.4): what the previous daemon left running is killed, and the
//! attempts it was running or finishing fail with `daemon_crash` for the policy to decide. Deadlines, effects and the
//! outbox are in the state, so they carry on by themselves; schedules are armed after this.

use super::{RunHandler, row::Row};
use crate::{
    driver::{Failure, FailureKind, Source},
    writer::{Error, Event, Tx},
};
use serde_json::json;

impl RunHandler {
    pub(super) fn recover(&mut self, tx: &mut Tx<'_>) -> Result<(), Error> {
        // Every recorded instance is stopped by its pid and start time and swept by its marker (invariants 6, 7);
        // no reconnect in the first release (§5.6). A new process for one of its runs waits for its death.
        let instances: Vec<String> = {
            let mut stmt = tx.sql().prepare("SELECT id FROM instances ORDER BY rowid")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in &instances {
            Self::stop_instance(tx, id, "recovery")?;
        }
        let active: Vec<String> = {
            let mut stmt = tx.sql().prepare(
                "SELECT id FROM runs WHERE state IN ('starting', 'running', 'requires_action', 'finishing') ORDER BY n",
            )?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in &active {
            // What the lost attempt still had queued belongs to a process that is gone.
            tx.sql().execute(
                "DELETE FROM effects WHERE key = ?1 AND json_extract(payload, '$.op') IN ('launch', 'prompt', 'respond')",
                [id],
            )?;
            // The deciding hooks still queued (`run.finishing` among them) judge the lost attempt; their decision
            // would be refused, and the attempt's failure queues its own under the same key.
            tx.sql()
                .execute("DELETE FROM effects WHERE key = ?1", [format!("hooks:{id}")])?;
            // The command of an exec step lives on in its own process group: it is killed by its marker, under the
            // key of the exec effects so that no later attempt runs the command before.
            let exec = format!("exec:{id}");
            let markers: Vec<String> = {
                let mut stmt = tx
                    .sql()
                    .prepare("SELECT json_extract(payload, '$.marker') FROM effects WHERE key = ?1")?;
                let rows = stmt.query_map([&exec], |r| r.get::<_, Option<String>>(0))?;
                rows.filter_map(Result::transpose).collect::<Result<_, _>>()?
            };
            tx.sql().execute("DELETE FROM effects WHERE key = ?1", [&exec])?;
            for marker in markers {
                tx.effect(
                    &exec,
                    &format!("sweep:{marker}"),
                    &json!({ "op": "sweep", "instance": marker }),
                )?;
            }
            let row = Row::load(tx, id)?;
            self.attempt_failed(tx, row, Failure::new(FailureKind::DaemonCrash, Source::Core))?;
        }
        tx.emit(&Event {
            kind: "daemon.started".to_owned(),
            project: None,
            rig: None,
            run: None,
            correlation: None,
            data: json!({ "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(),
                "interrupted_runs": active.len() }),
        })
        .map(drop)
    }
}
