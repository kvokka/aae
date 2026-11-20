//! The kind of a failed turn (`classify`): one row for every value of every field the classification reads.

use super::*;

fn assistant(error: &str) -> Value {
    json!({ "type": "assistant", "error": error })
}

fn api(status: u64) -> Value {
    result(
        true,
        json!({ "api_error_status": status, "terminal_reason": "api_error" }),
    )
}

fn ended(subtype: &str, terminal_reason: &str) -> Value {
    json!({ "type": "result", "subtype": subtype, "is_error": true, "terminal_reason": terminal_reason,
        "errors": [] })
}

/// A claude that could not start, as 2.1.326 reports it with `CLAUDE_CODE_STARTUP_FAILURE_RESULTS=1`.
fn startup(reason: &str) -> Value {
    json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "num_turns": 0,
        "errors": ["EACCES: permission denied, mkdir '/nonexistent'"], "startup_failure_reason": reason })
}

fn limit(info: Value) -> Value {
    json!({ "type": "rate_limit_event", "rate_limit_info": info })
}

#[test]
fn failed_turns_are_classified() {
    use FailureKind::*;
    let mut cases: Vec<(Vec<Value>, FailureKind)> = Vec::new();
    let errors = [
        ("authentication_failed", AuthRequired),
        ("oauth_org_not_allowed", AuthRequired),
        ("cloud_credential_error", AuthRequired),
        ("verification_required", AuthRequired),
        ("billing_error", Billing),
        ("account_on_hold", Billing),
        ("rate_limit", RateLimited),
        ("overloaded", Overloaded),
        ("server_error", Overloaded),
        ("model_not_found", ConfigError),
        ("invalid_request", Unknown),
        ("max_output_tokens", Unknown),
        ("unknown", Unknown),
        ("an_error_of_tomorrow", Unknown),
    ];
    for (error, kind) in errors {
        cases.push((
            vec![
                assistant(error),
                result(true, json!({ "terminal_reason": "api_error" })),
            ],
            kind,
        ));
    }
    let statuses = [
        (401, AuthRequired),
        (403, AuthRequired),
        (429, RateLimited),
        (404, ConfigError),
        (408, Overloaded),
        (500, Overloaded),
        (502, Overloaded),
        (503, Overloaded),
        (504, Overloaded),
        (529, Overloaded),
        (400, Unknown),
    ];
    cases.extend(statuses.map(|(status, kind)| (vec![api(status)], kind)));
    // The status of the assistant message counts when the result has none.
    cases.push((
        vec![
            json!({ "type": "assistant", "error": "unknown", "api_error_status": 529 }),
            api_less(),
        ],
        Overloaded,
    ));
    let ends = [
        ("error_max_turns", "max_turns", RunawayTurn),
        ("error_max_budget_usd", "budget_exhausted", RunawayTurn),
        (
            "error_max_structured_output_retries",
            "structured_output_retry_exhausted",
            RunawayTurn,
        ),
        ("success", "max_turns", RunawayTurn),
        ("success", "budget_exhausted", RunawayTurn),
        ("success", "structured_output_retry_exhausted", RunawayTurn),
        ("success", "prompt_too_long", ContextOverflow),
        ("success", "blocking_limit", ContextOverflow),
        ("error_during_execution", "model_error", Unknown),
        ("error_during_execution", "a_reason_of_tomorrow", Unknown),
    ];
    cases.extend(ends.map(|(subtype, reason, kind)| (vec![ended(subtype, reason)], kind)));
    let rejected = limit(json!({ "status": "rejected", "resetsAt": 1_794_286_800 }));
    let credits = limit(json!({ "status": "allowed_warning", "errorCode": "credits_required" }));
    cases.extend([
        // The narrow text rules: a logout without a field, and a subscription quota worded only in its result.
        (
            vec![result(true, json!({ "result": "Not logged in · Please run /login" }))],
            AuthRequired,
        ),
        (
            vec![
                assistant("rate_limit"),
                result(true, json!({ "result": "You've hit your limit · resets 6am" })),
            ],
            QuotaExhausted,
        ),
        // C1: the words of the result text are never fields.
        (
            vec![result(
                true,
                json!({ "result": "rate_limit 429 billing_error credit balance overloaded" }),
            )],
            Unknown,
        ),
        // C2: a rejected limit makes a quota only of a rate-limited turn.
        (vec![rejected.clone(), assistant("server_error"), api(529)], Overloaded),
        (vec![rejected, assistant("rate_limit"), api(429)], QuotaExhausted),
        // C3: `credits_required` outlives the assistant message that follows it.
        (vec![credits, assistant("rate_limit"), api(429)], Billing),
    ]);
    for (lines, kind) in cases {
        assert_failure(&lines, kind);
    }
}

#[test]
fn startup_failures_are_classified_by_reason() {
    use FailureKind::*;
    // C8: a claude that cannot start is told by its reason; what may pass by itself is unknown.
    let reasons = [
        ("gateway_signin_required", AuthRequired),
        ("gateway_access_denied", AuthRequired),
        ("session_held_by_background", TranscriptDivergence),
        ("org_pin_api_key_conflict", ConfigError),
        ("provider_not_allowed", ConfigError),
        ("org_pin_mismatch", ConfigError),
        ("managed_settings_invalid", ConfigError),
        ("proxy_invalid", ConfigError),
        ("temp_dir_unusable", ConfigError),
        ("cwd_unavailable", ConfigError),
        ("shell_tool_missing", ConfigError),
        ("worktree_resume_refused", ConfigError),
        ("worktree_unverified", ConfigError),
        ("cli_version_too_old", ConfigError),
        ("bypass_root", ConfigError),
        ("org_config_refused", ConfigError),
        ("sandbox_required_unavailable", ConfigError),
        ("initialize_not_first", ConfigError),
        ("config_file_invalid", ConfigError),
        ("org_verify_failed", Unknown),
        ("remote_settings_required_unavailable", Unknown),
        ("org_config_required_unavailable", Unknown),
        ("managed_settings_not_delivered", Unknown),
        ("a_reason_of_tomorrow", Unknown),
    ];
    for (reason, kind) in reasons {
        assert_failure(&[startup(reason)], kind);
    }
}

fn assert_failure(lines: &[Value], kind: FailureKind) {
    use FailureKind::*;
    let events = feed(&mut stream(), lines);
    let f = failure(&events);
    assert_eq!(f.kind, kind, "{lines:?}");
    assert_eq!(f.account.as_deref(), Some("acct"));
    assert_eq!(
        f.retried_by_harness,
        matches!(kind, RateLimited | Overloaded),
        "{lines:?}"
    );
    assert_eq!(f.text.is_some(), kind == Unknown, "{lines:?}");
    assert_eq!(
        events.last(),
        Some(&Normalized::TurnEnded {
            stop_reason: StopReason::Failed
        })
    );
}

/// A failed result with no field that names a kind.
fn api_less() -> Value {
    result(true, json!({ "terminal_reason": "api_error" }))
}

#[test]
fn an_unknown_failure_keeps_the_words_of_claude_or_the_value() {
    let text = |lines: &[Value]| failure(&feed(&mut stream(), lines)).text.clone();
    assert_eq!(
        text(&[result(true, json!({ "result": "something new" }))]).as_deref(),
        Some("something new")
    );
    // C4: an `error_during_execution` carries its words in `errors`.
    let during = json!({ "type": "result", "subtype": "error_during_execution", "is_error": true,
        "errors": ["boom", "and again"] });
    assert_eq!(text(&[during]).as_deref(), Some("boom; and again"));
    assert_eq!(
        text(&[ended("error_during_execution", "a_reason_of_tomorrow")]).as_deref(),
        Some("terminal_reason=a_reason_of_tomorrow")
    );
    let future = json!({ "type": "result", "subtype": "error_of_tomorrow", "is_error": true, "errors": [] });
    assert_eq!(text(&[future]).as_deref(), Some("subtype=error_of_tomorrow"));
    let status = json!({ "type": "result", "is_error": true, "api_error_status": 418 });
    assert_eq!(text(&[status]).as_deref(), Some("api_error_status=418"));
    let quiet = json!({ "type": "result", "subtype": "error_during_execution", "is_error": true,
        "startup_failure_reason": "a_reason_of_tomorrow" });
    assert_eq!(
        text(&[quiet]).as_deref(),
        Some("startup_failure_reason=a_reason_of_tomorrow")
    );
}

#[test]
fn a_rejected_limit_is_a_quota_with_its_reset_and_a_401_fails_at_once() {
    let limit = json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected",
        "resetsAt": 1_794_286_800,
        "rateLimitType": "five_hour", "unifiedWindows": { "five_hour": { "utilization": 1.0 } } } });
    let events = feed(
        &mut stream(),
        &[limit, result(true, json!({ "api_error_status": 429 }))],
    );
    assert!(
        matches!(events[0], Normalized::Usage { rate_limit: Some(RateLimit { utilization, .. }), ..
            } if utilization == 1.0)
    );
    let f = failure(&events);
    assert_eq!(
        (f.kind, f.retry_at_ms),
        (FailureKind::QuotaExhausted, Some(1_794_286_800_000))
    );

    let retry = json!({ "type": "system", "subtype": "api_retry", "attempt": 1, "retry_delay_ms": 567,
        "error_status": 401, "error": "authentication_failed" });
    let events = feed(&mut stream(), &[retry.clone(), retry]);
    let failures = events.iter().filter(|e| matches!(e, Normalized::Failure(_))).count();
    assert_eq!(failures, 1, "{events:?}");
    assert_eq!(failure(&events).kind, FailureKind::AuthRequired);
}

/// claude retries a logged-out account for about a minute; the first `api_retry` that says so fails the turn.
#[test]
fn a_logged_out_retry_fails_at_once_and_an_overload_does_not() {
    let retry = |status: Value, error: &str| {
        json!({ "type": "system", "subtype": "api_retry", "attempt": 1, "retry_delay_ms": 567,
            "error_status": status, "error": error })
    };
    let auth = [
        retry(json!(403), "unknown"),
        retry(Value::Null, "oauth_org_not_allowed"),
        retry(Value::Null, "cloud_credential_error"),
    ];
    for line in auth {
        let events = feed(&mut stream(), &[line.clone(), line.clone()]);
        let failures: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Normalized::Failure(f) => Some(f.kind),
                _ => None,
            })
            .collect();
        assert_eq!(failures, [FailureKind::AuthRequired], "{line}");
    }
    let events = feed(&mut stream(), &[retry(json!(529), "overloaded")]);
    assert!(
        !events.iter().any(|e| matches!(e, Normalized::Failure(_))),
        "{events:?}"
    );
}
