//! The failure taxonomy an adapter or the core reports (spec §7.1, docs/design/contracts.md 4.4).

use serde::Serialize;

/// The failure kinds of spec §7.1 (contracts 4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    QuotaExhausted,
    RateLimited,
    Overloaded,
    AuthRequired,
    Billing,
    ContextOverflow,
    ConfigError,
    HarnessCrash,
    AdapterCrash,
    DaemonCrash,
    Stall,
    RunawayTurn,
    ToolFailure,
    TranscriptDivergence,
    WorkspaceFailure,
    HollowDone,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Session,
    Instance,
    Account,
    ProviderModel,
    Run,
    Rig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Harness,
    Adapter,
    Core,
}

impl FailureKind {
    /// The scope a failure of this kind has unless the harness says otherwise (contracts 4.4).
    pub fn default_scope(self) -> Scope {
        use FailureKind::*;
        match self {
            QuotaExhausted | AuthRequired | Billing => Scope::Account,
            RateLimited | Overloaded => Scope::ProviderModel,
            AdapterCrash => Scope::Instance,
            DaemonCrash | HollowDone => Scope::Run,
            WorkspaceFailure => Scope::Rig,
            ContextOverflow | ConfigError | HarnessCrash | Stall | RunawayTurn | ToolFailure | TranscriptDivergence
            | Unknown => Scope::Session,
        }
    }
}

/// A failure as the adapter or the core reports it (contracts 1.3).
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub kind: FailureKind,
    /// The harness wording, set only for `Unknown`.
    pub text: Option<String>,
    pub scope: Scope,
    pub retry_at_ms: Option<i64>,
    pub retried_by_harness: bool,
    pub source: Source,
    /// The harness text, cut to 4 KiB.
    pub raw: String,
    pub account: Option<String>,
    pub provider_model: Option<String>,
}

impl Failure {
    /// A failure of `kind` with its default scope and nothing else known.
    pub fn new(kind: FailureKind, source: Source) -> Self {
        Self {
            kind,
            text: None,
            scope: kind.default_scope(),
            retry_at_ms: None,
            retried_by_harness: false,
            source,
            raw: String::new(),
            account: None,
            provider_model: None,
        }
    }
}
