//! The model gate key of a run is its config's (docs/design/contracts.md 7): a closed model gate holds a new run on
//! that model before its first attempt, and a failure that names another model adds its key.

mod common;

use aae_core::{
    driver::{Failure, FailureKind, Normalized, Source},
    run::{GateSpec, Msg},
};
use aae_testkit::Step;
use common::{World, world};
use std::time::Duration;

const ON_MODEL: &[(&str, &str)] = &[(
    "shop/agents/dev/agent.toml",
    "harness = \"scripted\"\nprovider = \"acme\"\nmodel = \"m1\"\n",
)];

impl World {
    async fn close(&self, key: &str) {
        let gate = GateSpec {
            key: key.into(),
            until_ms: None,
            reason: "model down".into(),
        };
        self.core.send(None, Msg::CloseGate(gate)).await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn a_closed_model_gate_holds_a_new_run_before_its_first_attempt() {
    let w = world(ON_MODEL);
    w.close("model:acme/m1").await;
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(60)).await;
    let held = events
        .iter()
        .find(|(kind, e)| matches!(kind.as_str(), "run.queued" | "run.parked") && !e["gate"].is_null());
    let held = held.unwrap_or_else(|| panic!("never held by a gate: {events:?}"));
    assert_eq!(held.1["gate"]["key"], "model:acme/m1");
    let states = w.states(&run).await;
    assert!(!states.iter().any(|s| s == "starting"), "no attempt starts: {states:?}");
}

#[tokio::test(start_paused = true)]
async fn a_failure_naming_another_model_adds_its_key() {
    let w = world(ON_MODEL);
    let mut quota = Failure::new(FailureKind::QuotaExhausted, Source::Adapter);
    quota.provider_model = Some("m1-2026".into());
    w.driver.script(vec![Step::Emit(Normalized::Failure(quota))]);
    let run = w.create().await;
    let keys = &w.until(Some(&run), "run.attempt_failed", 0).await["keys"];
    let keys: Vec<&str> = keys.as_array().unwrap().iter().filter_map(|k| k.as_str()).collect();
    assert!(keys.ends_with(&["model:acme/m1", "model:acme/m1-2026"]), "{keys:?}");
}
