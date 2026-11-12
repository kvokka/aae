//! The stdout of one `claude -p --output-format stream-json` session read into normalized events
//! (docs/design/contracts.md 4.3, 4.6; docs/design/experiments.md, experiment 1a and "Idle without background").

use aae_core::driver::{
    Failure, FailureKind, Normalized, PromptKey, RateLimit, RequestKind, SessionState, Source, StopReason,
};
use serde_json::Value;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// The harness wording of a failed turn → its kind; the first rule with a matching needle wins. Needles are
/// looked for in `<error> <api_error_status> <subtype> <terminal_reason> <result text>` of the turn.
const KINDS: &[(&[&str], FailureKind)] = &[
    (&["You've hit your"], FailureKind::QuotaExhausted),
    (
        &["billing_error", "account_on_hold", "credit balance", "credits_required"],
        FailureKind::Billing,
    ),
    (&["rate_limit", " 429 "], FailureKind::RateLimited),
    (
        &["overloaded", "server_error", " 529 ", " 500 "],
        FailureKind::Overloaded,
    ),
    (
        &["authentication_failed", " 401 ", "Not logged in"],
        FailureKind::AuthRequired,
    ),
    (&["prompt_too_long", "prompt is too long"], FailureKind::ContextOverflow),
    (&["model_not_found", " 404 "], FailureKind::ConfigError),
    (&["error_max_budget_usd", "error_max_turns"], FailureKind::RunawayTurn),
];

/// How much of a harness line a failure keeps.
const RAW_MAX: usize = 4096;

pub(crate) struct Stream {
    account: Option<String>,
    model: Option<String>,
    interrupted: bool,
    /// A prompt was taken or a turn began, and no result closed it yet.
    open: bool,
    /// The `error` and `api_error_status` the assistant reported in this turn.
    error: String,
    /// `resetsAt` of a rejected rate limit in this turn, in ms.
    rejected: Option<i64>,
    auth_reported: bool,
    background: bool,
    notified_since_result: bool,
    activity: Option<(Instant, bool)>,
}

impl Stream {
    pub(crate) fn new(account: Option<String>, model: Option<String>) -> Self {
        Self {
            account,
            model,
            interrupted: false,
            open: false,
            error: String::new(),
            rejected: None,
            auth_reported: false,
            background: false,
            notified_since_result: false,
            activity: None,
        }
    }

    /// We sent `control_request{interrupt}`: the next error result is `TurnEnded{Cancelled}`, never a failure.
    pub(crate) fn interrupted(&mut self) {
        self.interrupted = true;
    }

    /// One stdout line seen at `now`; the events it yields, in order. Unknown types yield nothing.
    pub(crate) fn line(&mut self, msg: &Value, now: Instant) -> Vec<Normalized> {
        let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
        match (msg["type"].as_str(), msg["subtype"].as_str()) {
            (Some("command_lifecycle"), _) if msg["state"] == "queued" => {
                match Uuid::parse_str(msg["command_uuid"].as_str().unwrap_or_default()) {
                    Ok(key) => {
                        self.open = true;
                        vec![Normalized::PromptAcked { key: PromptKey(key) }]
                    }
                    Err(_) => vec![],
                }
            }
            (Some("system"), Some("init")) => {
                self.open = true;
                let mut out = vec![Normalized::TurnStarted];
                let servers = msg["mcp_servers"].as_array().into_iter().flatten();
                let failed: Vec<String> = servers
                    .filter(|s| s["status"] == "failed")
                    .map(|s| text(&s["name"]))
                    .collect();
                if !failed.is_empty() {
                    out.push(Normalized::Failure(
                        self.failure(FailureKind::ToolFailure, failed.join(", ")),
                    ));
                }
                out
            }
            (Some("system"), Some("session_state_changed")) => {
                let state = match msg["state"].as_str() {
                    Some("idle") => SessionState::Idle,
                    Some("requires_action") => SessionState::RequiresAction,
                    Some("running") => SessionState::Running,
                    _ => return vec![],
                };
                let background = self.background || self.notified_since_result;
                vec![Normalized::State { state, background }]
            }
            (Some("system"), Some("background_tasks_changed")) => {
                self.background = msg["tasks"].as_array().is_some_and(|t| !t.is_empty());
                vec![]
            }
            (Some("system"), Some("task_notification")) => {
                self.notified_since_result = true;
                vec![]
            }
            (Some("system"), Some("api_retry")) => self.retry(msg),
            (Some("rate_limit_event"), _) => self.rate_limit(&msg["rate_limit_info"]),
            (Some("control_request"), _) if msg["request"]["subtype"] == "can_use_tool" => {
                let request = &msg["request"];
                let tool = text(&request["tool_name"]);
                let kind = match tool.as_str() {
                    "AskUserQuestion" => RequestKind::Question,
                    _ => RequestKind::Permission,
                };
                vec![Normalized::PermissionRequested {
                    request_id: text(&msg["request_id"]),
                    kind,
                    tool,
                    input: request["input"].clone(),
                    text: text(&request["description"]),
                }]
            }
            (Some("result"), _) => self.result(msg),
            (Some("assistant"), _) => {
                if let Some(error) = msg["error"].as_str() {
                    self.error = format!("{error} {} ", msg["api_error_status"]);
                }
                let content = msg["message"]["content"].as_array().into_iter().flatten();
                let tool = content.clone().any(|c| c["type"] == "tool_use");
                self.activity(now, tool)
            }
            (Some("tool_progress"), _) => self.activity(now, true),
            (Some("user" | "stream_event"), _) | (Some("system"), Some("status")) => self.activity(now, false),
            _ => vec![],
        }
    }

    /// stdout closed and the process exited: a turn left open is a crash, whatever the exit code.
    /// `stderr` is the tail of the process's stderr: the only word of a claude that dies on its own.
    pub(crate) fn exited(&mut self, code: Option<i32>, signal: Option<i32>, stderr: &str) -> Vec<Normalized> {
        let mut out = Vec::new();
        if std::mem::take(&mut self.open) {
            let mut how = match (code, signal) {
                (Some(code), _) => format!("exited with code {code} in a turn"),
                (None, signal) => format!("killed by signal {} in a turn", signal.unwrap_or_default()),
            };
            if !stderr.trim().is_empty() {
                how = format!("{how}: {}", stderr.trim());
            }
            out.push(Normalized::Failure(self.failure(FailureKind::HarnessCrash, how)));
        }
        out.push(Normalized::Exited { code, signal });
        out
    }

    fn result(&mut self, msg: &Value) -> Vec<Normalized> {
        let usage = &msg["usage"];
        let mut out = vec![Normalized::Usage {
            input_tokens: usage["input_tokens"].as_u64().unwrap_or_default(),
            output_tokens: usage["output_tokens"].as_u64().unwrap_or_default(),
            rate_limit: None,
        }];
        let failed = msg["is_error"].as_bool().unwrap_or(true) || msg["subtype"] != "success";
        let stop_reason = match (failed, std::mem::take(&mut self.interrupted)) {
            (false, _) => StopReason::Completed,
            (true, true) => StopReason::Cancelled,
            (true, false) => {
                out.push(Normalized::Failure(self.classify(msg)));
                StopReason::Failed
            }
        };
        out.push(Normalized::TurnEnded { stop_reason });
        self.open = false;
        self.error.clear();
        self.rejected = None;
        self.auth_reported = false;
        self.notified_since_result = false;
        out
    }

    fn classify(&self, msg: &Value) -> Failure {
        let words = format!(
            "{} {} {} {} {}",
            self.error, msg["api_error_status"], msg["subtype"], msg["terminal_reason"], msg["result"]
        );
        let raw = msg.to_string();
        if let Some(at) = self.rejected {
            let mut failure = self.failure(FailureKind::QuotaExhausted, raw);
            failure.retry_at_ms = Some(at);
            return failure;
        }
        let found = KINDS
            .iter()
            .find(|(needles, _)| needles.iter().any(|n| words.contains(n)));
        let Some((_, kind)) = found else {
            let mut failure = self.failure(FailureKind::Unknown, raw);
            failure.text = Some(msg["result"].as_str().unwrap_or_default().to_owned());
            return failure;
        };
        let mut failure = self.failure(*kind, raw);
        // claude retries 429 and 5xx itself before it gives up (capabilities: `retries_429`).
        failure.retried_by_harness = matches!(kind, FailureKind::RateLimited | FailureKind::Overloaded);
        failure
    }

    fn retry(&mut self, msg: &Value) -> Vec<Normalized> {
        let mut out = vec![Normalized::Retrying {
            attempt: msg["attempt"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or_default(),
            delay: Duration::from_millis(msg["retry_delay_ms"].as_u64().unwrap_or_default()),
            text: msg["error"].as_str().unwrap_or_default().to_owned(),
        }];
        // claude retries a 401 for about a minute; the account is known to be logged out at the first one.
        let auth = msg["error_status"] == 401 || msg["error"] == "authentication_failed";
        if auth && !std::mem::replace(&mut self.auth_reported, true) {
            let mut failure = self.failure(FailureKind::AuthRequired, msg.to_string());
            failure.retried_by_harness = true;
            out.push(Normalized::Failure(failure));
        }
        out
    }

    fn rate_limit(&mut self, info: &Value) -> Vec<Normalized> {
        let resets_at_ms = info["resetsAt"].as_i64().map(|s| s * 1000);
        if info["status"] == "rejected" {
            self.rejected = resets_at_ms;
        }
        if info["errorCode"] == "credits_required" {
            self.error = "credits_required ".to_owned();
        }
        let window = info["rateLimitType"].as_str().unwrap_or_default();
        let utilization = info["unifiedWindows"][window]["utilization"]
            .as_f64()
            .unwrap_or_default();
        vec![Normalized::Usage {
            input_tokens: 0,
            output_tokens: 0,
            rate_limit: Some(RateLimit {
                utilization,
                resets_at_ms,
            }),
        }]
    }

    /// At most one activity a second, but a change of `tool_running` always goes through.
    fn activity(&mut self, now: Instant, tool_running: bool) -> Vec<Normalized> {
        let quiet = self
            .activity
            .is_some_and(|(at, was)| was == tool_running && now.duration_since(at) < Duration::from_secs(1));
        if quiet {
            return vec![];
        }
        self.activity = Some((now, tool_running));
        vec![Normalized::Activity { tool_running }]
    }

    fn failure(&self, kind: FailureKind, mut raw: String) -> Failure {
        if raw.len() > RAW_MAX {
            let mut cut = RAW_MAX;
            while !raw.is_char_boundary(cut) {
                cut -= 1;
            }
            raw.truncate(cut);
        }
        let mut failure = Failure::new(kind, Source::Adapter);
        failure.raw = raw;
        failure.account.clone_from(&self.account);
        failure.provider_model.clone_from(&self.model);
        failure
    }
}

#[cfg(test)]
mod tests;
