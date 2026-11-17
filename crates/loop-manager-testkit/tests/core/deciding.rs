//! The wait on a deciding event in a live daemon (docs/design/contracts.md 3.3): its deadline counts the deciding
//! hooks of the run queued ahead of it, and no clock of the attempt runs while the run waits.

use crate::common;

use common::{World, hollow_turn, last, world_with};
use loop_manager_core::{
    BoxFuture, CoreHandle,
    driver::Normalized,
    run::{Decision, HookRunner, Msg},
};
use loop_manager_testkit::Step;
use serde_json::Value;
use std::{sync::Arc, time::Duration};

const HOLLOW: &str = "run.turn_ended_without_outcome";
const FAILED: &str = "run.attempt_failed";

/// An addon with a 30 s hook on each of `on`.
fn addon(on: &[&str], watchdog: &str) -> World {
    let config = format!(
        "projects = [\"shop\"]\nbuiltin = {{ default-policy = {{ enabled = false }} }}\n[harness.scripted]\n\
         [settings]\naddons = [{{ name = \"policy\" }}]\n[[addon]]\npath = \"policy\"\n{watchdog}"
    );
    let hooks: String = on
        .iter()
        .map(|on| format!("[[hook]]\non = \"{on}\"\ncommand = [\"true\"]\ntimeout = \"30s\"\n"))
        .collect();
    let manifest = format!("name = \"policy\"\nversion = \"0.1.0\"\n{hooks}");
    let files = [
        ("config.toml", config.as_str()),
        ("policy/addon.toml", manifest.as_str()),
    ];
    world_with(&files, Arc::new(Slow))
}

/// Every hook runs 29 s, within its timeout; on a hollow turn or a failed attempt it then fails the run.
struct Slow;

impl HookRunner for Slow {
    fn run<'a>(&'a self, core: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(29)).await;
            let event = &payload["event"];
            if event["type"] == HOLLOW || event["type"] == FAILED {
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
    let w = addon(&[HOLLOW, FAILED], "");
    // The process dies while the hook on the hollow turn still runs: the failed attempt's hook waits behind it.
    let mut turn = hollow_turn();
    turn.push(Step::Wait(Duration::from_secs(1)));
    turn.push(Step::Exit {
        code: None,
        signal: Some(9),
    });
    w.driver.script(turn);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(120)).await;
    assert_eq!(last(&events, "run.failed")["reason"], "judged", "{events:?}");
}

#[tokio::test(start_paused = true)]
async fn late_session_events_do_not_stall_a_run_waiting_for_a_decision() {
    let w = addon(&[HOLLOW], "[watchdog]\nsilence = \"5s\"\n");
    let mut turn = hollow_turn();
    turn.push(Step::Wait(Duration::from_secs(1)));
    turn.push(Step::Emit(Normalized::Activity { tool_running: false }));
    w.driver.script(turn);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(60)).await;
    assert!(events.iter().all(|(t, _)| t != "run.attempt_failed"), "{events:?}");
    assert_eq!(last(&events, "run.failed")["reason"], "judged", "{events:?}");
}
