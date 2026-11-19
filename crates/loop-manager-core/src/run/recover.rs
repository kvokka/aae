//! Daemon start (docs/design/invariants.md, docs/design/contracts.md 6.4): what the previous daemon left running is
//! killed; the attempts it was running fail with `daemon_crash` for the policy to decide, a run that waited for a
//! decision on a deciding event goes on waiting for it, its hooks run again with their full time, and a run that waited
//! only for its background to stop after the outcome goes to `finishing` with that outcome. Effects and the outbox are
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
        // no reconnect in the first release (docs/design/invariants.md). A new process for one of its runs waits for
        // its death.
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
        // The command of an exec step lives on in its own process group: it is killed by its marker, under the key of
        // the exec effects so that no later attempt runs the command before. One that ended with the dead daemon
        // before its effect was done does not run again either, whatever the state of its run since (contracts 6.4).
        let execs: std::collections::BTreeSet<(String, String)> = {
            let mut stmt = tx.sql().prepare(
                "SELECT key, COALESCE(json_extract(payload, '$.marker'), json_extract(payload, '$.instance')) \
                 FROM effects WHERE key LIKE 'exec:%'",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
            rows.filter_map(|r| r.map(|(k, m)| m.map(|m| (k, m))).transpose())
                .collect::<Result<_, _>>()?
        };
        tx.sql().execute("DELETE FROM effects WHERE key LIKE 'exec:%'", [])?;
        for (key, marker) in execs {
            tx.effect(
                &key,
                &format!("sweep:{marker}"),
                &json!({ "op": "sweep", "instance": marker }),
            )?;
        }
        // A run whose turn ended waits for the decision on it (`waiting`) and keeps waiting; one whose turn ended with
        // the outcome declared was only waiting for its background to stop, which the kill above did
        // (docs/design/contracts.md 6); the others had an attempt going, which is lost (a drain step's too: its next
        // attempt gives no bead its entry has given). A run waiting in an await step has no attempt to lose: it waits on.
        let caught: Vec<(String, bool, bool)> = {
            let mut stmt = tx.sql().prepare(
                "SELECT r.id, r.state IN ('running', 'finishing') AND r.waiting IS NOT NULL, \
                 r.state = 'running' AND r.waiting IS NULL AND EXISTS (SELECT 1 FROM attempts a \
                 WHERE a.run = r.id AND a.n = r.attempt AND a.outcome IS NOT NULL AND a.turn_ended != 0) FROM runs r \
                 WHERE r.state IN ('starting', 'running', 'finishing') \
                 AND NOT (r.step_kind = 'await' AND r.state = 'running') ORDER BY r.n",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        self.rearm(tx)?;
        let mut interrupted = 0;
        for (id, waits, settled) in &caught {
            // What the attempt still had queued belongs to a process that is gone.
            tx.sql().execute(
                "DELETE FROM effects WHERE key = ?1 AND json_extract(payload, '$.op') IN ('launch', \
                'prompt', 'stop_background')",
                [id],
            )?;
            let row = Row::load(tx, id)?;
            // `finishing` with no decision pending: the check script of the step was running (docs/design/config.md).
            let checking = !*waits && row.state == "finishing";
            if *waits || checking || *settled {
                // The session that took the turn is gone with its instance: a decision that needs it fails the
                // attempt (`next_prompt`), the others apply as they would have. No clock of the turn runs.
                tx.sql().execute(
                    "UPDATE attempts SET handle = NULL WHERE run = ?1 AND n = ?2",
                    rusqlite::params![id, row.attempt],
                )?;
                for what in ["state", "silence"] {
                    tx.clear_deadline(&row.deadline(what))?;
                }
                // The script runs again, after the sweep of the lost one: its execution counts once, when its word
                // is written.
                if checking {
                    self.check(tx, row)?;
                } else if *settled {
                    let attempt = row.attempt;
                    self.turn_settled(tx, row, attempt)?;
                }
                continue;
            }
            // The deciding hooks still queued judge the lost attempt; their decision would be refused, and the
            // attempt's failure queues its own under the same key.
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
    /// time the daemon was down is not the effect's
    /// (the deadlines of real time — gates, schedules, `after` — stay).
    fn rearm(&self, tx: &mut Tx<'_>) -> Result<(), Error> {
        // The deciding hooks of a run run in order under its key, from the first one's retry time (after a panic of
        // the runner): the decision waits for all of them (contracts 3.3).
        let waiting: Vec<(String, i64)> = {
            let mut stmt = tx.sql().prepare(&format!(
                "SELECT r.id, MAX(?1, MIN(e.next_at_ms)) + {} FROM runs r \
                 JOIN effects e ON e.key = 'hooks:' || r.id, json_each(e.payload, '$.hooks') h \
                 WHERE r.waiting IS NOT NULL GROUP BY r.id",
                super::decide::queued()
            ))?;
            let rows = stmt.query_map([tx.now_ms()], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (run, at) in waiting {
            tx.set_deadline(&format!("{run}:decide"), at + DECIDE_SLACK_MS)?;
        }
        // A run in `preparing` with a state deadline waits for the tracker: the claim of its bead, or its comments and
        // parents.
        tx.sql().execute(
            "UPDATE deadlines SET at_ms = ?1 WHERE id IN (SELECT id || ':state' FROM runs WHERE state = 'preparing')",
            [tx.now_ms() + self.config.watchdog.start_timeout_ms],
        )?;
        Ok(())
    }
}
