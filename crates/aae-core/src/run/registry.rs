//! The instance registry (spec §7.3, docs/design/contracts.md 8): instances by fingerprint, shared up to their
//! capacity, stopped once idle. A start or a stop is written before it happens (invariant 9).

use super::RunHandler;
use crate::{
    driver::{Capabilities, Failure, FailureKind, InstanceHandle, Source, Split},
    writer::{Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

impl RunHandler {
    /// An instance for one more session of `split`: a live one of the fingerprint with room, else a new one.
    /// Returns its id and its handle (null while it starts).
    pub(super) fn acquire(
        tx: &mut Tx<'_>,
        harness: &str,
        split: &Split,
        caps: &Capabilities,
    ) -> Result<Option<(String, Value)>, Error> {
        let found: Option<(String, Option<String>)> = tx
            .sql()
            .query_row(
                "SELECT id, handle FROM instances WHERE fingerprint = ?1 AND kind = 'instance' \
                 AND state IN ('starting', 'ready') AND sessions < capacity ORDER BY rowid LIMIT 1",
                [&split.fingerprint.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, handle)) = found {
            tx.sql().execute(
                "UPDATE instances SET sessions = sessions + 1, idle_since_ms = NULL WHERE id = ?1",
                [&id],
            )?;
            tx.clear_deadline(&format!("{id}:idle"))?;
            let handle = handle.and_then(|h| serde_json::from_str(&h).ok()).unwrap_or_default();
            return Ok(Some((id, handle)));
        }
        let Some(token) = super::new_token() else {
            return Ok(None);
        };
        let id = format!("i-{}", &token[..16]);
        let idle_ms = i64::try_from(caps.instance_idle.as_millis()).unwrap_or(i64::MAX);
        tx.sql().execute(
            "INSERT INTO instances (id, kind, harness, fingerprint, process, state, sessions, capacity, idle_ms) \
             VALUES (?1, 'instance', ?2, ?3, ?4, 'starting', 1, ?5, ?6)",
            params![
                id,
                harness,
                split.fingerprint.0,
                split.process.0.to_string(),
                i64::try_from(caps.sessions_per_instance.max(1)).unwrap_or(i64::MAX),
                idle_ms
            ],
        )?;
        Self::instance_event(tx, &id, "instance.starting", json!({ "reason": null }))?;
        Ok(Some((id, Value::Null)))
    }

    /// One session fewer on `id`; with none left the instance stops at once or after its idle time.
    pub(super) fn release(tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        tx.sql().execute(
            "UPDATE instances SET sessions = sessions - 1 WHERE id = ?1 AND sessions > 0",
            [id],
        )?;
        let row: Option<(String, i64, i64)> = tx
            .sql()
            .query_row(
                "SELECT state, sessions, idle_ms FROM instances WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match row {
            Some((state, 0, _)) if state == "starting" => {
                // Stopped once it is up: its handle is not known yet.
                tx.sql()
                    .execute("UPDATE instances SET state = 'stopping' WHERE id = ?1", [id])?;
                Ok(())
            }
            Some((state, 0, 0)) if state == "ready" => Self::stop_instance(tx, id),
            Some((state, 0, idle_ms)) if state == "ready" => {
                let now = tx.now_ms();
                tx.sql().execute(
                    "UPDATE instances SET idle_since_ms = ?2 WHERE id = ?1",
                    params![id, now],
                )?;
                tx.set_deadline(&format!("{id}:idle"), now.saturating_add(idle_ms))
            }
            _ => Ok(()),
        }
    }

    /// The idle time of `id` passed: it stops if it still has no session.
    pub(super) fn instance_idle(tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        let idle: Option<i64> = tx
            .sql()
            .query_row(
                "SELECT 1 FROM instances WHERE id = ?1 AND state = 'ready' AND sessions = 0",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        match idle {
            Some(_) => Self::stop_instance(tx, id),
            None => Ok(()),
        }
    }

    /// The instance `id` runs as `handle`.
    pub(super) fn instance_ready(
        tx: &mut Tx<'_>,
        id: &str,
        harness: &str,
        handle: &InstanceHandle,
    ) -> Result<(), Error> {
        let state: Option<String> = tx
            .sql()
            .query_row("SELECT state FROM instances WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        let handle_json = json!(handle);
        tx.sql().execute(
            "UPDATE instances SET handle = ?2 WHERE id = ?1",
            params![id, handle_json.to_string()],
        )?;
        match state.as_deref() {
            Some("starting") => {
                tx.sql()
                    .execute("UPDATE instances SET state = 'ready' WHERE id = ?1", [id])?;
                Self::instance_event(tx, id, "instance.ready", json!({ "reason": null }))
            }
            Some("stopping") => Self::stop_instance(tx, id),
            // Unknown: nothing may run for it.
            _ => {
                let payload =
                    json!({ "op": "stop_instance", "harness": harness, "instance": id, "handle": handle_json });
                tx.effect(id, "stop", &payload)
            }
        }
    }

    /// The instance `id` could not start: its record goes, the attempts on it fail through their launches.
    pub(super) fn instance_failed(tx: &mut Tx<'_>, id: &str, failure: &Failure) -> Result<(), Error> {
        let data = json!({ "failure": super::failure_json(failure) });
        Self::instance_event(tx, id, "instance.failed", data)?;
        tx.sql().execute("DELETE FROM instances WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The process of `id` ended: a stop the registry asked for, or a crash.
    pub(super) fn instance_exited(tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        let state: Option<String> = tx
            .sql()
            .query_row("SELECT state FROM instances WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        match state.as_deref() {
            Some("stopping") | None => Ok(()),
            Some(_) => Self::instance_failed(tx, id, &Failure::new(FailureKind::HarnessCrash, Source::Adapter)),
        }
    }

    /// The stop of `id` is done.
    pub(super) fn instance_stopped(tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        Self::instance_event(tx, id, "instance.stopped", json!({ "reason": "idle" }))?;
        tx.sql().execute("DELETE FROM instances WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The handles of instances still stopping that served `run` or the run it continues: a new process for them
    /// starts only after these are dead (invariant 6).
    pub(super) fn stopping_for(tx: &Tx<'_>, run: &str, from: Option<&str>) -> Result<Vec<Value>, Error> {
        let mut stmt = tx.sql().prepare(
            "SELECT DISTINCT i.handle FROM attempts a JOIN instances i ON i.id = a.instance \
             WHERE a.run IN (?1, ?2) AND i.state = 'stopping' AND i.handle IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![run, from.unwrap_or(run)], |r| r.get::<_, String>(0))?;
        let handles: Vec<String> = rows.collect::<Result<_, _>>()?;
        Ok(handles.iter().filter_map(|h| serde_json::from_str(h).ok()).collect())
    }

    fn stop_instance(tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        let (harness, handle): (String, Option<String>) =
            tx.sql()
                .query_row("SELECT harness, handle FROM instances WHERE id = ?1", [id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        tx.sql().execute(
            "UPDATE instances SET state = 'stopping', idle_since_ms = NULL WHERE id = ?1",
            [id],
        )?;
        tx.clear_deadline(&format!("{id}:idle"))?;
        let handle: Value = handle.and_then(|h| serde_json::from_str(&h).ok()).unwrap_or_default();
        let payload = json!({ "op": "stop_instance", "harness": harness, "instance": id, "handle": handle });
        tx.effect(id, "stop", &payload)
    }

    /// An `instance.*` event with the instance view of contracts 1.3 merged into `data`.
    fn instance_event(tx: &mut Tx<'_>, id: &str, kind: &str, mut data: Value) -> Result<(), Error> {
        let (harness, fingerprint, handle): (String, String, Option<String>) = tx.sql().query_row(
            "SELECT harness, fingerprint, handle FROM instances WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let handle: Value = handle.and_then(|h| serde_json::from_str(&h).ok()).unwrap_or_default();
        data["instance"] = json!({ "id": id, "harness": harness, "fingerprint": fingerprint,
            "pid": handle["pid"], "pgid": handle["pgid"] });
        tx.emit(&Event {
            kind: kind.to_owned(),
            project: None,
            rig: None,
            run: None,
            correlation: None,
            data,
        })
        .map(drop)
    }
}
