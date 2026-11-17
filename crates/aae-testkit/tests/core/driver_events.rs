//! What the core does with each normalized driver event (docs/design/contracts.md 4.3, decision 61): every
//! event and field an adapter sends reaches the journal, the run view or the policy.

use crate::common;

use aae_core::driver::{Failure, FailureKind, Normalized, RateLimit, Source, StopReason};
use aae_core::run::ExecHooks;
use aae_testkit::Step;
use common::{World, hollow_turn, last, world, world_with};
use serde_json::json;
use std::{sync::Arc, time::Duration};

fn usage(input_tokens: u64, output_tokens: u64, rate_limit: Option<RateLimit>) -> Step {
    Step::Emit(Normalized::Usage {
        input_tokens,
        output_tokens,
        rate_limit,
    })
}

/// A world with the bundled policy, no quota margin, and an agent of account `a`.
fn with_policy() -> World {
    let config = "projects = [\"shop\"]\n[harness.scripted]\n[builtin.default-policy]\nquota_margin_s = 0\n";
    let agent = "harness = \"scripted\"\n[native.scripted]\naccount = \"a\"\n";
    let hooks = ExecHooks {
        socket: None,
        bundled: Some(aae_plugins::run),
    };
    world_with(
        &[("config.toml", config), ("shop/agents/dev/agent.toml", agent)],
        Arc::new(hooks),
    )
}

#[tokio::test(start_paused = true)]
async fn the_tokens_of_a_run_add_up_over_its_usage_events() {
    let w = world(&[]);
    let mut steps = hollow_turn();
    steps.splice(1..1, [usage(100, 20, None), usage(5, 7, None)]);
    w.driver.script(steps);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let tokens = json!({ "input_tokens": 105, "output_tokens": 27 });
    assert_eq!(last(&events, "run.failed")["usage"], tokens);
    let reader = aae_core::run::Reader::open(&w.dir.path().join("state.db")).unwrap();
    let view = reader.run(&run).unwrap().unwrap();
    assert_eq!(view["usage"], tokens);
    assert_eq!(view["attempts"][0]["input_tokens"], 105);
}

#[tokio::test(start_paused = true)]
async fn a_quota_without_a_reset_time_waits_for_the_accounts_known_reset() {
    let w = with_policy();
    let limit = RateLimit {
        utilization: 1.0,
        resets_at_ms: Some(2 * 3_600_000),
    };
    let quota = Failure::new(FailureKind::QuotaExhausted, Source::Adapter);
    w.driver
        .script(vec![usage(0, 0, Some(limit)), Step::Emit(Normalized::Failure(quota))]);
    let run = w.create().await;
    let failed = w.until(Some(&run), "run.attempt_failed", 0).await;
    assert_eq!(
        failed["rate_limit"],
        json!({ "utilization": 1.0, "resets_at": "1970-01-01T02:00:00.000Z" })
    );
    let parked = w.until(Some(&run), "run.parked", 0).await;
    assert_eq!(parked["gate"]["until"], "1970-01-01T02:00:00.000Z", "{parked}");
}

#[tokio::test(start_paused = true)]
async fn the_event_of_a_turn_without_an_outcome_tells_how_the_turn_ended() {
    let w = world(&[]);
    let mut cancelled = hollow_turn();
    cancelled[1] = Step::Emit(Normalized::TurnEnded {
        stop_reason: StopReason::Cancelled,
    });
    w.driver.script(cancelled);
    let run = w.create().await;
    let ended = w.until(Some(&run), "run.turn_ended_without_outcome", 0).await;
    assert_eq!(ended["stop_reason"], "cancelled");
}

#[tokio::test(start_paused = true)]
async fn a_degraded_instance_is_told_and_keeps_its_session() {
    let w = world(&[]);
    let mut long = hollow_turn();
    long.insert(1, Step::Wait(Duration::from_secs(150)));
    w.driver.script(long);
    let run = w.create().await;
    w.until(Some(&run), "run.state_changed", 2).await;
    w.driver.degrade(Some("auth refresh failing"));
    let degraded = w.until(None, "instance.degraded", 0).await;
    assert_eq!(degraded["reason"], "auth refresh failing");
    let reader = aae_core::run::Reader::open(&w.dir.path().join("state.db")).unwrap();
    let instance = &reader.instances().unwrap()[0];
    assert_eq!(
        (&instance["state"], &instance["degraded"]),
        (&json!("ready"), &json!("auth refresh failing"))
    );
    w.driver.degrade(None);
    w.until(None, "instance.ready", 1).await;
    assert_eq!(reader.instances().unwrap()[0]["degraded"], json!(null));
    let events = w.events(&run, Duration::from_secs(150)).await;
    assert!(
        !events.iter().any(|(t, _)| t == "run.attempt_failed"),
        "the session goes on"
    );
    assert_eq!(
        last(&events, "run.turn_ended_without_outcome")["stop_reason"],
        "completed"
    );
}

#[tokio::test(start_paused = true)]
async fn a_repeated_driver_event_is_dropped_and_a_gap_is_told() {
    let w = world(&[]);
    let crash = Failure::new(FailureKind::HarnessCrash, Source::Adapter);
    // The ack is 1 and `turn.started` 2: the crash repeats 2, then the numbering jumps to 9.
    let mut steps = hollow_turn();
    steps.splice(
        1..1,
        [Step::Reseq(2), Step::Emit(Normalized::Failure(crash)), Step::Reseq(9)],
    );
    w.driver.script(steps);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert!(
        !events.iter().any(|(t, _)| t == "run.attempt_failed"),
        "the repeat is dropped"
    );
    assert_eq!(
        last(&events, "run.turn_ended_without_outcome")["stop_reason"],
        "completed"
    );
    let told: Vec<_> = w
        .all("resync.diverged")
        .iter()
        .map(|d| (d["expected"].clone(), d["observed"].clone()))
        .collect();
    assert_eq!(told, [(json!(3), json!(2)), (json!(3), json!(9))]);
}

#[tokio::test(start_paused = true)]
async fn a_failure_before_the_ack_ends_the_attempt_at_once() {
    let w = world(&[]);
    let quota = Failure::new(FailureKind::QuotaExhausted, Source::Adapter);
    w.driver
        .script(vec![Step::NoAck, Step::Emit(Normalized::Failure(quota))]);
    let run = w.create().await;
    let states = w.states(&run).await;
    assert_eq!(states[states.len() - 3..], ["starting", "interrupted", "failed"]);
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(
        last(&events, "run.attempt_failed")["failure"]["kind"],
        "quota_exhausted"
    );
}

/// A denial is told as `run.permission_denied` with what the adapter reported, and nothing waits on it: the turn goes
/// on to its end (decision 124).
#[tokio::test(start_paused = true)]
async fn a_denied_permission_is_told_and_the_turn_goes_on() {
    let w = world(&[]);
    let mut steps = hollow_turn();
    let denied = Normalized::PermissionDenied {
        tool: "Bash".into(),
        tool_use_id: Some("toolu_1".into()),
        input: json!({ "command": "touch a" }),
        reason: Some("mode".into()),
    };
    steps.insert(1, Step::Emit(denied));
    w.driver.script(steps);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let told = last(&events, "run.permission_denied");
    assert_eq!(
        (&told["tool"], &told["tool_use_id"], &told["input"], &told["reason"]),
        (
            &json!("Bash"),
            &json!("toolu_1"),
            &json!({ "command": "touch a" }),
            &json!("mode")
        )
    );
    assert_eq!(told["run"]["id"], run.as_str());
    assert_eq!(last(&events, "run.failed")["class"], "hollow_done");
}
