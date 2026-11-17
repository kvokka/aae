//! The kind of a turn's final error (`classify`): one row for every `codexErrorInfo` variant and status.

use super::*;

/// The failure of a turn that ends on `error` after the snapshots `limits`.
fn failure_of(error: Value, limits: &[Value]) -> Failure {
    let mut stream = Stream::new(None);
    let mut messages = vec![note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } }))];
    messages.extend(
        limits
            .iter()
            .map(|l| note("account/rateLimits/updated", json!({ "rateLimits": l }))),
    );
    messages.push(note(
        "error",
        json!({ "threadId": "t", "willRetry": false, "error": error }),
    ));
    messages.push(turn_completed("failed", error));
    let events = play(&mut stream, &messages);
    events
        .into_iter()
        .find_map(|e| match e {
            Normalized::Failure(f) => Some(f),
            _ => None,
        })
        .expect("a failure")
}

#[test]
fn every_final_error_has_its_kind() {
    use FailureKind::*;
    let status = |variant: &str, code: u64| json!({ variant: { "httpStatusCode": code } });
    let mut cases = vec![
        (json!("usageLimitExceeded"), QuotaExhausted, false),
        (json!("rateLimitExceeded"), RateLimited, false),
        (json!("serverOverloaded"), Overloaded, true),
        (json!("internalServerError"), Overloaded, true),
        // C7: capacity of a tier or of a policy is an overload too.
        (json!("flexUnavailable"), Overloaded, true),
        (json!("policyCapacityUnavailable"), Overloaded, true),
        (json!("unauthorized"), AuthRequired, false),
        (json!("contextWindowExceeded"), ContextOverflow, false),
        (json!("modelNotFound"), ConfigError, false),
        (json!("badRequest"), ConfigError, false),
        (json!("sandboxError"), ConfigError, false),
        (json!("sessionBudgetExceeded"), RunawayTurn, false),
        (json!("cyberPolicy"), Unknown, false),
        (json!("misalignmentPolicyViolation"), Unknown, false),
        (json!("tooManyDenials"), Unknown, false),
        (json!("threadRollbackFailed"), Unknown, false),
        (
            json!({ "activeTurnNotSteerable": { "turnKind": "review" } }),
            Unknown,
            false,
        ),
        (json!("other"), Unknown, false),
        (json!("aVariantOfTomorrow"), Unknown, false),
        (Value::Null, Unknown, false),
    ];
    let variants = [
        "httpConnectionFailed",
        "responseStreamConnectionFailed",
        "responseStreamDisconnected",
        "responseTooManyFailedAttempts",
    ];
    // C7: 403, 408 and 504 are kinds of their own, by the status and never by the words.
    let codes = [
        (401, AuthRequired, false),
        (403, AuthRequired, false),
        (429, RateLimited, false),
        (404, ConfigError, false),
        (408, Overloaded, true),
        (500, Overloaded, true),
        (502, Overloaded, true),
        (503, Overloaded, true),
        (504, Overloaded, true),
        (529, Overloaded, true),
        (400, Unknown, false),
    ];
    for variant in variants {
        cases.extend(codes.map(|(code, kind, retried)| (status(variant, code), kind, retried)));
        cases.push((json!({ variant: { "httpStatusCode": null } }), Unknown, false));
    }
    for (info, kind, retried) in cases {
        let f = failure_of(json!({ "message": "it failed", "codexErrorInfo": info }), &[]);
        assert_eq!((f.kind, f.retried_by_harness), (kind, retried), "{info}");
        assert_eq!(f.text.is_some(), kind == Unknown, "{info}");
    }
    let f = failure_of(json!({ "codexErrorInfo": "aVariantOfTomorrow" }), &[]);
    assert_eq!(f.text.as_deref(), Some("codexErrorInfo=\"aVariantOfTomorrow\""));
}

/// C5, C6: `usageLimitExceeded` is a quota to wait out only when nothing says it is a plan or credit matter.
#[test]
fn a_usage_limit_of_a_plan_or_credit_is_billing() {
    let limit = |message: &str| json!({ "message": message, "codexErrorInfo": "usageLimitExceeded" });
    let window = json!({ "primary": { "usedPercent": 100, "resetsAt": 4_000_000_000_i64 } });
    let snapshot = |field: &str, value: Value| {
        let mut s = window.clone();
        s[field] = value;
        s
    };
    let cases = [
        (
            limit("You’ve hit your usage limit."),
            vec![window.clone()],
            FailureKind::QuotaExhausted,
        ),
        (
            limit("You’ve hit your usage limit."),
            vec![snapshot("rateLimitReachedType", json!("rate_limit_reached"))],
            FailureKind::QuotaExhausted,
        ),
        (
            limit("…"),
            vec![snapshot(
                "rateLimitReachedType",
                json!("workspace_owner_credits_depleted"),
            )],
            FailureKind::Billing,
        ),
        (
            limit("…"),
            vec![snapshot(
                "rateLimitReachedType",
                json!("workspace_member_credits_depleted"),
            )],
            FailureKind::Billing,
        ),
        (
            limit("…"),
            vec![snapshot(
                "rateLimitReachedType",
                json!("workspace_owner_usage_limit_reached"),
            )],
            FailureKind::Billing,
        ),
        (
            limit("…"),
            vec![snapshot(
                "rateLimitReachedType",
                json!("workspace_member_usage_limit_reached"),
            )],
            FailureKind::Billing,
        ),
        (
            limit("…"),
            vec![snapshot("spendControlReached", json!(true))],
            FailureKind::Billing,
        ),
        // A later snapshot that no longer names the reached type is believed.
        (
            limit("You’ve hit your usage limit."),
            vec![snapshot("spendControlReached", json!(true)), window.clone()],
            FailureKind::QuotaExhausted,
        ),
        (
            limit("Quota exceeded. Check your plan and billing details."),
            vec![],
            FailureKind::Billing,
        ),
        (
            limit("To use Codex with your ChatGPT plan, upgrade to Plus: https://chatgpt.com/explore/plus."),
            vec![],
            FailureKind::Billing,
        ),
        (
            limit("Your workspace is out of credits. Add credits to continue."),
            vec![],
            FailureKind::Billing,
        ),
        (
            limit("You hit your spend cap set in your workspace. Increase your spend cap to continue."),
            vec![],
            FailureKind::Billing,
        ),
    ];
    for (error, limits, kind) in cases {
        let f = failure_of(error.clone(), &limits);
        assert_eq!(f.kind, kind, "{error} after {limits:?}");
        let retry_at = (kind == FailureKind::QuotaExhausted).then_some(4_000_000_000_000);
        assert_eq!(f.retry_at_ms, retry_at, "{error}");
    }
}

#[test]
fn a_retry_with_401_or_403_reports_the_logout_and_a_full_queue_is_an_overload() {
    for code in [401, 403] {
        let retry = note(
            "error",
            json!({ "threadId": "t", "willRetry": true, "error": { "message": "Reconnecting... 1/5",
            "codexErrorInfo": { "responseStreamDisconnected": { "httpStatusCode": code } } } }),
        );
        let events = play(&mut Stream::new(None), &[retry]);
        assert!(
            matches!(&events[1], Normalized::Failure(f) if f.kind == FailureKind::AuthRequired),
            "{events:?}"
        );
    }
    let full = rpc::rpc_failure(
        "turn/start",
        &json!({ "code": -32001, "message": "Server overloaded; retry later." }),
    );
    assert_eq!(full.kind, FailureKind::Overloaded);
}
