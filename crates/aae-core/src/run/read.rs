//! Reads of the run state for the frontends, on a read-only connection of their own (WAL lets it read beside the
//! writer without blocking it).

use super::{gates::closures, row, row::Row};
use crate::writer::Error;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use std::{path::Path, sync::Mutex};

pub struct Reader(Mutex<Connection>);

/// Which runs `Reader::runs` lists.
#[derive(Default)]
pub struct RunFilter {
    pub project: Option<String>,
    pub state: Option<String>,
    pub limit: Option<u32>,
}

impl Reader {
    /// Opens the state database `store::open` created.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Self(Mutex::new(conn)))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The `RunView` of contracts 1.3 with where the run is and its attempts; `None` if there is no such run.
    pub fn run(&self, id: &str) -> Result<Option<Value>, Error> {
        let conn = self.conn();
        let Some(row) = Row::read(&conn, id)? else {
            return Ok(None);
        };
        let waiting_for: Option<String> =
            conn.query_row("SELECT waiting_for FROM runs WHERE id = ?1", [id], |r| r.get(0))?;
        let mut view = row.view();
        view["project"] = json!(row.project);
        view["rig"] = json!(row.rig);
        view["correlation"] = json!(row.correlation);
        view["inputs"] = serde_json::from_str(&row.inputs).unwrap_or_default();
        view["waiting_for"] = json!(waiting_for);
        view["attempts"] = json!(row::attempts(&conn, id)?);
        Ok(Some(view))
    }

    /// The runs `filter` selects, newest first (at most 100 unless the filter says otherwise).
    pub fn runs(&self, filter: &RunFilter) -> Result<Vec<Value>, Error> {
        let ids: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT id FROM runs WHERE (?1 IS NULL OR project = ?1) AND (?2 IS NULL OR state = ?2) \
                 ORDER BY n DESC LIMIT ?3",
            )?;
            let rows = stmt.query_map(
                params![filter.project, filter.state, filter.limit.unwrap_or(100)],
                |r| r.get(0),
            )?;
            rows.collect::<Result<_, _>>()?
        };
        ids.iter().filter_map(|id| self.run(id).transpose()).collect()
    }

    /// Every trigger of `config` on every rig it runs on, with its next firing (contracts 1.3).
    pub fn triggers(&self, config: &crate::config::Config) -> Result<Vec<Value>, Error> {
        let conn = self.conn();
        super::triggers::list(config, |id| {
            let at = conn.query_row("SELECT at_ms FROM deadlines WHERE id = ?1", [id], |r| r.get(0));
            Ok(rusqlite::OptionalExtension::optional(at)?)
        })
    }

    /// `{outbox_depth, outbox_oldest_ms, active_runs}` for the daemon status (contracts 3.5): the outbox rows the
    /// journal has not acknowledged, the time of the oldest one, and the runs a drain waits for (one in a turn,
    /// a hook or a preparation).
    pub fn status(&self) -> Result<Value, Error> {
        let conn = self.conn();
        let (depth, oldest): (i64, Option<i64>) =
            conn.query_row("SELECT COUNT(*), MIN(ts_ms) FROM outbox", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let active: i64 = conn.query_row(
            "SELECT COUNT(*) FROM runs \
             WHERE state IN ('preparing', 'starting', 'running', 'requires_action', 'finishing')",
            [],
            |r| r.get(0),
        )?;
        Ok(json!({ "outbox_depth": depth, "outbox_oldest_ms": oldest, "active_runs": active }))
    }

    /// Every instance of the registry (contracts 8), oldest first.
    pub fn instances(&self) -> Result<Vec<Value>, Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, kind, harness, fingerprint, state, sessions, capacity, handle, idle_since_ms \
             FROM instances ORDER BY rowid",
        )?;
        let rows = stmt.query_map([], |r| {
            let handle: Option<String> = r.get(7)?;
            let handle: Value = handle.and_then(|h| serde_json::from_str(&h).ok()).unwrap_or_default();
            let idle_since: Option<i64> = r.get(8)?;
            Ok(json!({
                "id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "harness": r.get::<_, String>(2)?,
                "fingerprint": r.get::<_, String>(3)?, "state": r.get::<_, String>(4)?,
                "sessions": r.get::<_, i64>(5)?, "capacity": r.get::<_, i64>(6)?,
                "pid": handle["pid"], "pgid": handle["pgid"],
                "idle_since": idle_since.map(crate::journal::rfc3339_ms),
            }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every active gate closure at `now_ms` (contracts 7), oldest first.
    pub fn gates(&self, now_ms: i64) -> Result<Vec<Value>, Error> {
        closures(&self.conn(), now_ms, None)
    }
}
