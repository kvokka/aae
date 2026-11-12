use super::*;
use serde_json::json;

const KEY: &str = "26c22b42-962d-4bc6-9c9f-f75d7f34b294";

fn feed(stream: &mut Stream, lines: &[Value]) -> Vec<Normalized> {
    let now = Instant::now();
    lines.iter().flat_map(|l| stream.line(l, now)).collect()
}

fn stream() -> Stream {
    Stream::new(Some("acct".into()), Some("claude-haiku-5-5".into()))
}

fn result(is_error: bool, extra: Value) -> Value {
    let mut msg = json!({ "type": "result", "subtype": "success", "is_error": is_error,
        "usage": { "input_tokens": 3, "output_tokens": 2 }, "result": "OK" });
    msg.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    msg
}

fn failure(events: &[Normalized]) -> &Failure {
    events
        .iter()
        .find_map(|e| match e {
            Normalized::Failure(f) => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no failure in {events:?}"))
}

fn state(name: &str) -> Value {
    json!({ "type": "system", "subtype": "session_state_changed", "state": name })
}

#[test]
fn a_reply_is_acked_started_and_completed() {
    let mut s = stream();
    let events = feed(
        &mut s,
        &[
            json!({ "type": "command_lifecycle", "command_uuid": KEY, "state": "queued" }),
            json!({ "type": "command_lifecycle", "command_uuid": KEY, "state": "started" }),
            json!({ "type": "system", "subtype": "init", "mcp_servers": [{ "name": "aae", "status": "connected" }] }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "OK" }] } }),
            result(false, json!({})),
            state("idle"),
        ],
    );
    let key = PromptKey(Uuid::parse_str(KEY).unwrap());
    assert_eq!(
        events,
        vec![
            Normalized::PromptAcked { key },
            Normalized::TurnStarted,
            Normalized::Activity { tool_running: false },
            Normalized::Usage {
                input_tokens: 3,
                output_tokens: 2,
                rate_limit: None
            },
            Normalized::TurnEnded {
                stop_reason: StopReason::Completed
            },
            Normalized::State {
                state: SessionState::Idle,
                background: false
            },
        ]
    );
    assert_eq!(
        s.exited(Some(0), None),
        vec![Normalized::Exited {
            code: Some(0),
            signal: None
        }]
    );
}

#[test]
fn a_permission_request_and_an_interrupted_turn() {
    let mut s = stream();
    let request = json!({ "type": "control_request", "request_id": "r1", "request": { "subtype": "can_use_tool",
        "tool_name": "Bash", "input": { "command": "touch x" }, "description": "touch x" } });
    let events = feed(&mut s, &[request]);
    assert!(
        matches!(&events[..], [Normalized::PermissionRequested { request_id, kind: RequestKind::Permission, tool, text, .. }]
        if request_id == "r1" && tool == "Bash" && text == "touch x")
    );
    s.interrupted();
    let aborted = json!({ "subtype": "error_during_execution", "terminal_reason": "aborted_tools" });
    let events = feed(&mut s, &[result(true, aborted)]);
    assert_eq!(
        events.last(),
        Some(&Normalized::TurnEnded {
            stop_reason: StopReason::Cancelled
        })
    );
}

#[test]
fn failed_turns_are_classified() {
    let assistant =
        |error: &str, status: Value| json!({ "type": "assistant", "error": error, "api_error_status": status });
    let api =
        |status: u64, reason: &str| result(true, json!({ "api_error_status": status, "terminal_reason": reason }));
    let cases = [
        (
            vec![assistant("rate_limit", json!(429)), api(429, "api_error")],
            FailureKind::RateLimited,
        ),
        (
            vec![assistant("server_error", json!(529)), api(529, "api_error")],
            FailureKind::Overloaded,
        ),
        (
            vec![assistant("server_error", json!(500)), api(500, "api_error")],
            FailureKind::Overloaded,
        ),
        (
            vec![assistant("invalid_request", json!(400)), api(400, "prompt_too_long")],
            FailureKind::ContextOverflow,
        ),
        (
            vec![assistant("billing_error", json!(400)), api(400, "api_error")],
            FailureKind::Billing,
        ),
        (
            vec![assistant("model_not_found", json!(404)), api(404, "api_error")],
            FailureKind::ConfigError,
        ),
        (
            vec![result(true, json!({ "result": "Not logged in · Please run /login" }))],
            FailureKind::AuthRequired,
        ),
        (
            vec![result(true, json!({ "subtype": "error_max_budget_usd" }))],
            FailureKind::RunawayTurn,
        ),
        (
            vec![
                assistant("rate_limit", json!(429)),
                result(true, json!({ "result": "You've hit your limit · resets 6am" })),
            ],
            FailureKind::QuotaExhausted,
        ),
        (
            vec![result(true, json!({ "result": "something new" }))],
            FailureKind::Unknown,
        ),
    ];
    for (lines, kind) in cases {
        let events = feed(&mut stream(), &lines);
        let f = failure(&events);
        assert_eq!(f.kind, kind, "{lines:?}");
        assert_eq!(f.account.as_deref(), Some("acct"));
        assert_eq!(
            f.retried_by_harness,
            matches!(kind, FailureKind::RateLimited | FailureKind::Overloaded)
        );
        assert_eq!(
            events.last(),
            Some(&Normalized::TurnEnded {
                stop_reason: StopReason::Failed
            })
        );
    }
    let events = feed(&mut stream(), &[result(true, json!({ "result": "something new" }))]);
    assert_eq!(failure(&events).text.as_deref(), Some("something new"));
}

#[test]
fn a_rejected_limit_is_a_quota_with_its_reset_and_a_401_fails_at_once() {
    let limit = json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected", "resetsAt": 1_794_286_800,
        "rateLimitType": "five_hour", "unifiedWindows": { "five_hour": { "utilization": 1.0 } } } });
    let events = feed(
        &mut stream(),
        &[limit, result(true, json!({ "api_error_status": 429 }))],
    );
    assert!(
        matches!(events[0], Normalized::Usage { rate_limit: Some(RateLimit { utilization, .. }), .. } if utilization == 1.0)
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

#[test]
fn idle_counts_only_without_background_work_and_late_notifications() {
    let mut s = stream();
    let tasks =
        |n: usize| json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": vec![json!({}); n] });
    let notified = json!({ "type": "system", "subtype": "task_notification", "status": "completed" });
    let idle = |background| Normalized::State {
        state: SessionState::Idle,
        background,
    };
    assert_eq!(
        feed(&mut s, &[tasks(1), result(false, json!({})), state("idle")]).last(),
        Some(&idle(true))
    );
    // The task list empties and a notification came after the result: the harness is about to wake itself.
    assert_eq!(
        feed(&mut s, &[notified, tasks(0), state("idle")]).last(),
        Some(&idle(true))
    );
    assert_eq!(
        feed(&mut s, &[result(false, json!({})), state("idle")]).last(),
        Some(&idle(false))
    );
}

#[test]
fn activity_is_coalesced_to_one_a_second() {
    let mut s = stream();
    let start = Instant::now();
    let tool = json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use" }] } });
    let progress = json!({ "type": "tool_progress" });
    let user = json!({ "type": "user" });
    let mut seen = Vec::new();
    for (ms, line) in [
        (0, &tool),
        (100, &progress),
        (1200, &progress),
        (1300, &user),
        (1400, &user),
    ] {
        seen.extend(s.line(line, start + Duration::from_millis(ms)));
    }
    let running: Vec<_> = seen
        .iter()
        .map(|e| matches!(e, Normalized::Activity { tool_running: true }))
        .collect();
    assert_eq!(running, [true, true, false]);
}

#[test]
fn an_exit_in_an_open_turn_is_a_crash_even_with_code_zero() {
    let mut s = stream();
    feed(
        &mut s,
        &[json!({ "type": "command_lifecycle", "command_uuid": KEY, "state": "queued" })],
    );
    let events = s.exited(Some(0), None);
    assert_eq!(failure(&events).kind, FailureKind::HarnessCrash);
    assert_eq!(
        events.last(),
        Some(&Normalized::Exited {
            code: Some(0),
            signal: None
        })
    );
    let mut s = stream();
    let events = feed(
        &mut s,
        &[json!({ "type": "system", "subtype": "init", "mcp_servers": [{ "name": "x", "status": "failed" }] })],
    );
    assert_eq!(failure(&events).kind, FailureKind::ToolFailure);
}
