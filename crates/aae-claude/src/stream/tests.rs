use super::*;
use serde_json::json;

mod classify;

const KEY: &str = "26c22b42-962d-4bc6-9c9f-f75d7f34b294";

pub(super) fn feed(stream: &mut Stream, lines: &[Value]) -> Vec<Normalized> {
    let now = Instant::now();
    lines.iter().flat_map(|l| stream.line(l, now)).collect()
}

pub(super) fn stream() -> Stream {
    Stream::new(Some("acct".into()), Some("claude-haiku-5-5".into()))
}

pub(super) fn result(is_error: bool, extra: Value) -> Value {
    let mut msg = json!({ "type": "result", "subtype": "success", "is_error": is_error,
        "usage": { "input_tokens": 3, "output_tokens": 2 }, "result": "OK" });
    msg.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    msg
}

pub(super) fn failure(events: &[Normalized]) -> &Failure {
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
        s.exited(Some(0), None, ""),
        vec![Normalized::Exited {
            code: Some(0),
            signal: None
        }]
    );
}

#[test]
fn an_interrupted_turn_is_cancelled() {
    let mut s = stream();
    feed(&mut s, &[json!({ "type": "system", "subtype": "init" })]);
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
fn an_interrupt_after_the_turn_ended_does_not_cancel_the_next_turn() {
    let mut s = stream();
    feed(
        &mut s,
        &[json!({ "type": "system", "subtype": "init" }), result(false, json!({}))],
    );
    s.interrupted();
    let events = feed(
        &mut s,
        &[json!({ "type": "system", "subtype": "init" }), result(true, json!({}))],
    );
    assert!(
        matches!(
            events[..],
            [
                ..,
                Normalized::Failure(_),
                Normalized::TurnEnded {
                    stop_reason: StopReason::Failed
                }
            ]
        ),
        "{events:?}"
    );
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
fn a_stopped_task_is_no_wake_up_and_only_commands_are_stopped() {
    let mut s = stream();
    let tasks = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [
        { "task_id": "b1", "task_type": "local_bash" }, { "task_id": "a1", "task_type": "local_agent" }] });
    let none = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] });
    let stopped = json!({ "type": "system", "subtype": "task_notification", "status": "stopped" });
    feed(&mut s, &[tasks, result(false, json!({})), state("idle")]);
    assert_eq!(s.commands(), ["b1"]);
    let idle = Normalized::State {
        state: SessionState::Idle,
        background: false,
    };
    assert_eq!(feed(&mut s, &[stopped, none, state("idle")]).last(), Some(&idle));
    assert!(s.commands().is_empty());
}

/// As claude 2.1.326 runs a background subagent on a fake provider: the state stays `running` after the result, and
/// the subagent's steps come out of the turn, as far apart as its commands run.
#[test]
fn a_background_subagent_keeps_the_session_idle_with_background_until_its_wake_up_ends() {
    let mut s = stream();
    let agent = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [
        { "task_id": "a1", "task_type": "local_agent", "is_backgrounded": true }] });
    let none = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] });
    let step = json!({ "type": "user", "parent_tool_use_id": "toolu_1", "agent_id": "a1" });
    let notified = json!({ "type": "system", "subtype": "task_notification", "status": "completed" });
    let idle = |background| Normalized::State {
        state: SessionState::Idle,
        background,
    };
    let bash = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [
        { "task_id": "b1", "task_type": "local_bash", "is_backgrounded": true }] });
    let events = feed(&mut s, &[bash, result(false, json!({}))]);
    assert!(
        !events.iter().any(|e| matches!(e, Normalized::State { .. })),
        "{events:?}"
    );
    feed(&mut s, &[json!({ "type": "system", "subtype": "init" })]);
    let events = feed(&mut s, &[agent, result(false, json!({}))]);
    assert_eq!(events.last(), Some(&idle(true)), "{events:?}");
    // The subagent's own requests out of the turn keep the background budget.
    for line in [
        json!({ "type": "system", "subtype": "api_retry", "attempt": 1, "retry_delay_ms": 500, "error": "overloaded" }),
        json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed" } }),
        json!({ "type": "system", "subtype": "permission_denied", "tool_name": "Bash", "tool_use_id": "toolu_9" }),
    ] {
        let events = s.line(&line, Instant::now());
        assert_eq!(events.len(), 2, "{events:?}");
        assert_eq!(events.last(), Some(&idle(true)), "{events:?}");
    }
    let quiet = feed(&mut s, &[step, notified, none]);
    assert!(quiet.is_empty(), "{quiet:?}");
    let wake = json!({ "type": "system", "subtype": "init" });
    let woken = feed(&mut s, &[wake, result(false, json!({})), state("idle")]);
    assert_eq!(woken.last(), Some(&idle(false)), "{woken:?}");
    assert!(
        !woken[..woken.len() - 1]
            .iter()
            .any(|e| matches!(e, Normalized::State { .. })),
        "{woken:?}"
    );
}

#[test]
fn activity_is_coalesced_to_one_a_second() {
    let mut s = stream();
    s.line(&json!({ "type": "system", "subtype": "init" }), Instant::now());
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
    let events = s.exited(Some(0), None, "Error: boom\n");
    assert_eq!(failure(&events).kind, FailureKind::HarnessCrash);
    assert_eq!(failure(&events).raw, "exited with code 0 in a turn: Error: boom");
    assert_eq!(
        events.last(),
        Some(&Normalized::Exited {
            code: Some(0),
            signal: None
        })
    );
    for status in ["failed", "needs-auth"] {
        let servers = json!([{ "name": "ok", "status": "connected" }, { "name": "x", "status": status }]);
        let events = feed(
            &mut stream(),
            &[json!({ "type": "system", "subtype": "init", "mcp_servers": servers })],
        );
        let f = failure(&events);
        assert_eq!((f.kind, f.raw.as_str()), (FailureKind::ToolFailure, "x"), "{status}");
    }
}

/// A denial comes as `system/permission_denied` and again in the turn's `result.permission_denials`: one event each,
/// keyed by `tool_use_id`; the next turn starts with nothing told.
#[test]
fn a_denial_is_told_once_from_either_source() {
    let mut s = stream();
    let denied = |id: &str| {
        json!({ "type": "system", "subtype": "permission_denied", "tool_name": "Bash", "tool_use_id": id,
            "decision_reason_type": "mode", "message": "denied" })
    };
    let listed = |ids: &[&str]| {
        let list: Vec<Value> = ids
            .iter()
            .map(|id| json!({ "tool_name": "Bash", "tool_use_id": id, "tool_input": { "command": "touch a" } }))
            .collect();
        result(false, json!({ "permission_denials": list }))
    };
    let denials = |events: Vec<Normalized>| -> Vec<Normalized> {
        events
            .into_iter()
            .filter(|e| matches!(e, Normalized::PermissionDenied { .. }))
            .collect()
    };
    let first = denials(feed(&mut s, &[denied("toolu_1"), listed(&["toolu_1", "toolu_2"])]));
    let told = |id: &str, input: Value, reason: Option<&str>| Normalized::PermissionDenied {
        tool: "Bash".into(),
        tool_use_id: Some(id.into()),
        input,
        reason: reason.map(str::to_owned),
    };
    assert_eq!(
        first,
        [
            told("toolu_1", Value::Null, Some("mode")),
            told("toolu_2", json!({ "command": "touch a" }), None),
        ]
    );
    let next = denials(feed(&mut s, &[listed(&["toolu_1"])]));
    assert_eq!(next, [told("toolu_1", json!({ "command": "touch a" }), None)]);
}
