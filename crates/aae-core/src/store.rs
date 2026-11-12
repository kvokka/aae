use rusqlite::Connection;
use std::path::Path;

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

/// Runs and their attempts (docs/design/contracts.md 6). `waiting` is the `seq` of the deciding event whose
/// decision the run waits for; `next` is the attempt kind an `interrupted` run starts when its `after` passes.
const SCHEMA_V2: &str = "
CREATE TABLE runs (
    n INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    project TEXT NOT NULL,
    rig TEXT NOT NULL,
    formula TEXT NOT NULL,
    agent TEXT NOT NULL,
    inputs TEXT NOT NULL,
    correlation TEXT NOT NULL,
    depth INTEGER NOT NULL,
    parent_run TEXT,
    workdir TEXT NOT NULL,
    state TEXT NOT NULL,
    class TEXT,
    reason TEXT,
    attempt INTEGER NOT NULL DEFAULT 0,
    prompt TEXT,
    timeout_ms INTEGER,
    config TEXT,
    waiting INTEGER,
    next TEXT,
    created_ms INTEGER NOT NULL,
    started_ms INTEGER
);
CREATE INDEX runs_state ON runs (state);
CREATE TABLE attempts (
    run TEXT NOT NULL,
    n INTEGER NOT NULL,
    kind TEXT NOT NULL,
    harness TEXT NOT NULL,
    instance TEXT,
    handle TEXT,
    session TEXT,
    request TEXT,
    turn_ended INTEGER NOT NULL DEFAULT 0,
    -- What was left of the step timeout when the attempt began to wait for an answer.
    paused_ms INTEGER,
    started_ms INTEGER NOT NULL,
    ended_ms INTEGER,
    failure TEXT,
    PRIMARY KEY (run, n)
);
CREATE INDEX attempts_instance ON attempts (instance);
";

/// The finisher (docs/design/contracts.md 6.3): the attempt token by its SHA-256, the declared outcome and the
/// number of prompts the attempt sent (remarks = prompts - 1); why a queued run waits; the run whose session a run
/// continues.
const SCHEMA_V3: &str = "
ALTER TABLE attempts ADD COLUMN token_sha256 TEXT;
ALTER TABLE attempts ADD COLUMN outcome TEXT;
ALTER TABLE attempts ADD COLUMN prompts INTEGER NOT NULL DEFAULT 1;
CREATE INDEX attempts_token ON attempts (token_sha256);
ALTER TABLE runs ADD COLUMN waiting_for TEXT;
ALTER TABLE runs ADD COLUMN continue_from TEXT;
";

/// Issue gates (docs/design/contracts.md 7): one closure per `(key, closed_by)`; the account and model gate keys a
/// run's attempts reported.
const SCHEMA_V4: &str = "
CREATE TABLE gates (
    key TEXT NOT NULL,
    closed_by TEXT NOT NULL,
    until_ms INTEGER,
    reason TEXT NOT NULL,
    closed_ms INTEGER NOT NULL,
    PRIMARY KEY (key, closed_by)
);
ALTER TABLE runs ADD COLUMN account TEXT;
ALTER TABLE runs ADD COLUMN model TEXT;
";

const SCHEMA_V5: &str = "
ALTER TABLE idempotency ADD COLUMN at_ms INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idempotency_at ON idempotency (at_ms);
";

/// The instance registry (docs/design/contracts.md 8): a row is written before its process starts or stops
/// (invariant 9); `handle` is the `InstanceHandle` once the process runs.
const SCHEMA_V6: &str = "
CREATE TABLE instances (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    harness TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    process TEXT NOT NULL,
    handle TEXT,
    state TEXT NOT NULL,
    sessions INTEGER NOT NULL,
    capacity INTEGER NOT NULL,
    idle_ms INTEGER NOT NULL,
    idle_since_ms INTEGER
);
CREATE INDEX instances_fingerprint ON instances (fingerprint, state);
";

/// The trigger (`<pack>.<name>`) that started a run, for the overlap check of spec §9.5.
const SCHEMA_V7: &str = "
ALTER TABLE runs ADD COLUMN trigger TEXT;
CREATE INDEX runs_trigger ON runs (trigger);
";

/// Bead ownership (spec §10.3, invariants 2–4): the owner run of a bead (the truth the tracker's assignee mirrors),
/// the generation of its claims and the failed runs counted toward its gate; the last ready set of each project's
/// tracker, in the tracker's order; the bead of a run and its data as the claim returned it.
const SCHEMA_V8: &str = "
CREATE TABLE beads (
    project TEXT NOT NULL,
    id TEXT NOT NULL,
    owner TEXT,
    generation INTEGER NOT NULL DEFAULT 0,
    failures INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (project, id)
);
CREATE TABLE ready_beads (
    project TEXT NOT NULL,
    pos INTEGER NOT NULL,
    id TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (project, pos)
);
ALTER TABLE runs ADD COLUMN bead TEXT;
ALTER TABLE runs ADD COLUMN bead_data TEXT;
";

/// The last tracker write each project's tracker events were read to (`Change::seq`).
const SCHEMA_V9: &str = "CREATE TABLE tracker_cursors (project TEXT PRIMARY KEY, seq INTEGER NOT NULL);";

/// The delivery record of every prompt of an attempt (docs/design/contracts.md 6.4): `n` counts from 1.
const SCHEMA_V10: &str = "
CREATE TABLE prompts (
    run TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    n INTEGER NOT NULL,
    key TEXT NOT NULL,
    text TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('intent', 'sent', 'acked', 'settled')),
    PRIMARY KEY (run, attempt, n)
);
";

/// Opens the state database in WAL mode and brings its schema to the current version.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL in WAL mode survives a process crash (kill -9, criterion 8); only power loss can drop a commit.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (to, sql) in [
        (1, SCHEMA_V1),
        (2, SCHEMA_V2),
        (3, SCHEMA_V3),
        (4, SCHEMA_V4),
        (5, SCHEMA_V5),
        (6, SCHEMA_V6),
        (7, SCHEMA_V7),
        (8, SCHEMA_V8),
        (9, SCHEMA_V9),
        (10, SCHEMA_V10),
    ]
    .into_iter()
    .filter(|(to, _)| *to > version)
    {
        conn.execute_batch(&format!("BEGIN; {sql} PRAGMA user_version = {to}; COMMIT;"))?;
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
