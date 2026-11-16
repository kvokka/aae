//! The model gate key of a run is its config's alone (spec §5.7, docs/design/contracts.md 7): a closed model gate
//! holds a new run on that model before its first attempt, and a failure that names another model adds no key.

use crate::common;

use aae_core::{
    driver::{Failure, FailureKind, Normalized, Source},
    run::{GateSpec, Msg},
};
use aae_testkit::Step;
use common::{World, world};
use std::time::Duration;

const ON_MODEL: &[(&str, &str)] = &[("shop/agents/dev/agent.toml", "harness = \"scripted\"\nmodel = \"m1\"\n")];

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
    w.close("model:scripted/m1").await;
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(60)).await;
    let held = events
        .iter()
        .find(|(kind, e)| matches!(kind.as_str(), "run.queued" | "run.parked") && !e["gate"].is_null());
    let held = held.unwrap_or_else(|| panic!("never held by a gate: {events:?}"));
    assert_eq!(held.1["gate"]["key"], "model:scripted/m1");
    let states = w.states(&run).await;
    assert!(!states.iter().any(|s| s == "starting"), "no attempt starts: {states:?}");
}

#[tokio::test(start_paused = true)]
async fn a_failure_naming_another_model_adds_no_key() {
    let w = world(ON_MODEL);
    let mut quota = Failure::new(FailureKind::QuotaExhausted, Source::Adapter);
    quota.provider_model = Some("m1-2026".into());
    w.driver.script(vec![Step::Emit(Normalized::Failure(quota))]);
    let run = w.create().await;
    let keys = &w.until(Some(&run), "run.attempt_failed", 0).await["keys"];
    let keys: Vec<&str> = keys.as_array().unwrap().iter().filter_map(|k| k.as_str()).collect();
    assert_eq!(
        keys.iter()
            .filter(|k| k.starts_with("model:"))
            .copied()
            .collect::<Vec<_>>(),
        ["model:scripted/m1"]
    );
}

#[tokio::test(start_paused = true)]
async fn a_closed_model_gate_keeps_a_new_run_queued() {
    let w = world(ON_MODEL);
    w.close("model:scripted/m1").await;
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(60)).await;
    let queued = events.iter().find(|(kind, _)| kind == "run.queued").map(|(_, e)| e);
    assert_eq!(
        queued.map(|e| &e["gate"]["key"]),
        Some(&"model:scripted/m1".into()),
        "{events:?}"
    );
    assert!(w.states(&run).await.is_empty(), "never left `queued`");
}

#[tokio::test(start_paused = true)]
async fn a_rig_runs_the_variant_under_its_name() {
    let project = "[agents.fast]\nbase = \"shop.dev\"\nmodel = \"m2\"\n\
                   [rigs.web]\npath = \"web\"\nagents = { dev = \"fast\" }\n";
    let w = world(&[ON_MODEL[0], ("shop/aae.toml", project)]);
    w.close("model:scripted/m2").await;
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(60)).await;
    let queued = events
        .iter()
        .find(|(kind, _)| kind == "run.queued")
        .map(|(_, e)| e)
        .unwrap();
    assert_eq!(
        (&queued["run"]["agent"], &queued["gate"]["key"]),
        (&"fast".into(), &"model:scripted/m2".into())
    );
}
