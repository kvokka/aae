//! The kind of a failed turn from the structured fields claude reports (docs/design/contracts.md 4.4, 4.6): the
//! first field that names a kind decides, in the order `startup_failure_reason` → `subtype` and `terminal_reason` →
//! the assistant's `error` → `api_error_status` → a narrow text rule; what none of them names is `unknown`.
//! The values are those of `@anthropic-ai/claude-agent-sdk` 0.3.326 (`sdk.d.ts`, claude 2.1.326).

use super::Stream;
use aae_contract::driver::{Failure, FailureKind};
use serde_json::Value;

/// The `error` values of an assistant message or `api_retry` that mean the account is not logged in.
pub(super) const AUTH: &[&str] = &[
    "authentication_failed",
    "oauth_org_not_allowed",
    "cloud_credential_error",
];

impl Stream {
    pub(super) fn classify(&self, msg: &Value) -> Failure {
        let startup = msg["startup_failure_reason"].as_str();
        let found = startup
            .map(startup_kind)
            .or_else(|| ended(msg["subtype"].as_str(), msg["terminal_reason"].as_str()))
            .or_else(|| self.error.as_deref().and_then(error_kind))
            .or_else(|| status_kind(msg["api_error_status"].as_u64().or(self.status)?))
            .or_else(|| {
                text(msg)
                    .starts_with("Not logged in")
                    .then_some(FailureKind::AuthRequired)
            });
        let mut failure = self.failure(found.unwrap_or(FailureKind::Unknown), msg.to_string());
        match failure.kind {
            // A rate limit of this turn tells what kind of limit it was.
            FailureKind::RateLimited if self.credits => failure.kind = FailureKind::Billing,
            FailureKind::RateLimited if self.rejected.is_some() || text(msg).starts_with("You've hit your") => {
                failure.kind = FailureKind::QuotaExhausted;
                failure.retry_at_ms = self.rejected;
            }
            FailureKind::Unknown => {
                let values = [
                    ("startup_failure_reason", startup),
                    ("terminal_reason", msg["terminal_reason"].as_str()),
                    ("error", self.error.as_deref()),
                ];
                let value = values.iter().find_map(|(k, v)| v.map(|v| format!("{k}={v}")));
                failure.text = Some(Some(text(msg)).filter(|t| !t.is_empty()).or(value).unwrap_or_default());
            }
            _ => {}
        }
        // claude retries 429 and 5xx itself before it gives up (so `retried_by_harness`).
        failure.retried_by_harness = matches!(failure.kind, FailureKind::RateLimited | FailureKind::Overloaded);
        failure
    }
}

/// The words of a failed result: its `result`, else its `errors` (an `error_*` subtype carries no `result`).
fn text(msg: &Value) -> String {
    match msg["result"].as_str() {
        Some(result) => result.to_owned(),
        None => {
            let errors = msg["errors"].as_array().into_iter().flatten();
            errors.filter_map(Value::as_str).collect::<Vec<_>>().join("; ")
        }
    }
}

/// A claude that could not start (with `CLAUDE_CODE_STARTUP_FAILURE_RESULTS=1`); what may pass by itself is `unknown`.
fn startup_kind(reason: &str) -> FailureKind {
    match reason {
        "gateway_signin_required" | "gateway_access_denied" => FailureKind::AuthRequired,
        "session_held_by_background" => FailureKind::TranscriptDivergence,
        "org_pin_api_key_conflict"
        | "provider_not_allowed"
        | "org_pin_mismatch"
        | "managed_settings_invalid"
        | "proxy_invalid"
        | "temp_dir_unusable"
        | "cwd_unavailable"
        | "shell_tool_missing"
        | "worktree_resume_refused"
        | "worktree_unverified"
        | "cli_version_too_old"
        | "bypass_root"
        | "org_config_refused"
        | "sandbox_required_unavailable"
        | "initialize_not_first"
        | "config_file_invalid" => FailureKind::ConfigError,
        _ => FailureKind::Unknown,
    }
}

fn ended(subtype: Option<&str>, terminal_reason: Option<&str>) -> Option<FailureKind> {
    match (subtype, terminal_reason) {
        (Some("error_max_turns" | "error_max_budget_usd" | "error_max_structured_output_retries"), _)
        | (_, Some("max_turns" | "budget_exhausted" | "structured_output_retry_exhausted")) => {
            Some(FailureKind::RunawayTurn)
        }
        (_, Some("prompt_too_long" | "blocking_limit")) => Some(FailureKind::ContextOverflow),
        _ => None,
    }
}

fn error_kind(error: &str) -> Option<FailureKind> {
    Some(match error {
        e if AUTH.contains(&e) || e == "verification_required" => FailureKind::AuthRequired,
        "billing_error" | "account_on_hold" => FailureKind::Billing,
        "rate_limit" => FailureKind::RateLimited,
        "overloaded" | "server_error" => FailureKind::Overloaded,
        "model_not_found" => FailureKind::ConfigError,
        _ => return None,
    })
}

fn status_kind(status: u64) -> Option<FailureKind> {
    Some(match status {
        401 | 403 => FailureKind::AuthRequired,
        429 => FailureKind::RateLimited,
        404 => FailureKind::ConfigError,
        408 | 500..=504 | 529 => FailureKind::Overloaded,
        _ => return None,
    })
}
