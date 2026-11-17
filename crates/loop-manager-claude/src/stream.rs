//! The stdout of one `claude -p --output-format stream-json` session read into normalized events
//! (docs/design/contracts.md 4.3, 4.6; docs/design/experiments.md, experiment 1a and "Idle without background").

use loop_manager_contract::driver::{
    Failure, FailureKind, Normalized, PromptKey, RateLimit, SessionState, Source, StopReason,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// How much of a harness line a failure keeps.
const RAW_MAX: usize = 4096;

pub(crate) struct Stream {
    account: Option<String>,
    model: Option<String>,
    interrupted: bool,
    /// A prompt was taken or a turn began, and no result closed it yet.
    open: bool,
    /// The `error` and `api_error_status` the assistant reported in this turn.
    error: Option<String>,
    status: Option<u64>,
    /// `resetsAt` of a rejected rate limit in this turn, in ms.
    rejected: Option<i64>,
    /// A rate limit of this turn said `credits_required`.
    credits: bool,
    auth_reported: bool,
    background: bool,
    /// The last `background_tasks_changed` lists a subagent (`local_agent`).
    agent: bool,
    /// `task_id` of every shell command of the last `background_tasks_changed`: what `stop_task` may stop.
    commands: Vec<String>,
    notified_since_result: bool,
    activity: Option<(Instant, bool)>,
    /// `tool_use_id` of every denial already told in this turn: the turn's `result` lists them again.
    denied: HashSet<String>,
}

impl Stream {
    pub(crate) fn new(account: Option<String>, model: Option<String>) -> Self {
        Self {
            account,
            model,
            interrupted: false,
            open: false,
            error: None,
            status: None,
            rejected: None,
            credits: false,
            auth_reported: false,
            background: false,
            agent: false,
            commands: Vec::new(),
            notified_since_result: false,
            activity: None,
            denied: HashSet::new(),
        }
    }

    /// We sent `control_request{interrupt}`: the error result of the open turn is `TurnEnded{Cancelled}`, never a
    /// failure. With no turn open there is nothing to cancel, and a later turn's failure stays one.
    pub(crate) fn interrupted(&mut self) {
        self.interrupted = self.open;
    }

    /// The background shell commands of the session; a subagent is never among them (criterion 4a).
    pub(crate) fn commands(&self) -> Vec<String> {
        self.commands.clone()
    }

    /// One stdout line seen at `now`; the events it yields, in order. Unknown types yield nothing.
    pub(crate) fn line(&mut self, msg: &Value, now: Instant) -> Vec<Normalized> {
        let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
        let mut out = match (msg["type"].as_str(), msg["subtype"].as_str()) {
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
                    // A server that waits for a login is as gone as a failed one: nobody logs it in.
                    .filter(|s| s["status"] == "failed" || s["status"] == "needs-auth")
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
                    Some("running") => SessionState::Running,
                    _ => return vec![],
                };
                let background = self.background || self.notified_since_result;
                vec![Normalized::State { state, background }]
            }
            (Some("system"), Some("background_tasks_changed")) => {
                let tasks = msg["tasks"].as_array().map_or(&[][..], Vec::as_slice);
                self.background = !tasks.is_empty();
                self.agent = tasks.iter().any(|t| t["task_type"] == "local_agent");
                let commands = tasks.iter().filter(|t| t["task_type"] == "local_bash");
                self.commands = commands
                    .filter_map(|t| t["task_id"].as_str().map(str::to_owned))
                    .collect();
                vec![]
            }
            // A task the adapter stopped wakes no one: no turn follows it (experiment 8).
            (Some("system"), Some("task_notification")) => {
                self.notified_since_result |= msg["status"] != "stopped";
                vec![]
            }
            (Some("system"), Some("api_retry")) => self.retry(msg),
            (Some("system"), Some("permission_denied")) => {
                self.denial(msg, &msg["decision_reason_type"]).into_iter().collect()
            }
            (Some("rate_limit_event"), _) => self.rate_limit(&msg["rate_limit_info"]),
            (Some("result"), _) => self.result(msg),
            (Some("assistant"), _) => {
                if let Some(error) = msg["error"].as_str() {
                    self.error = Some(error.to_owned());
                    self.status = msg["api_error_status"].as_u64();
                }
                let content = msg["message"]["content"].as_array().into_iter().flatten();
                let tool = content.clone().any(|c| c["type"] == "tool_use");
                self.activity(now, tool)
            }
            (Some("tool_progress"), _) => self.activity(now, true),
            (Some("user" | "stream_event"), _) | (Some("system"), Some("status")) => self.activity(now, false),
            _ => vec![],
        };
        // Out of a turn, a background subagent's retry, rate limit or denial re-arms the core's silence clock: the
        // session is still idle with background, and its budget is the background one.
        if !self.open && self.agent && !out.is_empty() && !matches!(out.last(), Some(Normalized::State { .. })) {
            out.push(Normalized::State {
                state: SessionState::Idle,
                background: true,
            });
        }
        out
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
        let denials = msg["permission_denials"].as_array().into_iter().flatten();
        out.extend(denials.filter_map(|d| self.denial(d, &Value::Null)));
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
        // The state stays `running` after the result while a background subagent lives (docs/design/experiments.md,
        // "A background subagent after the result"): the session is idle with background from here.
        if self.agent {
            out.push(Normalized::State {
                state: SessionState::Idle,
                background: true,
            });
        }
        self.open = false;
        self.error = None;
        self.status = None;
        self.rejected = None;
        self.credits = false;
        self.auth_reported = false;
        self.notified_since_result = false;
        self.denied.clear();
        out
    }

    /// One denial of `system/permission_denied` or of `result.permission_denials`, unless its `tool_use_id` was told
    /// already in this turn. A denial without a `tool_use_id` (not seen from claude) is told as it is.
    fn denial(&mut self, msg: &Value, reason: &Value) -> Option<Normalized> {
        let tool_use_id = msg["tool_use_id"].as_str().map(str::to_owned);
        if let Some(id) = &tool_use_id
            && !self.denied.insert(id.clone())
        {
            return None;
        }
        Some(Normalized::PermissionDenied {
            tool: msg["tool_name"].as_str().unwrap_or_default().to_owned(),
            tool_use_id,
            input: msg["tool_input"].clone(),
            reason: reason.as_str().map(str::to_owned),
        })
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
        let auth = matches!(msg["error_status"].as_u64(), Some(401 | 403))
            || matches!(msg["error"].as_str(), Some(e) if classify::AUTH.contains(&e));
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
        self.credits |= info["errorCode"] == "credits_required";
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

    /// At most one activity a second, but a change of `tool_running` always goes through. Out of a turn it is a
    /// background subagent's step, which is no news: its silence is the background budget's.
    fn activity(&mut self, now: Instant, tool_running: bool) -> Vec<Normalized> {
        let quiet = !self.open
            || self
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

mod classify;
#[cfg(test)]
mod tests;
