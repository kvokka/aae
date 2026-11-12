//! Requests of a live attempt and failures outside `running` (docs/design/contracts.md 6.1–6.2, invariant 5).

mod common;

use aae_core::{
    Error,
    driver::{Answer, Failure, FailureKind, Normalized, RequestKind, Source},
    run::Msg,
};
use aae_testkit::Step;
use common::{hollow_turn, last, world};
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn a_permission_request_waits_for_its_answer() {
    let w = world(&[]);
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Permission,
        tool: "Bash".into(),
        input: json!({ "command": "ls" }),
        text: String::new(),
    };
    w.driver.script(vec![Step::Emit(request)]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    assert_eq!(w.states(&run).await.last().unwrap(), "requires_action");
    let answer = |request: &str| Msg::Answer {
        run: run.clone(),
        request: request.into(),
        answer: Answer::Allow { updated_input: None },
    };
    assert!(matches!(
        w.core.send(None, answer("req-2")).await,
        Err(Error::Rejected(_))
    ));
    w.core.send(None, answer("req-1")).await.unwrap();
    assert_eq!(w.states(&run).await[3..], ["requires_action", "running", "failed"]);
}

#[tokio::test(start_paused = true)]
async fn a_request_nobody_answers_is_denied_after_the_human_timeout() {
    let w = world(&[]);
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Question,
        tool: String::new(),
        input: Value::Null,
        text: "Which branch?".into(),
    };
    w.driver.script(vec![Step::Emit(request)]);
    let run = w.create().await;
    let states = w.states(&run).await;
    assert_eq!(states.last().unwrap(), "requires_action");
    tokio::time::sleep(Duration::from_secs(3600)).await;
    assert_eq!(w.states(&run).await[4..], ["running"]);
}

#[tokio::test(start_paused = true)]
async fn a_failure_before_the_ack_or_in_requires_action_ends_the_attempt_at_once() {
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Question,
        tool: String::new(),
        input: Value::Null,
        text: "Which branch?".into(),
    };
    let quota = Failure::new(FailureKind::QuotaExhausted, Source::Adapter);
    let cases = [
        (
            vec![Step::NoAck, Step::Emit(Normalized::Failure(quota))],
            "starting",
            "quota_exhausted",
        ),
        (
            vec![
                Step::Emit(request),
                Step::Exit {
                    code: Some(1),
                    signal: None,
                },
            ],
            "requires_action",
            "harness_crash",
        ),
    ];
    for (steps, from, kind) in cases {
        let w = world(&[]);
        w.driver.script(steps);
        let run = w.create().await;
        let states = w.states(&run).await;
        assert_eq!(states[states.len() - 3..], [from, "interrupted", "failed"], "{kind}");
        let events = w.events(&run, Duration::ZERO).await;
        assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], kind);
    }
}
