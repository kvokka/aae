use super::{check, run};
use serde_json::{Value, json};

mod worktree;

/// `run.attempt_failed` at 12:00:00 for a failure of `kind`, after `attempts` of `(kind, failure_kind, seconds)`.
fn failed(kind: &str, failure: Value, attempts: &[(&str, &str, i64)], gates: Value) -> Value {
    let attempts: Vec<Value> = attempts
        .iter()
        .enumerate()
        .map(|(n, (k, f, secs))| {
            json!({ "n": n + 1, "kind": k, "failure_kind": f, "started_at": "2026-11-13T11:00:00.000Z",
                "ended_at": format!("2026-11-13T11:{:02}:{:02}.000Z", secs / 60, secs % 60) })
        })
        .collect();
    let mut failure = failure;
    failure["kind"] = json!(kind);
    json!({ "type": "run.attempt_failed", "ts": "2026-11-13T12:00:00.000Z", "data": {
        "run": { "id": "r1" }, "failure": failure, "attempts": attempts, "gates": gates,
        "keys": ["orchestrator", "project:shop", "rig:shop/web", "agent:shop.dev",
            "account:claude/a1", "model:claude/haiku"],
    } })
}

fn policy(event: &Value, params: Value) -> Value {
    run("default-policy", event, &params).unwrap().unwrap()
}

#[test]
fn default_policy_follows_contracts_10() {
    let none = json!({});
    let crash = |n| vec![("resume", "harness_crash", 600); n];
    let cases = [
        (
            failed("harness_crash", none.clone(), &crash(1), json!([])),
            json!({"decision": "resume", "after": "0s"}),
        ),
        (
            failed("harness_crash", none.clone(), &crash(3), json!([])),
            json!({"decision": "new_session"}),
        ),
        (
            failed(
                "harness_crash",
                none.clone(),
                &[("new_session", "harness_crash", 5); 5],
                json!([]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "agent:shop.dev",
                "until": "2026-11-13T12:05:00.000Z", "reason": "harness_crash"}}),
        ),
        (
            failed(
                "quota_exhausted",
                json!({"retry_at": "2026-11-13T15:00:00.000Z"}),
                &[],
                json!([]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "account:claude/a1",
                "until": "2026-11-13T15:01:00.000Z", "reason": "quota_exhausted"}}),
        ),
        (
            failed(
                "rate_limited",
                none.clone(),
                &[("resume", "rate_limited", 1); 2],
                json!([]),
            ),
            json!({"decision": "resume", "after": "20s"}),
        ),
        (
            failed("overloaded", json!({"retried_by_harness": true}), &[], json!([])),
            json!({"decision": "park", "then": "resume", "gate": {"key": "model:claude/haiku",
                "until": "2026-11-13T12:10:00.000Z", "reason": "overloaded"}}),
        ),
        (
            failed("config_error", none.clone(), &[], json!([])),
            json!({"decision": "fail", "reason": "config_error", "gate": {"key": "agent:shop.dev", "until": null,
                "reason": "config_error"}}),
        ),
        (
            failed("stall", none.clone(), &[("resume", "stall", 9); 2], json!([])),
            json!({"decision": "new_session"}),
        ),
        (
            failed("unknown", none.clone(), &[("resume", "unknown", 9); 4], json!([])),
            json!({"decision": "fail", "reason": "unknown"}),
        ),
        // Decisions 49, 50: an auth failure may pass (a proxy or WAF error), so a wait on the account, longer with
        // every failure in a row.
        (
            failed(
                "auth_required",
                none.clone(),
                &[("resume", "auth_required", 9); 3],
                json!([]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "account:claude/a1",
                "until": "2026-11-13T12:02:00.000Z", "reason": "auth_required"}}),
        ),
        (
            failed("billing", none.clone(), &[("resume", "billing", 9)], json!([])),
            json!({"decision": "park", "then": "resume", "gate": {"key": "account:claude/a1",
                "until": "2026-11-13T12:00:30.000Z", "reason": "billing"}}),
        ),
        // A gate that closed while the attempt ran answers its failure: park on it, no new closure.
        (
            failed(
                "rate_limited",
                none.clone(),
                &[("resume", "rate_limited", 9)],
                json!([{"key": "model:claude/haiku", "until": "2026-11-13T12:30:00.000Z", "reason": "x",
                    "closed_at": "2026-11-13T11:00:05.000Z"}]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "model:claude/haiku",
                "until": "2026-11-13T12:30:00.000Z", "reason": "x"}}),
        ),
    ];
    for (event, want) in cases {
        assert_eq!(policy(&event, json!({})), want, "{event}");
    }
    // Thresholds come from the config.
    let event = failed("unknown", none, &[("resume", "unknown", 9)], json!([]));
    assert_eq!(policy(&event, json!({"unknown_after_s": 1}))["after"], "1s");
    assert!(check("default-policy", &json!({"no_such": 1})).is_err());
}

/// Every threshold of `[builtin.default-policy]` moves its decision away from the default one.
#[test]
fn every_policy_threshold_comes_from_the_config() {
    let none = json!({});
    let until = |v: &Value| v["gate"]["until"].clone();
    let quota = |retry: Value| failed("quota_exhausted", retry, &[], json!([]));
    let reset = quota(json!({"retry_at": "2026-11-13T15:00:00.000Z"}));
    assert_eq!(
        until(&policy(&reset, json!({"quota_margin_s": 0}))),
        "2026-11-13T15:00:00.000Z"
    );
    let no_reset = quota(none.clone());
    assert_eq!(
        until(&policy(&no_reset, json!({"quota_wait_s": 60}))),
        "2026-11-13T12:01:00.000Z"
    );
    // Without a reset time of its own, the account's last known limit gives it, once used up and still ahead.
    let limit = |utilization: f64, resets_at: &str| {
        let mut event = quota(none.clone());
        event["data"]["rate_limit"] = json!({ "utilization": utilization, "resets_at": resets_at });
        event
    };
    let margin = json!({"quota_margin_s": 0, "quota_wait_s": 60});
    let known = limit(1.0, "2026-11-13T14:00:00.000Z");
    assert_eq!(until(&policy(&known, margin.clone())), "2026-11-13T14:00:00.000Z");
    for not_used_up in [
        limit(0.4, "2026-11-13T14:00:00.000Z"),
        limit(1.0, "2026-11-13T11:00:00.000Z"),
    ] {
        assert_eq!(until(&policy(&not_used_up, margin.clone())), "2026-11-13T12:01:00.000Z");
    }
    let mut limited = known.clone();
    limited["data"]["failure"]["kind"] = json!("rate_limited");
    let parked = policy(&limited, margin);
    assert_eq!(
        (&parked["gate"]["key"], until(&parked)),
        (&json!("account:claude/a1"), json!("2026-11-13T14:00:00.000Z"))
    );
    let overloaded = failed("overloaded", json!({"retried_by_harness": true}), &[], json!([]));
    assert_eq!(
        until(&policy(&overloaded, json!({"model_pause_s": 60}))),
        "2026-11-13T12:01:00.000Z"
    );

    let limited = failed(
        "rate_limited",
        none.clone(),
        &[("resume", "rate_limited", 1); 2],
        json!([]),
    );
    assert_eq!(policy(&limited, json!({"backoff_base_s": 1}))["after"], "2s");
    assert_eq!(policy(&limited, json!({"backoff_cap_s": 5}))["after"], "5s");
    assert_eq!(policy(&limited, json!({"backoff_park_from": 2}))["decision"], "park");

    let rapid = failed(
        "harness_crash",
        none.clone(),
        &[("new_session", "harness_crash", 5); 2],
        json!([]),
    );
    assert_eq!(policy(&rapid, json!({}))["decision"], "resume");
    let looped = policy(&rapid, json!({"crash_loop": 2, "crash_pause_s": 60}));
    assert_eq!(until(&looped), "2026-11-13T12:01:00.000Z");
    assert_eq!(
        policy(&rapid, json!({"crash_loop": 2, "rapid_s": 5}))["decision"],
        "resume"
    );
    let resumed = failed(
        "harness_crash",
        none.clone(),
        &[("resume", "harness_crash", 600)],
        json!([]),
    );
    assert_eq!(
        policy(&resumed, json!({"resume_failures": 1}))["decision"],
        "new_session"
    );

    // The streak of `auth_required` began an hour ago, when its first attempt started.
    let auth = failed(
        "auth_required",
        none.clone(),
        &[("resume", "auth_required", 9); 2],
        json!([]),
    );
    assert_eq!(until(&policy(&auth, json!({}))), "2026-11-13T12:01:00.000Z");
    let based = policy(&auth, json!({"auth_backoff_base_s": 1}));
    assert_eq!(until(&based), "2026-11-13T12:00:02.000Z");
    let capped = policy(&auth, json!({"auth_backoff_cap_s": 5}));
    assert_eq!(until(&capped), "2026-11-13T12:00:05.000Z");
    assert!(check("default-policy", &json!({"auth_escalate_s": 3600})).is_err());
    for (key, value, error) in [
        ("crash_loop", 0, "`crash_loop` must be at least 1"),
        ("auth_backoff_base_s", 0, "`auth_backoff_base_s` must be at least 1"),
        ("backoff_cap_s", -5, "`backoff_cap_s` must be at least 1"),
        ("rapid_s", -1, "`rapid_s` must not be negative"),
    ] {
        let refused = check("default-policy", &json!({ key: value })).unwrap_err();
        assert_eq!(refused, format!("[builtin.default-policy]: {error}"));
    }
    assert!(check("default-policy", &json!({"rapid_s": 0, "unknown_retries": 0})).is_ok());
    // However long it lasts, the wait grows to an hour and never goes to an operator.
    let long = failed(
        "auth_required",
        none.clone(),
        &[("resume", "auth_required", 9); 40],
        json!([]),
    );
    assert_eq!(until(&policy(&long, json!({}))), "2026-11-13T13:00:00.000Z");

    let unknown = failed("unknown", none, &[("resume", "unknown", 9); 2], json!([]));
    assert_eq!(policy(&unknown, json!({}))["decision"], "resume");
    assert_eq!(policy(&unknown, json!({"unknown_retries": 1}))["decision"], "fail");
}

#[test]
fn default_policy_reminds_once() {
    let turn = |remarks| json!({ "type": "run.turn_ended_without_outcome", "data": { "remarks": remarks } });
    assert_eq!(policy(&turn(0), json!({}))["decision"], "continue");
    assert_eq!(policy(&turn(1), json!({}))["decision"], "fail");
    let other = json!({ "type": "run.finishing", "data": {} });
    assert_eq!(run("default-policy", &other, &json!({})), Ok(None));
}
