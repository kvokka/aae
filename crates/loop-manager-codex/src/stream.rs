//! The messages of one `codex app-server` read into normalized events per thread (docs/design/contracts.md 4.3,
//! 4.6; docs/design/experiments.md, experiment 2). A turn ends on `turn/completed` only; the thread status `idle`
//! comes just before it and ends nothing (docs/design/contracts.md 4.6).

use loop_manager_contract::driver::{Failure, FailureKind, Normalized, RateLimit, SessionState, Source, StopReason};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

/// The first words of a `usageLimitExceeded` that is no limit to wait out but a plan or credit matter (the
/// messages of `QuotaExceeded`, `UsageNotIncluded` and the workspace credit and spend cap of codex 0.168).
const BILLING_WORDS: &[&str] = &[
    "Quota exceeded.",
    "To use Codex with your ChatGPT plan",
    "Your workspace is out of credits.",
    "You hit your spend cap",
];

/// How much of a harness message a failure keeps.
const RAW_MAX: usize = 4096;

/// The normalized events of one message, each with the thread it belongs to (`None`: instance-wide).
pub(crate) type Events = Vec<(Option<String>, Normalized)>;

/// What is known of one thread between messages.
#[derive(Default)]
struct Thread {
    model: Option<String>,
    /// A turn started and no `turn/completed` closed it yet.
    open: bool,
    /// We sent `turn/interrupt`: the next `turn/completed` is cancelled whatever its status says.
    interrupted: bool,
    /// The last `error{willRetry:false}` of the turn.
    error: Option<Value>,
    retries: u32,
    auth_reported: bool,
    /// An MCP server of the thread that did not start, told while no turn was open: the core may not know the thread
    /// yet, so it waits for the next prompt or turn (`held`).
    held: Option<Failure>,
    /// `commandExecution` items started and not completed: work that outlives the turn.
    commands: HashSet<String>,
    activity: Option<(Instant, bool)>,
}

pub(crate) struct Stream {
    account: Option<String>,
    threads: HashMap<String, Thread>,
    /// The last `account/rateLimits/updated` snapshot.
    limits: Value,
}

impl Stream {
    pub(crate) fn new(account: Option<String>) -> Self {
        Self {
            account,
            threads: HashMap::new(),
            limits: Value::Null,
        }
    }

    /// A thread of this process runs `model`: its failures name it.
    pub(crate) fn bind(&mut self, thread: &str, model: Option<String>) {
        self.threads.entry(thread.to_owned()).or_default().model = model;
    }

    /// `turn/start` was accepted: the turn is open before its `turn/started` arrives.
    pub(crate) fn accepted(&mut self, thread: &str) {
        self.threads.entry(thread.to_owned()).or_default().open = true;
    }

    /// The failure of an MCP server of `thread` that waits to be told, taken.
    pub(crate) fn held(&mut self, thread: &str) -> Option<Failure> {
        self.threads.get_mut(thread)?.held.take()
    }

    /// We interrupt the open turn of `thread`; false when no turn is open and there is nothing to interrupt.
    pub(crate) fn interrupted(&mut self, thread: &str) -> bool {
        let Some(open) = self.threads.get_mut(thread).filter(|t| t.open) else {
            return false;
        };
        open.interrupted = true;
        true
    }

    /// The threads with a turn in flight.
    pub(crate) fn open(&self) -> Vec<String> {
        self.threads
            .iter()
            .filter(|(_, t)| t.open)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// One notification or server request seen at `now`; unknown methods yield nothing.
    pub(crate) fn message(&mut self, msg: &Value, now: Instant) -> Events {
        let params = &msg["params"];
        let Some(method) = msg["method"].as_str() else {
            return vec![];
        };
        if method == "account/rateLimits/updated" {
            return self.rate_limits(&params["rateLimits"]);
        }
        let id = params["threadId"].as_str().unwrap_or_default().to_owned();
        if id.is_empty() {
            return vec![];
        }
        if msg.get("id").is_some() {
            // A server request: the adapter refuses every one (`rpc::refusal`), so each is a denial of the thread.
            return vec![(
                Some(id),
                Normalized::PermissionDenied {
                    tool: method.to_owned(),
                    tool_use_id: params["itemId"].as_str().map(str::to_owned),
                    input: params.clone(),
                    reason: params["reason"].as_str().map(str::to_owned),
                },
            )];
        }
        let thread = self.threads.entry(id.clone()).or_default();
        let events = match method {
            "turn/started" => {
                thread.open = true;
                let held = thread.held.take().map(Normalized::Failure);
                [Normalized::TurnStarted].into_iter().chain(held).collect()
            }
            // A server of the rig or the host (loop-manager's own are `required` and fail `thread/start` instead).
            "mcpServer/startupStatus/updated" if params["status"] == "failed" => {
                let failure = fail(&self.account, &thread.model, FailureKind::ToolFailure, msg.to_string());
                if !thread.open {
                    thread.held = Some(failure);
                    return vec![];
                }
                vec![Normalized::Failure(failure)]
            }
            "turn/completed" => return self.completed(&id, &params["turn"]),
            "thread/status/changed" => {
                let status = &params["status"];
                let state = match status["type"].as_str() {
                    // In a turn `idle` comes just before `turn/completed`; the state follows the turn end instead.
                    Some("idle") if thread.open => return vec![],
                    Some("idle") => SessionState::Idle,
                    Some("active") => SessionState::Running,
                    _ => return vec![],
                };
                let background = !thread.commands.is_empty();
                vec![Normalized::State { state, background }]
            }
            "error" if params["willRetry"] == true => {
                thread.retries += 1;
                let mut out = vec![Normalized::Retrying {
                    attempt: thread.retries,
                    delay: Duration::ZERO,
                    text: params["error"]["message"].as_str().unwrap_or_default().to_owned(),
                }];
                // codex retries a 401 a few times; the account is known to be logged out at the first one.
                let auth = matches!(info(&params["error"]), ("unauthorized", _) | (_, Some(401 | 403)));
                if auth && !std::mem::replace(&mut thread.auth_reported, true) {
                    let mut failure = fail(&self.account, &thread.model, FailureKind::AuthRequired, msg.to_string());
                    failure.retried_by_harness = true;
                    out.push(Normalized::Failure(failure));
                }
                out
            }
            "error" => {
                thread.error = Some(params["error"].clone());
                vec![]
            }
            "thread/tokenUsage/updated" => {
                let last = &params["tokenUsage"]["last"];
                vec![Normalized::Usage {
                    input_tokens: last["inputTokens"].as_u64().unwrap_or_default(),
                    output_tokens: last["outputTokens"].as_u64().unwrap_or_default(),
                    rate_limit: None,
                }]
            }
            "item/started" | "item/completed" => {
                let item = &params["item"];
                let command = item["type"] == "commandExecution";
                if command {
                    let item_id = item["id"].as_str().unwrap_or_default().to_owned();
                    match method {
                        "item/started" => thread.commands.insert(item_id),
                        _ => thread.commands.remove(&item_id),
                    };
                }
                let mut out = activity(thread, now, !thread.commands.is_empty());
                // The last background command of an ended turn is done: the thread is idle without background.
                if command && method == "item/completed" && !thread.open && thread.commands.is_empty() {
                    out.push(Normalized::State {
                        state: SessionState::Idle,
                        background: false,
                    });
                }
                out
            }
            _ if method.ends_with("/delta") => {
                let busy = !thread.commands.is_empty();
                activity(thread, now, busy)
            }
            _ => vec![],
        };
        events.into_iter().map(|e| (Some(id.clone()), e)).collect()
    }

    /// The process exited: every open turn is a crash of its thread, then the instance is gone.
    pub(crate) fn exited(&mut self, code: Option<i32>, signal: Option<i32>, stderr: &str) -> Events {
        let mut how = match (code, signal) {
            (Some(code), _) => format!("exited with code {code} in a turn"),
            (None, signal) => format!("killed by signal {} in a turn", signal.unwrap_or_default()),
        };
        if !stderr.trim().is_empty() {
            how = format!("{how}: {}", stderr.trim());
        }
        let mut out: Events = std::mem::take(&mut self.threads)
            .into_iter()
            .filter(|(_, t)| t.open)
            .map(|(id, t)| {
                let failure = fail(&self.account, &t.model, FailureKind::HarnessCrash, how.clone());
                (Some(id), Normalized::Failure(failure))
            })
            .collect();
        out.push((None, Normalized::Exited { code, signal }));
        out
    }

    fn completed(&mut self, id: &str, turn: &Value) -> Events {
        let thread = self.threads.remove(id).unwrap_or_default();
        let mut out = Vec::new();
        let stop_reason = match (turn["status"].as_str(), thread.interrupted) {
            (Some("completed"), _) => StopReason::Completed,
            (_, true) | (Some("interrupted"), _) => StopReason::Cancelled,
            _ => {
                let error = thread.error.as_ref().unwrap_or(&turn["error"]);
                out.push(Normalized::Failure(self.classify(error, &thread)));
                StopReason::Failed
            }
        };
        out.push(Normalized::TurnEnded { stop_reason });
        let background = !thread.commands.is_empty();
        out.push(Normalized::State {
            state: SessionState::Idle,
            background,
        });
        // The commands of a turn outlive it (a background command): they stay known to the thread.
        let rest = Thread {
            model: thread.model,
            commands: thread.commands,
            ..Thread::default()
        };
        self.threads.insert(id.to_owned(), rest);
        out.into_iter().map(|e| (Some(id.to_owned()), e)).collect()
    }

    /// The kind of a turn's final error by its `codexErrorInfo` (variant and HTTP status); the variants are those of
    /// `codex-rs/app-server-protocol` v2 of codex 0.168. What no variant or status names is `unknown`.
    fn classify(&self, error: &Value, thread: &Thread) -> Failure {
        let kind = match info(error) {
            ("usageLimitExceeded", _) => self.usage_limit(error),
            ("rateLimitExceeded", _) | (_, Some(429)) => FailureKind::RateLimited,
            ("serverOverloaded" | "internalServerError" | "flexUnavailable" | "policyCapacityUnavailable", _)
            | (_, Some(408 | 500..=504 | 529)) => FailureKind::Overloaded,
            ("unauthorized", _) | (_, Some(401 | 403)) => FailureKind::AuthRequired,
            ("contextWindowExceeded", _) => FailureKind::ContextOverflow,
            ("modelNotFound" | "badRequest" | "sandboxError", _) | (_, Some(404)) => FailureKind::ConfigError,
            ("sessionBudgetExceeded", _) => FailureKind::RunawayTurn,
            _ => FailureKind::Unknown,
        };
        let mut failure = fail(&self.account, &thread.model, kind, error.to_string());
        match kind {
            FailureKind::QuotaExhausted => failure.retry_at_ms = self.reset_at(),
            FailureKind::Unknown => {
                let text = error["message"].as_str().map(str::to_owned);
                failure.text = text.or_else(|| Some(format!("codexErrorInfo={}", error["codexErrorInfo"])));
            }
            _ => {}
        }
        // codex retries 5xx itself; a 429 only when the provider sent `retry-after` (the turn saw a retry).
        failure.retried_by_harness =
            kind == FailureKind::Overloaded || (kind == FailureKind::RateLimited && thread.retries > 0);
        failure
    }

    /// codex words a used-up plan, a missing plan, no credit and a spend cap all as `usageLimitExceeded`: the
    /// snapshot tells the workspace credit and spend cap apart, the message the plan matters.
    fn usage_limit(&self, error: &Value) -> FailureKind {
        let reached = self.limits["rateLimitReachedType"].as_str().unwrap_or_default();
        let message = error["message"].as_str().unwrap_or_default();
        if reached.starts_with("workspace_")
            || self.limits["spendControlReached"] == true
            || BILLING_WORDS.iter().any(|w| message.starts_with(w))
        {
            FailureKind::Billing
        } else {
            FailureKind::QuotaExhausted
        }
    }

    /// When the exhausted window of the last snapshot resets, in ms.
    fn reset_at(&self) -> Option<i64> {
        ["primary", "secondary"]
            .iter()
            .map(|w| &self.limits[w])
            .filter(|w| w["usedPercent"].as_f64().is_some_and(|p| p >= 100.0))
            .find_map(|w| w["resetsAt"].as_i64())
            .map(|s| s * 1000)
    }

    /// A snapshot is sparse: what it names replaces the last one's, and a window it does not name is forgotten once
    /// past its reset; it goes to every thread with a turn in flight.
    fn rate_limits(&mut self, limits: &Value) -> Events {
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        for field in ["rateLimitReachedType", "spendControlReached"] {
            self.limits[field] = limits[field].clone();
        }
        for window in ["primary", "secondary"] {
            if !limits[window].is_null() {
                self.limits[window] = limits[window].clone();
            } else if self.limits[window]["resetsAt"].as_i64().is_some_and(|at| at <= now_s) {
                self.limits[window] = Value::Null;
            }
        }
        let worst = ["primary", "secondary"]
            .iter()
            .map(|w| &self.limits[w])
            .filter(|w| !w.is_null())
            .max_by(|a, b| {
                a["usedPercent"]
                    .as_f64()
                    .partial_cmp(&b["usedPercent"].as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some(worst) = worst else { return vec![] };
        let rate_limit = RateLimit {
            utilization: worst["usedPercent"].as_f64().unwrap_or_default() / 100.0,
            resets_at_ms: worst["resetsAt"].as_i64().map(|s| s * 1000),
        };
        self.open()
            .into_iter()
            .map(|id| {
                let usage = Normalized::Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                    rate_limit: Some(rate_limit),
                };
                (Some(id), usage)
            })
            .collect()
    }
}

/// The variant of a `codexErrorInfo` and its `httpStatusCode`: `"modelNotFound"` or
/// `{"httpConnectionFailed":{"httpStatusCode":401}}`.
fn info(error: &Value) -> (&str, Option<u64>) {
    match &error["codexErrorInfo"] {
        Value::String(variant) => (variant, None),
        Value::Object(map) => map
            .iter()
            .next()
            .map_or(("", None), |(variant, v)| (variant, v["httpStatusCode"].as_u64())),
        _ => ("", None),
    }
}

fn fail(account: &Option<String>, model: &Option<String>, kind: FailureKind, mut raw: String) -> Failure {
    if raw.len() > RAW_MAX {
        let mut cut = RAW_MAX;
        while !raw.is_char_boundary(cut) {
            cut -= 1;
        }
        raw.truncate(cut);
    }
    let mut failure = Failure::new(kind, Source::Adapter);
    failure.raw = raw;
    failure.account.clone_from(account);
    failure.provider_model.clone_from(model);
    failure
}

/// At most one activity a second, but a change of `tool_running` always goes through.
fn activity(thread: &mut Thread, now: Instant, tool_running: bool) -> Vec<Normalized> {
    let quiet = thread
        .activity
        .is_some_and(|(at, was)| was == tool_running && now.duration_since(at) < Duration::from_secs(1));
    if quiet {
        return vec![];
    }
    thread.activity = Some((now, tool_running));
    vec![Normalized::Activity { tool_running }]
}

#[cfg(test)]
mod tests;
