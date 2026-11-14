//! What the core does with each normalized driver event (docs/design/contracts.md 4.3, spec §2.2 item 5): every
//! event and field an adapter sends reaches the journal, the run view or the policy.

mod common;

use aae_core::driver::{Failure, FailureKind, Normalized, RateLimit, Source};
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
