//! Daemon start (spec §5.6, docs/design/contracts.md 6.4): what the previous daemon left running is killed; the
//! attempts it was running fail with `daemon_crash` for the policy to decide, and a run that waited for a decision
//! on a deciding event goes on waiting for it, its hooks run again with their full time. Effects and the outbox are
//! in the state, so they carry on by themselves; schedules are armed after this.

use super::{RunHandler, decide::DECIDE_SLACK_MS, row::Row};
use crate::{
    driver::{Failure, FailureKind, Source},
    writer::{Error, Event, Tx},
};
use serde_json::json;
use std::collections::BTreeSet;

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
        // A hook still queued may be running in its own process group, left by the dead daemon: it is killed by its
        // marker now, before it runs again — no effect of this daemon has started yet.
        let hooks: BTreeSet<String> = {
            let mut stmt = tx.sql().prepare(
                "SELECT json_extract(payload, '$.marker') FROM effects
                 WHERE key LIKE 'hooks:%' AND json_extract(payload, '$.op') = 'hooks'",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
            rows.filter_map(Result::transpose).collect::<Result<_, _>>()?
        };
        if !hooks.is_empty() {
            crate::orphans::sweep_all(&hooks);
        }
        // A run whose turn ended waits for the decision on it (`waiting`) and keeps waiting; the others had an attempt
        // going, which is lost.
        let caught: Vec<(String, bool)> = {
            let mut stmt = tx.sql().prepare(
                "SELECT id, state IN ('running', 'finishing') AND waiting IS NOT NULL FROM runs \
                 WHERE state IN ('starting', 'running', 'requires_action', 'finishing') ORDER BY n",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        self.rearm(tx)?;
        let mut interrupted = 0;
        for (id, waits) in &caught {
            // What the attempt still had queued belongs to a process that is gone.
            tx.sql().execute(
                "DELETE FROM effects WHERE key = ?1 AND json_extract(payload, '$.op') IN ('launch', 'prompt', 'respond')",
                [id],
            )?;
            // The command of an exec step lives on in its own process group: it is killed by its marker, under the
            // key of the exec effects so that no later attempt runs the command before. One that ended with the
            // dead daemon before its effect was done does not run again either.
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
            if *waits {
                // The session that took the turn is gone with its instance: a decision that needs it fails the
                // attempt (`next_prompt`), the others apply as they would have. No clock of the turn runs.
                tx.sql().execute(
                    "UPDATE attempts SET handle = NULL WHERE run = ?1 AND n = ?2",
                    rusqlite::params![id, row.attempt],
                )?;
                for what in ["state", "silence"] {
                    tx.clear_deadline(&row.deadline(what))?;
                }
                continue;
            }
            // The deciding hooks still queued (`run.requires_action`) judge the lost attempt; their decision would be
            // refused, and the attempt's failure queues its own under the same key.
            tx.sql()
                .execute("DELETE FROM effects WHERE key = ?1", [format!("hooks:{id}")])?;
            self.attempt_failed(tx, row, Failure::new(FailureKind::DaemonCrash, Source::Core))?;
            interrupted += 1;
        }
        tx.emit(&Event {
            kind: "daemon.started".to_owned(),
            project: None,
            rig: None,
            run: None,
            correlation: None,
            data: json!({ "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(),
                "interrupted_runs": interrupted }),
        })
        .map(drop)
    }

    /// A deadline that waits for an effect's answer counts from now: the effect runs again from its start, and the
    /// time the daemon was down is not the effect's (the deadlines of real time — gates, schedules, `after` — stay).
    fn rearm(&self, tx: &mut Tx<'_>) -> Result<(), Error> {
        // The deciding hooks of a run run in order under its key, from the first one's retry time (after a panic of
        // the runner): the decision waits for all of them (contracts 3.3).
        let waiting: Vec<(String, i64)> = {
            let mut stmt = tx.sql().prepare(
                "SELECT r.id, MAX(?1, MIN(e.next_at_ms)) + SUM(json_extract(h.value, '$.timeout_ms')) FROM runs r \
                 JOIN effects e ON e.key = 'hooks:' || r.id, json_each(e.payload, '$.hooks') h \
                 WHERE r.waiting IS NOT NULL AND r.state != 'requires_action' GROUP BY r.id",
            )?;
            let rows = stmt.query_map([tx.now_ms()], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (run, at) in waiting {
            tx.set_deadline(&format!("{run}:decide"), at + DECIDE_SLACK_MS)?;
        }
        // A run in `preparing` with a state deadline waits for the claim of its bead in the tracker.
        tx.sql().execute(
            "UPDATE deadlines SET at_ms = ?1 WHERE id IN (SELECT id || ':state' FROM runs WHERE state = 'preparing')",
            [tx.now_ms() + self.config.watchdog.start_timeout_ms],
        )?;
        Ok(())
    }
}
