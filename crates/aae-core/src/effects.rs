//! Effects run after commit through a queue keyed by subject: one at a time per key, retried with backoff
//! 500 ms → 5 min (spec §5.1).

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::HashSet;

const BACKOFF_FIRST_MS: i64 = 500;
const BACKOFF_CAP_MS: i64 = 5 * 60 * 1000;

pub(crate) struct Due {
    pub(crate) id: i64,
    pub(crate) key: String,
    pub(crate) payload: Value,
}

/// The oldest pending effect of every key, split into those due at `now_ms` (and not in flight) and the earliest
/// future wake-up among the rest.
pub(crate) fn heads(
    conn: &Connection,
    in_flight: &HashSet<String>,
    now_ms: i64,
) -> rusqlite::Result<(Vec<Due>, Option<i64>)> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, key, payload, next_at_ms FROM effects e
         WHERE id = (SELECT MIN(id) FROM effects WHERE key = e.key) ORDER BY id",
    )?;
    let mut due = Vec::new();
    let mut wake: Option<i64> = None;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Value>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (id, key, payload, next_at) = row?;
        if in_flight.contains(&key) {
            continue;
        }
        if next_at <= now_ms {
            due.push(Due { id, key, payload });
        } else {
            wake = Some(wake.map_or(next_at, |w| w.min(next_at)));
        }
    }
    Ok((due, wake))
}

/// Records the result of one execution: success removes the effect, failure schedules the next try.
pub(crate) fn finish(conn: &Connection, id: i64, ok: bool, now_ms: i64) -> rusqlite::Result<()> {
    if ok {
        conn.execute("DELETE FROM effects WHERE id = ?1", [id])?;
        return Ok(());
    }
    let attempts: Option<i64> = conn
        .query_row("SELECT attempts FROM effects WHERE id = ?1", [id], |r| r.get(0))
        .optional()?;
    if let Some(attempts) = attempts {
        conn.execute(
            "UPDATE effects SET attempts = ?2, next_at_ms = ?3 WHERE id = ?1",
            params![id, attempts + 1, now_ms + backoff_ms(attempts)],
        )?;
    }
    Ok(())
}

/// Delay before the retry that follows `failures` earlier failures: 500 ms, 1 s, 2 s, … capped at 5 min.
pub(crate) fn backoff_ms(failures: i64) -> i64 {
    let shift = u32::try_from(failures.clamp(0, 20)).unwrap_or(20);
    (BACKOFF_FIRST_MS << shift).min(BACKOFF_CAP_MS)
}
