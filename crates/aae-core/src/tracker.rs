//! The tracker interface (spec §10.1, docs/design/contracts.md 5): the operations the core calls on the tracker of a
//! project. The daemon is the only claimer (invariant 3); the state of the orchestrator is the truth of ownership
//! and the assignee in the tracker mirrors it (§10.3).

use crate::writer::BoxFuture;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A bead (contracts 5.2); `epic` is computed by the adapter, the rig of a bead is its `rig:<name>` label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bead {
    pub id: String,
    pub title: String,
    pub description: String,
    /// `open`, `in_progress`, `blocked`, `deferred` or `closed`.
    pub status: String,
    pub priority: i64,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    pub epic: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
}

/// One write to the tracker (contracts 5.2): `seq` orders the writes and is the cursor of `changes`; `actor` is who
/// made it (`aae:<run id>[:<attempt n>]` for aae and its sessions); `changed` names the fields of an `updated`.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub seq: i64,
    /// `created`, `updated` or `closed`.
    pub kind: String,
    pub actor: String,
    pub changed: Vec<String>,
    /// The bead as it is when the change is read.
    pub bead: Bead,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrackerError {
    NotFound,
    /// Someone else holds the bead: told by the state of the bead after the call.
    Claimed {
        holder: String,
    },
    /// A conditional release or close found another owner; nothing changed.
    OwnerMismatch {
        holder: Option<String>,
    },
    /// Transient: the effect queue retries it with backoff.
    Unavailable {
        text: String,
    },
    /// Permanent.
    Invalid {
        text: String,
    },
}

impl fmt::Display for TrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("no such bead"),
            Self::Claimed { holder } => write!(f, "claimed by `{holder}`"),
            Self::OwnerMismatch { holder } => write!(f, "held by `{}`", holder.as_deref().unwrap_or("nobody")),
            Self::Unavailable { text } => write!(f, "tracker unavailable: {text}"),
            Self::Invalid { text } => write!(f, "invalid: {text}"),
        }
    }
}

/// The operations of contracts 5.1 the core calls; `owner` is `aae:<run id>`. The adapter adds the rest of the twelve
/// as the API and the event source need them.
pub trait Tracker: Send + Sync {
    fn claim<'a>(&'a self, owner: &'a str, id: &'a str) -> BoxFuture<'a, Result<Bead, TrackerError>>;
    /// Conditional on `assignee == owner`.
    fn release<'a>(&'a self, owner: &'a str, id: &'a str) -> BoxFuture<'a, Result<(), TrackerError>>;
    /// Conditional on `assignee == owner`.
    fn close<'a>(&'a self, owner: &'a str, id: &'a str, reason: &'a str) -> BoxFuture<'a, Result<(), TrackerError>>;
    /// Every ready bead, in the tracker's order (`ready(None)` of contracts 5.1: the core picks within an epic itself).
    fn ready(&self) -> BoxFuture<'_, Result<Vec<Bead>, TrackerError>>;
    /// The writes after `after` (a `seq`, 0 for all), oldest first.
    fn changes(&self, after: i64) -> BoxFuture<'_, Result<Vec<Change>, TrackerError>>;
}
