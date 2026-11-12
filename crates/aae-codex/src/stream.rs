//! The messages of one `codex app-server` read into normalized events per thread (docs/design/contracts.md 4.3,
//! 4.6; docs/design/experiments.md, experiment 2). A turn ends on `turn/completed` only; the thread status `idle`
//! comes just before it and ends nothing (spec §7.5).

use aae_core::driver::{Failure, FailureKind, Normalized, RateLimit, RequestKind, SessionState, Source, StopReason};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

/// The harness wording of a failed turn → its kind; the first rule with a matching needle wins. Needles are looked
/// for in `<codexErrorInfo> <message> <additionalDetails>` of the turn's final error.
const KINDS: &[(&[&str], FailureKind)] = &[
    (&["usageLimitExceeded"], FailureKind::QuotaExhausted),
    (
        &["insufficient_quota", "usage_not_included", "billing"],
        FailureKind::Billing,
    ),
    (&["rateLimitExceeded", ":429}"], FailureKind::RateLimited),
    (
        &[
            "serverOverloaded",
            "internalServerError",
            ":500}",
            ":502}",
            ":503}",
            ":529}",
        ],
        FailureKind::Overloaded,
    ),
    (&["unauthorized", ":401}"], FailureKind::AuthRequired),
    (&["contextWindowExceeded"], FailureKind::ContextOverflow),
    (&["modelNotFound", "badRequest", ":404}"], FailureKind::ConfigError),
    (&["sessionBudgetExceeded"], FailureKind::RunawayTurn),
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
        let thread = self.threads.entry(id.clone()).or_default();
        let events = match method {
            "turn/started" => {
                thread.open = true;
                vec![Normalized::TurnStarted]
            }
            "turn/completed" => return self.completed(&id, &params["turn"]),
            "thread/status/changed" => {
                let status = &params["status"];
                let state = match status["type"].as_str() {
                    // In a turn `idle` comes just before `turn/completed`; the state follows the turn end instead.
                    Some("idle") if thread.open => return vec![],
                    Some("idle") => SessionState::Idle,
                    Some("active") => match status["activeFlags"].as_array().is_some_and(|f| !f.is_empty()) {
                        true => SessionState::RequiresAction,
                        false => SessionState::Running,
                    },
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
                let auth = params["error"]["codexErrorInfo"].to_string().contains(":401}");
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
            "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
            | "item/tool/requestUserInput" => {
                let kind = match method {
                    "item/tool/requestUserInput" => RequestKind::Question,
                    _ => RequestKind::Permission,
                };
                let tool = method.trim_start_matches("item/").split('/').next().unwrap_or_default();
                let text = params["command"]
                    .as_str()
                    .or(params["reason"].as_str())
                    .unwrap_or_default();
                vec![Normalized::PermissionRequested {
                    request_id: msg["id"].to_string(),
                    kind,
                    tool: tool.to_owned(),
                    input: params.clone(),
                    text: text.to_owned(),
                }]
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

    fn classify(&self, error: &Value, thread: &Thread) -> Failure {
        let words = format!(
            "{} {} {}",
            error["codexErrorInfo"].to_string().replace(' ', ""),
            error["message"],
            error["additionalDetails"]
        );
        let found = KINDS
            .iter()
            .find(|(needles, _)| needles.iter().any(|n| words.contains(n)));
        let Some((_, kind)) = found else {
            let mut failure = fail(&self.account, &thread.model, FailureKind::Unknown, error.to_string());
            failure.text = error["message"].as_str().map(str::to_owned);
            return failure;
        };
        let mut failure = fail(&self.account, &thread.model, *kind, error.to_string());
        if *kind == FailureKind::QuotaExhausted {
            failure.retry_at_ms = self.reset_at();
        }
        // codex retries 5xx itself; a 429 only when the provider sent `retry-after` (the turn saw a retry).
        failure.retried_by_harness =
            *kind == FailureKind::Overloaded || (*kind == FailureKind::RateLimited && thread.retries > 0);
        failure
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

    /// A snapshot is sparse: what it names replaces the last one's; it goes to every thread with a turn in flight.
    fn rate_limits(&mut self, limits: &Value) -> Events {
        for window in ["primary", "secondary"] {
            if !limits[window].is_null() {
                self.limits[window] = limits[window].clone();
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
