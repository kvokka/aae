use super::*;
use aae_contract::driver::Normalized;
use aae_contract::driver::{RateLimit, SessionState, StopReason};

pub(crate) fn agent<T>(value: T) -> aae_contract::config::Sourced<T> {
    aae_contract::config::Sourced {
        value,
        from: aae_contract::config::Layer::Agent,
    }
}

pub(crate) fn config() -> EffectiveConfig {
    EffectiveConfig {
        services: Default::default(),
        harness: "codex".into(),
        harness_from: aae_contract::config::Layer::Agent,
        scope: Default::default(),
        model_given: None,
        model: Some(agent("gpt-fake".into())),
        effort: Some(agent("low".into())),
        system_prompt: None,
        inherit_host: Some(agent(false)),
        env: [("OPENAI_API_KEY".to_owned(), agent("sk-fake".to_owned()))].into(),
        mcp: Default::default(),
        hooks: Default::default(),
        skills: Default::default(),
        plugins: Vec::new(),
        native: Some(agent(
            json!({ "sandbox_mode": "danger-full-access", "approval_policy": "never",
            "model_providers": { "fake": { "base_url": "http://127.0.0.1:1/v1", "wire_api": "responses" } } }),
        )),
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
fn an_exit_crashes_open_turns() {
    let mut stream = Stream::new(None);
    play(
        &mut stream,
        &[note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } }))],
    );
    let exit = stream.exited(None, Some(9), "boom");
    assert!(matches!(&exit[0], (Some(t), Normalized::Failure(f)) if t == "t"
            && f.kind == FailureKind::HarnessCrash
            && f.raw.ends_with("boom")));
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

/// Nothing waits for a person: an approval with a `decline` is declined and the turn goes on, any other server
/// request gets an error at once (decision 117); each refusal is told as a denial (decision 124).
#[test]
fn every_server_request_is_refused_at_once() {
    let request = |method: &str| json!({ "id": 7, "method": method, "params": { "threadId": "t" } });
    for method in [
        "item/commandExecution/requestApproval",
        "item/fileChange/requestApproval",
    ] {
        let answer = rpc::refusal(&request(method));
        assert_eq!(
            answer,
            json!({ "id": 7, "result": { "decision": "decline" } }),
            "{method}"
        );
    }
    for method in [
        "item/permissions/requestApproval",
        "item/tool/requestUserInput",
        "mcpServer/elicitation/request",
    ] {
        let answer = rpc::refusal(&request(method));
        assert_eq!(
            (answer["id"].clone(), answer["error"]["code"].clone()),
            (json!(7), json!(-32601)),
            "{method}"
        );
        assert!(answer.get("result").is_none());
    }
    // Each refusal is a denial of the thread.
    let approval = json!({ "id": 8, "method": "item/commandExecution/requestApproval",
        "params": { "threadId": "t", "itemId": "call_1", "command": "rm a", "reason": "outside the sandbox" } });
    assert_eq!(
        play(&mut Stream::new(None), std::slice::from_ref(&approval)),
        [Normalized::PermissionDenied {
            tool: "item/commandExecution/requestApproval".into(),
            tool_use_id: Some("call_1".into()),
            input: approval["params"].clone(),
            reason: Some("outside the sandbox".into()),
        }]
    );
}

/// A command that outlives its turn (docs/design/contracts.md 4.6): the turn ends idle with background, and the thread
/// is idle without it only once the command completes, outside any turn.
#[test]
fn a_command_that_outlives_its_turn_keeps_the_thread_in_background() {
    let mut stream = Stream::new(None);
    let command = |method: &str| {
        note(
            method,
            json!({ "threadId": "t", "item": { "type": "commandExecution", "id": "c" } }),
        )
    };
    let events = play(
        &mut stream,
        &[
            note("turn/started", json!({ "threadId": "t", "turn": { "id": "u" } })),
            command("item/started"),
            turn_completed("completed", Value::Null),
        ],
    );
    let idle = |background| Normalized::State {
        state: SessionState::Idle,
        background,
    };
    assert_eq!(
        events[events.len() - 2..],
        [
            Normalized::TurnEnded {
                stop_reason: StopReason::Completed
            },
            idle(true)
        ]
    );
    let idle_note = note(
        "thread/status/changed",
        json!({ "threadId": "t", "status": { "type": "idle" } }),
    );
    assert_eq!(
        play(&mut stream, &[idle_note]),
        [idle(true)],
        "an idle status with the command running"
    );
    let events = play(&mut stream, &[command("item/completed")]);
    assert_eq!(events.last(), Some(&idle(false)), "{events:?}");
}
