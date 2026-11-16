//! The wait on a deciding event in a live daemon (docs/design/contracts.md 3.3): its deadline counts the deciding
//! hooks of the run queued ahead of it, and no clock of the attempt runs while the run waits.

use crate::common;

use aae_core::{
    BoxFuture, CoreHandle,
    driver::{Answer, Normalized, RequestKind},
    run::{Decision, HookRunner, Msg},
};
use aae_testkit::Step;
use common::{World, hollow_turn, last, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const HOLLOW: &str = "run.turn_ended_without_outcome";

/// A plugin with a 10 s hook on each of `on`.
fn plugin(on: &[&str], watchdog: &str) -> World {
    let config = format!(
        "projects = [\"shop\"]\nbuiltin = {{ default-policy = {{ enabled = false }} }}\n[harness.scripted]\n\
         [[plugin]]\npath = \"policy\"\n{watchdog}"
    );
    let hooks: String = on
        .iter()
        .map(|on| format!("[[hook]]\non = \"{on}\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n"))
        .collect();
    let manifest = format!("name = \"policy\"\nversion = \"0.1.0\"\n{hooks}");
    let files = [
        ("config.toml", config.as_str()),
        ("policy/plugin.toml", manifest.as_str()),
    ];
    world_with(&files, Arc::new(Slow))
}

/// Every hook runs 9 s, within its timeout; on a hollow turn it then fails the run.
struct Slow;

impl HookRunner for Slow {
    fn run<'a>(&'a self, core: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(9)).await;
            let event = &payload["event"];
            if event["type"] == HOLLOW {
                let msg = Msg::Decide {
                    run: event["run"].as_str().unwrap().into(),
                    seq: event["seq"].as_i64().unwrap(),
                    decision: Decision::Fail {
                        reason: "judged".into(),
                        gate: None,
                    },
                };
                // A decision after the deadline is refused: the run already moved on.
                core.send(None, msg).await.ok();
            }
        })
    }
}

#[tokio::test(start_paused = true)]
async fn a_deciding_event_waits_for_the_slow_hook_of_the_run_queued_ahead() {
    let w = plugin(&["run.requires_action", HOLLOW], "");
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Permission,
        tool: "Bash".into(),
        input: json!({}),
        text: String::new(),
    };
    w.driver.script(vec![Step::Emit(request)]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    w.until(Some(&run), "run.requires_action", 0).await;
    // A human answers while the hook on the request still runs: the hollow turn's hook waits behind it.
    let answer = Msg::Answer {
        run: run.clone(),
        request: "req-1".into(),
        answer: Answer::Allow { updated_input: None },
    };
    w.core.send(None, answer).await.unwrap();
    let events = w.events(&run, Duration::from_secs(60)).await;
    assert_eq!(last(&events, "run.failed")["reason"], "judged", "{events:?}");
}

#[tokio::test(start_paused = true)]
async fn late_session_events_do_not_stall_a_run_waiting_for_a_decision() {
    let w = plugin(&[HOLLOW], "[watchdog]\nsilence = \"5s\"\n");
    let mut turn = hollow_turn();
    turn.push(Step::Wait(Duration::from_secs(1)));
    turn.push(Step::Emit(Normalized::Activity { tool_running: false }));
    w.driver.script(turn);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(60)).await;
    assert!(events.iter().all(|(t, _)| t != "run.attempt_failed"), "{events:?}");
    assert_eq!(last(&events, "run.failed")["reason"], "judged", "{events:?}");
}
