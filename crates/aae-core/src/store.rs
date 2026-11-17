use rusqlite::Connection;
use std::{os::unix::fs::OpenOptionsExt, path::Path};

/// The number of [`SCHEMA`] in `user_version`. Before the first release there are no migrations: a change of the
/// schema takes the next number, and a database of another number is refused.
const VERSION: i32 = 24;

const SCHEMA: &str = "
CREATE TABLE outbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_ms INTEGER NOT NULL,
    body TEXT NOT NULL
);
-- `request` is what the request of the key was: a repeat of the key with another one is refused.
CREATE TABLE idempotency (
    key TEXT PRIMARY KEY,
    reply TEXT NOT NULL,
    at_ms INTEGER NOT NULL DEFAULT 0,
    request TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idempotency_at ON idempotency (at_ms);
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

-- Runs and their attempts (docs/design/contracts.md 6). `waiting` is the `seq` of the deciding event whose decision
-- the run waits for; `next` is the attempt kind an `interrupted` run starts when its `after` passes; `waiting_for`
-- is why a queued run waits; `continue_from` is the run whose session a run continues; `account` and `model` are the
-- gate keys its attempts reported (contracts 7); `trigger` (`<pack>.<name>`) started it, for the overlap check of
-- docs/design/config.md; `bead` and `bead_data` are its bead as the claim returned it; `step` and `step_kind` are
-- the step of the formula it is at (a drain step has no agent, and its state is read in SQL); `as_agent` is the
-- agent the `agent:` label of its bead names for its ai steps; `round`, `remarks` and `back_from` (a JSON array of
-- names) are of the checks that sent it back; `step_attempt` and `step_remarks` are the execution of its step and the remark of the one before.
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
    started_ms INTEGER,
    waiting_for TEXT,
    continue_from TEXT,
    account TEXT,
    model TEXT,
    trigger TEXT,
    bead TEXT,
    bead_data TEXT,
    step TEXT NOT NULL DEFAULT '1',
    as_agent TEXT,
    step_kind TEXT NOT NULL DEFAULT 'ai',
    round INTEGER NOT NULL DEFAULT 1,
    remarks TEXT,
    step_attempt INTEGER NOT NULL DEFAULT 1,
    step_remarks TEXT,
    back_from TEXT
);
CREATE INDEX runs_state ON runs (state);
CREATE INDEX runs_trigger ON runs (trigger);
-- The finisher (contracts 6.3): the attempt token by its SHA-256, the declared `outcome` and the number of `prompts`
-- the attempt sent (remarks = prompts - 1). `waited`: its failure parked the run on an account gate, so it is
-- outside the attempts ceiling (invariant 16). The tokens it used (contracts 4.3) and the step it ran. `skipped`:
-- the step was skipped by its `if` since, so its outcome is no longer the step's.
CREATE TABLE attempts (
    run TEXT NOT NULL,
    n INTEGER NOT NULL,
    kind TEXT NOT NULL,
    harness TEXT NOT NULL,
    instance TEXT,
    handle TEXT,
    session TEXT,
    turn_ended INTEGER NOT NULL DEFAULT 0,
    -- What was left of the step timeout when its turn ended: the step timeout pauses between turns.
    paused_ms INTEGER,
    started_ms INTEGER NOT NULL,
    ended_ms INTEGER,
    failure TEXT,
    token_sha256 TEXT,
    outcome TEXT,
    prompts INTEGER NOT NULL DEFAULT 1,
    services TEXT,
    waited INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    step TEXT NOT NULL DEFAULT '1',
    skipped INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (run, n)
);
CREATE INDEX attempts_instance ON attempts (instance);
CREATE INDEX attempts_token ON attempts (token_sha256);
-- The delivery record of every prompt of an attempt (contracts 6.4): `n` counts from 1.
CREATE TABLE prompts (
    run TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    n INTEGER NOT NULL,
    key TEXT NOT NULL,
    text TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('intent', 'sent', 'acked', 'settled')),
    PRIMARY KEY (run, attempt, n)
);

-- Issue gates (contracts 7): one closure per `(key, closed_by)`.
CREATE TABLE gates (
    key TEXT NOT NULL,
    closed_by TEXT NOT NULL,
    until_ms INTEGER,
    reason TEXT NOT NULL,
    closed_ms INTEGER NOT NULL,
    PRIMARY KEY (key, closed_by)
);
-- The last limit each account reported (contracts 4.3).
CREATE TABLE account_limits (account TEXT PRIMARY KEY, utilization REAL NOT NULL, resets_at_ms INTEGER);

-- The instance registry (contracts 8): a row is written before its process starts or stops (invariant 9); `handle`
-- is the `InstanceHandle` once the process runs; `degraded` is why a ready instance is degraded, while it is; `seq`
-- is that of the last driver event taken from the instance (contracts 4.3).
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
    idle_since_ms INTEGER,
    degraded TEXT,
    seq INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX instances_fingerprint ON instances (fingerprint, state);

-- Bead ownership (contracts 5, invariants 2-4): the owner run of a bead (the truth the tracker's assignee mirrors),
-- the generation of its claims and the failed runs counted toward its gate; the last ready set of each project's
-- tracker, in the tracker's order; the last tracker write each project's tracker events were read to (`Change::seq`).
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
CREATE TABLE tracker_cursors (project TEXT PRIMARY KEY, seq INTEGER NOT NULL);
-- The triggers the operator switched off, by `<project>/<rig>/<trigger>`.
CREATE TABLE triggers_off (id TEXT PRIMARY KEY);
";

/// How long a statement waits for a lock held by another connection before it fails with `SQLITE_BUSY`.
const BUSY: std::time::Duration = std::time::Duration::from_secs(5);

/// Opens the state database in WAL mode, creating its schema in a new one; a database of another schema is an error.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    // Created 0600 whatever the umask; SQLite gives its WAL files the mode of the database. A failure here is the
    // open's to report.
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path);
    let mut conn = Connection::open(path)?;
    // The daemon is the only writer, so a busy database means an outside lock: wait it out this long, then fail.
    conn.busy_timeout(BUSY)?;
    // A deferred transaction that reads first fails at once when it turns to write under that lock (SQLite does not
    // wait on the upgrade); taking the write lock at BEGIN puts every transaction of this connection under the wait.
    conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL in WAL mode survives a process crash (kill -9, criterion 8); only power loss can drop a commit.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    match conn.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))? {
        0 => conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {SCHEMA} PRAGMA user_version = {VERSION}; COMMIT;"
        ))?,
        VERSION => {}
        other => {
            let why = format!(
                "the state is of schema {other} and this build of aae has schema {VERSION}: it is not migrated \
                 before the first release; move the state directory away to start anew"
            );
            let code = rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_SCHEMA);
            return Err(rusqlite::Error::SqliteFailure(code, Some(why)));
        }
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
