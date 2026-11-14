//! The schemas of the replies the OpenAPI document shows: the core and the handlers build them as JSON, so these
//! types only describe them. A run view is left an object: its fields are `RunView` of docs/design/contracts.md
//! §1.3 with `project`, `rig`, `correlation`, `inputs`, `waiting_for` and `attempts`.
#![expect(dead_code, reason = "schemas of replies built as JSON; never constructed")]

use super::ErrorReply;
use std::collections::BTreeMap;
use utoipa::{IntoResponses, ToSchema};

/// The errors of an operation that takes an `Idempotency-Key`: the core carries it out.
#[derive(IntoResponses)]
pub(crate) enum Keyed {
    #[response(
        status = 400,
        description = "`bad_request`: a body unreadable or over 2 MiB, malformed JSON, \
        query or path, or a value the operation refuses"
    )]
    BadRequest(ErrorReply),
    #[response(
        status = 422,
        description = "`idempotency_key_reused`: the key came with another request"
    )]
    KeyReused(ErrorReply),
    #[response(status = 500, description = "`storage`: the state database failed")]
    Storage(ErrorReply),
    #[response(status = 503, description = "`stopped`: the core has stopped")]
    Stopped(ErrorReply),
}

/// `storage`, or `journal` on an event stream: the daemon could not read its state or journal.
#[derive(IntoResponses)]
#[response(
    status = 500,
    description = "`storage` or `journal`: the daemon could not read its state"
)]
pub(crate) struct Failed(ErrorReply);

/// A run started: `run` is its id; `nothing_ready` (and no `run`) when a bead input `{ next_ready = true }` found
/// no ready bead.
#[derive(ToSchema)]
pub(crate) struct Started {
    run: Option<String>,
    nothing_ready: Option<bool>,
}

/// A trigger fired by hand: `run`, or `skipped` (`overlap`, `depth`, `nothing_ready`, `input_unresolved`,
/// `rejected`, ...) with `detail` when no run started.
#[derive(ToSchema)]
pub(crate) struct Fired {
    run: Option<String>,
    skipped: Option<String>,
    detail: Option<String>,
}

#[derive(ToSchema)]
pub(crate) struct Switched {
    trigger: String,
    enabled: bool,
}

#[derive(ToSchema)]
pub(crate) struct Finished {
    /// `accepted`, `stale_attempt`, `run_terminal` or `invalid`.
    answer: String,
}

#[derive(ToSchema)]
pub(crate) struct Reloaded {
    config_generation: u64,
    /// The idle instances stopped because no agent of the new config uses them.
    restarted_instances: Vec<String>,
}

#[derive(ToSchema)]
pub(crate) struct Stopping {
    /// The `seq` of `daemon.stopping`.
    seq: i64,
    /// `drain` or `restart`.
    reason: String,
    active_runs: i64,
}

#[derive(ToSchema)]
pub(crate) struct InstanceStopping {
    stopping: bool,
}

#[derive(ToSchema)]
pub(crate) struct Checked {
    ok: bool,
    errors: Vec<String>,
}

#[derive(ToSchema)]
pub(crate) struct Explained {
    /// One line per value with its layer.
    text: String,
    inherits_host: bool,
}

#[derive(ToSchema)]
pub(crate) struct Daemon {
    version: String,
    pid: u32,
    socket: String,
    started_at: String,
    /// The last `seq` the journal wrote.
    journal_seq: i64,
    journal: JournalHealth,
    /// Outbox rows the journal has not acknowledged, and the age of the oldest one.
    outbox_depth: i64,
    outbox_oldest_age_ms: Option<i64>,
    /// Runs in a turn, a hook or a preparation: what a drain waits for.
    active_runs: i64,
    /// `drain` or `restart` once the daemon stops.
    stopping: Option<String>,
}

/// Failed journal writes in a row and the last error.
#[derive(ToSchema)]
pub(crate) struct JournalHealth {
    failures: u32,
    last_error: Option<String>,
}

/// An active gate closure.
#[derive(ToSchema)]
pub(crate) struct Gate {
    key: String,
    /// Absent: until opened by hand.
    until: Option<String>,
    reason: String,
    /// `operator`, `policy` or `core`.
    closed_by: String,
    closed_at: String,
}

/// A trigger on a rig: `on` for an event trigger, `schedule` and `next_at` for a scheduled one.
#[derive(ToSchema)]
pub(crate) struct Trigger {
    project: String,
    rig: String,
    trigger: String,
    formula: String,
    /// `event`, `cron` or `every`.
    source: String,
    on: Option<String>,
    schedule: Option<String>,
    next_at: Option<String>,
    enabled: bool,
}

/// A harness instance or service of the registry.
#[derive(ToSchema)]
pub(crate) struct Instance {
    id: String,
    kind: String,
    harness: String,
    fingerprint: String,
    state: String,
    sessions: i64,
    capacity: i64,
    pid: Option<i64>,
    pgid: Option<i64>,
    idle_since: Option<String>,
}

#[derive(ToSchema)]
pub(crate) struct Project {
    name: String,
    root: String,
    rigs: Vec<Rig>,
}

#[derive(ToSchema)]
pub(crate) struct Rig {
    name: String,
    path: String,
    vars: BTreeMap<String, String>,
}

#[derive(ToSchema)]
pub(crate) struct Agent {
    project: String,
    /// `<pack>.<name>`.
    agent: String,
    harness: String,
    max_runs: u32,
}

#[derive(ToSchema)]
pub(crate) struct Formula {
    project: String,
    /// As a run start takes it: `<pack>.<name>`, or the bare name in the project's own pack.
    formula: String,
    description: String,
    inputs: BTreeMap<String, FormulaInput>,
}

#[derive(ToSchema)]
pub(crate) struct FormulaInput {
    #[schema(rename = "type")]
    kind: String,
    required: bool,
}

/// A pack; the project's own pack has no version.
#[derive(ToSchema)]
pub(crate) struct Pack {
    project: String,
    name: String,
    version: Option<String>,
    root: String,
    requires_plugins: Vec<String>,
}

/// A declared plugin, or an enabled bundled one (no version nor root).
#[derive(ToSchema)]
pub(crate) struct Plugin {
    name: String,
    version: Option<String>,
    root: Option<String>,
    /// The events its hooks run on.
    on: Vec<String>,
    bundled: bool,
}
