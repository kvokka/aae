use super::*;
use aae_core::driver::Normalized;
use aae_core::driver::{RateLimit, RequestKind, SessionState, StopReason};

pub(crate) fn agent<T>(value: T) -> aae_core::config::Sourced<T> {
    aae_core::config::Sourced {
        value,
        from: aae_core::config::Layer::Agent,
    }
}

pub(crate) fn config() -> EffectiveConfig {
    EffectiveConfig {
        services: Default::default(),
        harness: "codex".into(),
        model: Some(agent("gpt-fake".into())),
        effort: Some(agent("low".into())),
        system_prompt: None,
        permission_mode: Some(agent("never".into())),
        inherit_host: Some(agent(false)),
        env: [("OPENAI_API_KEY".to_owned(), agent("sk-fake".to_owned()))].into(),
        mcp: Default::default(),
        hooks: Default::default(),
        skills: Default::default(),
        plugins: Vec::new(),
        native: Some(agent(json!({ "sandbox_mode": "danger-full-access",
            "model_providers": { "fake": { "base_url": "http://127.0.0.1:1/v1", "wire_api": "responses" } } }))),
    }
}

/// The events of `messages` (notification bodies with a `method`), thread ids dropped.
fn play(stream: &mut Stream, messages: &[Value]) -> Vec<Normalized> {
    let now = Instant::now();
    messages
        .iter()
        .flat_map(|m| stream.message(m, now))
        .map(|(_, e)| e)
        .collect()
}

fn note(method: &str, params: Value) -> Value {
    json!({ "method": method, "params": params })
}

fn turn_completed(status: &str, error: Value) -> Value {
    note(
        "turn/completed",
        json!({ "threadId": "t", "turn": { "id": "u", "status": status, "error": error } }),
    )
}

#[test]
fn a_turn_runs_a_command_and_ends_only_on_turn_completed() {
    let mut stream = Stream::new(Some("acct".into()));
    stream.bind("t", Some("gpt-fake".into()));
    let command = json!({ "type": "commandExecution", "id": "c1" });
    let events = play(
        &mut stream,
        &[
            note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } })),
            note(
                "thread/status/changed",
                json!({ "threadId": "t", "status": { "type": "active", "activeFlags": [] } }),
            ),
            note("item/started", json!({ "threadId": "t", "item": command })),
            note("item/completed", json!({ "threadId": "t", "item": command })),
            note(
                "thread/tokenUsage/updated",
                json!({ "threadId": "t", "tokenUsage": { "last": { "inputTokens": 5, "outputTokens": 2 } } }),
            ),
            note(
                "thread/status/changed",
                json!({ "threadId": "t", "status": { "type": "idle" } }),
            ),
            note("configWarning", json!({ "summary": "ignored" })),
            turn_completed("completed", Value::Null),
        ],
    );
    assert_eq!(
        events,
        [
            Normalized::TurnStarted,
            Normalized::State {
                state: SessionState::Running,
                background: false
            },
            Normalized::Activity { tool_running: true },
            Normalized::Activity { tool_running: false },
            Normalized::Usage {
                input_tokens: 5,
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
    assert!(stream.open().is_empty());
}

#[test]
fn a_usage_limit_is_a_quota_with_the_reset_of_the_exhausted_window() {
    let mut stream = Stream::new(Some("acct".into()));
    stream.bind("t", Some("gpt-fake".into()));
    let limits = json!({ "rateLimits": { "primary": { "usedPercent": 100, "resetsAt": 1_794_284_533 },
        "secondary": { "usedPercent": 42, "resetsAt": 1_794_680_933 } } });
    let error = json!({ "message": "You've hit your usage limit.", "codexErrorInfo": "usageLimitExceeded" });
    let events = play(
        &mut stream,
        &[
            note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } })),
            note("account/rateLimits/updated", limits),
            note(
                "thread/status/changed",
                json!({ "threadId": "t", "status": { "type": "systemError" } }),
            ),
            note("error", json!({ "threadId": "t", "willRetry": false, "error": error })),
            turn_completed("failed", error.clone()),
        ],
    );
    let usage = Normalized::Usage {
        input_tokens: 0,
        output_tokens: 0,
        rate_limit: Some(RateLimit {
            utilization: 1.0,
            resets_at_ms: Some(1_794_284_533_000),
        }),
    };
    assert_eq!(events[1], usage);
    let Normalized::Failure(failure) = &events[2] else {
        panic!("{events:?}")
    };
    assert_eq!(failure.kind, FailureKind::QuotaExhausted);
    assert_eq!(failure.retry_at_ms, Some(1_794_284_533_000));
    assert_eq!(
        (failure.account.as_deref(), failure.provider_model.as_deref()),
        (Some("acct"), Some("gpt-fake"))
    );
    assert_eq!(
        events[3],
        Normalized::TurnEnded {
            stop_reason: StopReason::Failed
        }
    );
    let idle = Normalized::State {
        state: SessionState::Idle,
        background: false,
    };
    assert_eq!(events[4], idle, "the state follows the turn end");
}

#[test]
fn a_window_past_its_reset_no_longer_counts() {
    let mut stream = Stream::new(None);
    stream.bind("t", None);
    let limits = |l: Value| note("account/rateLimits/updated", json!({ "rateLimits": l }));
    let events = play(
        &mut stream,
        &[
            note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } })),
            limits(json!({ "primary": { "usedPercent": 100, "resetsAt": 1_000 },
                "secondary": { "usedPercent": 42, "resetsAt": 4_000_000_000_i64 } })),
            limits(json!({ "secondary": { "usedPercent": 50, "resetsAt": 4_000_000_000_i64 } })),
        ],
    );
    let usage = |utilization, at: i64| Normalized::Usage {
        input_tokens: 0,
        output_tokens: 0,
        rate_limit: Some(RateLimit {
            utilization,
            resets_at_ms: Some(at * 1000),
        }),
    };
    assert_eq!(events[1..], [usage(1.0, 1_000), usage(0.5, 4_000_000_000)]);
}

#[test]
fn every_final_error_has_its_kind() {
    let cases = [
        (
            json!({ "responseTooManyFailedAttempts": { "httpStatusCode": 429 } }),
            FailureKind::RateLimited,
            false,
        ),
        (json!("serverOverloaded"), FailureKind::Overloaded, true),
        (
            json!({ "httpConnectionFailed": { "httpStatusCode": 401 } }),
            FailureKind::AuthRequired,
            false,
        ),
        (json!("contextWindowExceeded"), FailureKind::ContextOverflow, false),
        (json!("modelNotFound"), FailureKind::ConfigError, false),
        (json!("other"), FailureKind::Unknown, false),
    ];
    for (info, kind, retried) in cases {
        let mut stream = Stream::new(None);
        let error = json!({ "message": "it failed", "codexErrorInfo": info });
        let events = play(
            &mut stream,
            &[
                note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } })),
                note("error", json!({ "threadId": "t", "willRetry": false, "error": error })),
                turn_completed("failed", error.clone()),
            ],
        );
        let Normalized::Failure(failure) = &events[1] else {
            panic!("{events:?}")
        };
        assert_eq!((failure.kind, failure.retried_by_harness), (kind, retried), "{info}");
    }
}

#[test]
fn a_401_retry_reports_the_logout_once_and_an_interrupt_cancels() {
    let mut stream = Stream::new(None);
    let retry = |n: u32| {
        note(
            "error",
            json!({ "threadId": "t", "willRetry": true, "error": { "message": format!("Reconnecting... {n}/5"),
            "codexErrorInfo": { "responseStreamDisconnected": { "httpStatusCode": 401 } } } }),
        )
    };
    let started = note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } }));
    let events = play(&mut stream, &[started, retry(1), retry(2)]);
    assert!(matches!(&events[1], Normalized::Retrying { attempt: 1, .. }));
    assert!(
        matches!(&events[2], Normalized::Failure(f) if f.kind == FailureKind::AuthRequired && f.retried_by_harness)
    );
    assert!(matches!(&events[3], Normalized::Retrying { attempt: 2, .. }));
    assert_eq!(events.len(), 4);
    assert!(stream.interrupted("t"));
    let events = play(&mut stream, &[turn_completed("failed", Value::Null)]);
    assert_eq!(
        events[0],
        Normalized::TurnEnded {
            stop_reason: StopReason::Cancelled
        }
    );
}

#[test]
fn an_interrupt_after_the_turn_ended_does_not_cancel_the_next_turn() {
    let mut stream = Stream::new(None);
    let started = note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } }));
    play(
        &mut stream,
        &[started.clone(), turn_completed("completed", Value::Null)],
    );
    assert!(!stream.interrupted("t"));
    let quota = note(
        "error",
        json!({ "threadId": "t", "willRetry": false, "error": { "message": "You've hit your usage limit.",
            "codexErrorInfo": "usageLimitExceeded" } }),
    );
    let events = play(&mut stream, &[started, quota, turn_completed("failed", Value::Null)]);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Normalized::Failure(f) if f.kind == FailureKind::QuotaExhausted))
    );
    assert!(events.contains(&Normalized::TurnEnded {
        stop_reason: StopReason::Failed
    }));
}

#[test]
fn an_approval_is_a_permission_request_and_an_exit_crashes_open_turns() {
    let mut stream = Stream::new(None);
    let ask = json!({ "id": 0, "method": "item/commandExecution/requestApproval",
        "params": { "threadId": "t", "command": "touch x", "availableDecisions": ["accept", "cancel"] } });
    let events = play(
        &mut stream,
        &[
            note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } })),
            ask,
        ],
    );
    let Normalized::PermissionRequested {
        request_id,
        kind,
        tool,
        text,
        ..
    } = &events[1]
    else {
        panic!("{events:?}")
    };
    assert_eq!(
        (request_id.as_str(), *kind, tool.as_str(), text.as_str()),
        ("0", RequestKind::Permission, "commandExecution", "touch x")
    );
    let exit = stream.exited(None, Some(9), "boom");
    assert!(
        matches!(&exit[0], (Some(t), Normalized::Failure(f)) if t == "t" && f.kind == FailureKind::HarnessCrash && f.raw.ends_with("boom"))
    );
    assert_eq!(
        exit[1],
        (
            None,
            Normalized::Exited {
                code: None,
                signal: Some(9)
            }
        )
    );
}

#[test]
fn a_text_answers_every_question_of_a_user_input_request_by_its_id() {
    let request = json!({ "id": 7, "method": "item/tool/requestUserInput",
        "params": { "threadId": "t", "questions": [{ "id": "q1" }, { "id": "q2" }] } });
    let answer = Answer::Text { text: "blue".into() };
    assert_eq!(
        reply(&request, &answer),
        json!({ "id": 7, "result": { "answers": { "q1": { "answers": ["blue"] }, "q2": { "answers": ["blue"] } } } })
    );
}
