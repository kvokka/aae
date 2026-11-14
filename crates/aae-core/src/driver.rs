//! The driver interface between the core and a harness adapter (docs/design/contracts.md 4). The adapter
//! translates and executes; it never retries a turn, parks or decides an outcome (spec §4, §7.2).

use crate::{BoxFuture, config::EffectiveConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

mod failure;
pub use failure::{Failure, FailureKind, Scope, Source};

/// The part of the effective config that needs its own process; opaque to the core.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessPart(pub Value);

/// The part of the effective config that a session on a shared process carries; opaque to the core.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionPart(pub Value);

/// SHA-256 of the harness name and the canonical JSON of a [`ProcessPart`], hex: sessions whose process parts are
/// equal share an instance (contracts 8).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint(pub String);

impl Fingerprint {
    pub fn of(harness: &str, process: &ProcessPart) -> Self {
        // `serde_json` maps keep their keys sorted, so the text is canonical.
        let digest = Sha256::new()
            .chain_update(harness.as_bytes())
            .chain_update([0])
            .chain_update(process.0.to_string().as_bytes())
            .finalize();
        Self(digest.iter().map(|b| format!("{b:02x}")).collect())
    }
}

/// The effective config split by the adapter into what needs its own process and what a session carries.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub process: ProcessPart,
    pub session: SessionPart,
    pub fingerprint: Fingerprint,
}

impl Split {
    pub fn new(harness: &str, process: ProcessPart, session: SessionPart) -> Self {
        let fingerprint = Fingerprint::of(harness, &process);
        Self {
            process,
            session,
            fingerprint,
        }
    }
}

/// What the session of an attempt is; `run` and `attempt` name files in the state directory. The attempt environment:
/// `run`, `token` (`AAE_RUN`, `AAE_ATTEMPT_TOKEN`, contracts 6.3) and `actor` (the project's tracker actor, 5.1).
#[derive(Clone, Debug, PartialEq)]
pub enum SessionPlan {
    New {
        workdir: PathBuf,
        run: String,
        attempt: u32,
        token: String,
        actor: Option<(String, String)>,
    },
    Resume {
        session: SessionRef,
        workdir: PathBuf,
        run: String,
        attempt: u32,
        token: String,
        actor: Option<(String, String)>,
    },
}

/// Everything the adapter needs to resume a session; the core stores it and hands it back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionRef(pub Value);

/// A running harness process: `pid` and its kernel `start_time` identify the process (spec §5.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceHandle {
    pub id: String,
    pub pid: i32,
    pub start_time: u64,
    pub pgid: i32,
}

/// The idempotency key of one prompt (contracts 6.4), carried as the harness's own message id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PromptKey(pub Uuid);

impl PromptKey {
    /// UUID v5 of `"<run>/<attempt>/<prompt>"` in the URL namespace under the `aae:prompt:` scheme.
    pub fn new(run: &str, attempt: u32, prompt: u32) -> Self {
        let name = format!("aae:prompt:{run}/{attempt}/{prompt}");
        Self(Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()))
    }
}

/// A decision on `run.requires_action`.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Allow { updated_input: Option<Value> },
    Deny { message: String },
    Text { text: String },
}

/// Where the prompt with a given key stands in a session's transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transcript {
    Missing,
    Present { prompt: PromptState },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptState {
    Absent,
    Unanswered,
    Answered,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Health {
    Ok,
    Degraded { text: String },
    Dead,
}

/// The daemon's network settings a harness process inheriting the host gets too (spec §6.4): proxies and the
/// certificates to trust.
pub const HOST_NETWORK_ENV: [&str; 11] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub sessions_per_instance: usize,
    /// How long an instance with no sessions lives on before it is stopped (contracts 8).
    pub instance_idle: Duration,
    /// The tested versions of the harness binary, `[lowest, highest]` as `(major, minor, patch)`, inclusive.
    pub versions: [(u32, u32, u32); 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Running,
    Idle,
    RequiresAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    Permission,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RateLimit {
    pub utilization: f64,
    pub resets_at_ms: Option<i64>,
}

/// A normalized harness event (contracts 4.3): an input message of the core, not a journal event.
#[derive(Clone, Debug, PartialEq)]
pub enum Normalized {
    PromptAcked {
        key: PromptKey,
    },
    TurnStarted,
    TurnEnded {
        stop_reason: StopReason,
    },
    /// "Idle without background" is `Idle` with `background == false`; the adapter applies the harness rule.
    State {
        state: SessionState,
        background: bool,
    },
    PermissionRequested {
        request_id: String,
        kind: RequestKind,
        tool: String,
        input: Value,
        text: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        rate_limit: Option<RateLimit>,
    },
    Retrying {
        attempt: u32,
        delay: Duration,
        text: String,
    },
    /// Watchdog only; the adapter sends at most one per second per session.
    Activity {
        tool_running: bool,
    },
    Failure(Failure),
    /// The instance process is gone.
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DriverEvent {
    pub instance: String,
    /// `None` for instance-wide events.
    pub session: Option<SessionRef>,
    /// Monotonic per instance, from 1.
    pub seq: u64,
    pub event: Normalized,
}

pub type EventSink = mpsc::UnboundedSender<DriverEvent>;

/// One harness, behind one object-safe trait. The core owns one driver per harness.
pub trait Driver: Send + Sync {
    /// The harness name the config uses for it, `[harness.<name>]`.
    fn harness(&self) -> &'static str;
    fn capabilities(&self, process: &ProcessPart) -> Capabilities;
    /// The account id the gates use (spec §5.7): a stable hash of the credential source, never the credential.
    fn account(&self, process: &ProcessPart) -> Option<String>;
    /// A config the harness cannot express is a `config_error` failure of the attempt.
    fn split(&self, config: &EffectiveConfig, plan: &SessionPlan) -> Result<Split, Failure>;

    /// `id` is the registry's: the handle carries it, and the process environment holds it as `AAE_INSTANCE`.
    fn start_instance<'a>(
        &'a self,
        id: &'a str,
        process: &'a ProcessPart,
        sink: EventSink,
    ) -> BoxFuture<'a, Result<InstanceHandle, Failure>>;
    fn health<'a>(&'a self, instance: &'a InstanceHandle) -> BoxFuture<'a, Health>;
    fn stop_instance<'a>(&'a self, instance: &'a InstanceHandle, grace: Duration) -> BoxFuture<'a, ()>;

    fn start_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionPart,
    ) -> BoxFuture<'a, Result<SessionRef, Failure>>;
    fn resume_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        part: &'a SessionPart,
    ) -> BoxFuture<'a, Result<(), Failure>>;
    fn send_prompt<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        text: &'a str,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<(), Failure>>;
    fn cancel_turn<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
    ) -> BoxFuture<'a, Result<(), Failure>>;
    fn respond<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        request_id: &'a str,
        answer: &'a Answer,
    ) -> BoxFuture<'a, Result<(), Failure>>;
    /// Detaches the session from its instance; the transcript stays, so the session can be resumed later.
    fn close_session<'a>(&'a self, instance: &'a InstanceHandle, session: &'a SessionRef) -> BoxFuture<'a, ()>;

    fn transcript<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<Transcript, Failure>>;
    /// The raw harness messages of a session (spec §5.3 direct relay); a slow reader loses messages.
    fn live(&self, session: &SessionRef) -> Option<broadcast::Receiver<Value>>;
}

/// The last 4 KiB of `stderr`, read to its end (a descendant may hold it open past the exit: the caller bounds it).
pub async fn stderr_tail(mut stderr: tokio::process::ChildStderr) -> String {
    let (mut kept, mut buf) = (Vec::new(), [0; 4096]);
    while let Ok(n @ 1..) = tokio::io::AsyncReadExt::read(&mut stderr, &mut buf).await {
        kept.extend_from_slice(&buf[..n]);
        kept.drain(..kept.len().saturating_sub(buf.len()));
    }
    String::from_utf8_lossy(&kept).into_owned()
}
