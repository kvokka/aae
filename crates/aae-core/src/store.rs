use rusqlite::Connection;
use std::path::Path;

const SCHEMA_VERSION: i32 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE outbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms INTEGER NOT NULL,
    body TEXT NOT NULL
);
CREATE TABLE idempotency (key TEXT PRIMARY KEY, reply TEXT NOT NULL);
CREATE TABLE deadlines (id TEXT PRIMARY KEY, at_ms INTEGER NOT NULL);
CREATE INDEX deadlines_at ON deadlines (at_ms);
CREATE TABLE effects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL,
    dedupe TEXT NOT NULL,
    payload TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_at_ms INTEGER NOT NULL,
    UNIQUE (key, dedupe)
);
-- Durable outbox consumers and the last seq each acknowledged (docs/design/contracts.md 3.5).
CREATE TABLE consumers (name TEXT PRIMARY KEY, seq INTEGER NOT NULL);
INSERT INTO consumers (name, seq) VALUES ('journal', 0);
";

/// Opens the state database in WAL mode and brings its schema to the current version.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL in WAL mode survives a process crash (kill -9, criterion 8); only power loss can drop a commit.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < SCHEMA_VERSION {
        conn.execute_batch(&format!(
            "BEGIN; {SCHEMA_V1} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
        ))?;
    }
    Ok(conn)
}

/// Records that the durable consumer `name` has handled every event up to `seq`, and drops the outbox rows that
/// all consumers have handled (docs/design/contracts.md 3.5).
pub(crate) fn ack(conn: &mut Connection, name: &str, seq: i64) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE consumers SET seq = MAX(seq, ?2) WHERE name = ?1",
        rusqlite::params![name, seq],
    )?;
    tx.execute("DELETE FROM outbox WHERE seq <= (SELECT MIN(seq) FROM consumers)", [])?;
    tx.commit()
}
