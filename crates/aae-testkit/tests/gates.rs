//! Issue gates and parked runs (spec §5.7, invariant 18, docs/design/contracts.md 6–7) on the managed clock.

mod common;

use aae_core::{
    Journal,
    run::{Decision, GateSpec, Msg},
};
use aae_testkit::Step;
use common::{POLICY, World, hollow_turn, last, world};
use serde_json::Value;
use std::time::Duration;

const RIG: &str = "rig:shop/web";

fn gate(key: &str, until_ms: Option<i64>) -> GateSpec {
    GateSpec {
        key: key.into(),
        until_ms,
        reason: "quota".into(),
    }
}

fn crash() -> Vec<Step> {
    vec![Step::Exit {
        code: None,
        signal: Some(9),
    }]
}

impl World {
    /// The data of every event `kind` in the journal, of any run or none.
    fn all(&self, kind: &str) -> Vec<Value> {
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        all.into_iter()
            .filter(|e| e["type"] == kind)
            .map(|e| e["data"].clone())
            .collect()
    }

    async fn operator(&self, msg: Msg) {
        self.core.send(None, msg).await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn an_operator_pause_keeps_a_new_run_queued_with_the_reason_until_opened() {
    let w = world(&[]);
    w.driver.script(hollow_turn());
    w.operator(Msg::CloseGate(gate("project:shop", None))).await;
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(3600)).await;
    let queued = last(&events, "run.queued");
    assert_eq!(
        (&queued["waiting_for"], &queued["gate"]["key"]),
        (&"gate".into(), &"project:shop".into())
    );
    assert_eq!(queued["gate"]["closed_by"], "operator");
    assert!(w.states(&run).await.is_empty(), "never left `queued`");
    w.operator(Msg::OpenGate {
        key: "project:shop".into(),
    })
    .await;
    assert_eq!(w.all("gate.opened")[0]["by"], "operator");
    assert_eq!(w.states(&run).await.last().unwrap(), "failed");
}

#[tokio::test(start_paused = true)]
async fn a_parked_run_resumes_when_the_gate_opens_by_its_timer() {
    let w = world(POLICY);
    w.driver.script(crash());
    let run = w.create().await;
    let park = Decision::Park {
        gate: gate(RIG, Some(600_000)),
        then: "resume".into(),
    };
    w.decide(&run, "run.attempt_failed", 0, park).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let parked = last(&events, "run.parked");
    assert_eq!(
        (&parked["gate"]["key"], &parked["then"]),
        (&RIG.into(), &"resume".into())
    );
    assert_eq!(parked["gate"]["until"], "1970-01-01T00:10:00.000Z");
    assert_eq!(w.states(&run).await.last().unwrap(), "parked");
    w.driver.script(hollow_turn());
    let events = w.events(&run, Duration::from_secs(600)).await;
    assert_eq!(last(&events, "run.resumed")["gate_key"], RIG);
    assert_eq!(w.all("gate.opened")[0]["by"], "timer");
    let states = w.states(&run).await;
    assert!(
        states.ends_with(&["parked".into(), "starting".into(), "running".into(), "failed".into()]),
        "{states:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_policy_timer_does_not_reopen_an_operator_pause() {
    let w = world(POLICY);
    w.driver.script(crash());
    let run = w.create().await;
    w.operator(Msg::CloseGate(gate(RIG, None))).await;
    let park = Decision::Park {
        gate: gate(RIG, Some(60_000)),
        then: "new_session".into(),
    };
    w.decide(&run, "run.attempt_failed", 0, park).await;
    w.events(&run, Duration::from_secs(120)).await;
    assert!(w.all("gate.opened").is_empty(), "the operator closure holds the key");
    assert_eq!(w.states(&run).await.last().unwrap(), "parked");
    w.driver.script(hollow_turn());
    w.operator(Msg::OpenGate { key: RIG.into() }).await;
    let states = w.states(&run).await;
    assert!(
        states.ends_with(&["parked".into(), "starting".into(), "running".into(), "failed".into()]),
        "{states:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_retry_under_an_operator_pause_parks_and_times_out() {
    let w = world(POLICY);
    w.driver
        .script(vec![Step::Wait(Duration::from_secs(10)), crash().remove(0)]);
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.operator(Msg::CloseGate(gate(RIG, None))).await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, resume).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let gates = &last(&events, "run.attempt_failed")["gates"];
    assert_eq!(
        (&gates[0]["key"], &gates[0]["closed_by"]),
        (&RIG.into(), &"operator".into())
    );
    assert_eq!(last(&events, "run.parked")["then"], "resume");
    let events = w.events(&run, Duration::from_secs(7 * 86_400)).await;
    let failed = last(&events, "run.failed");
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&"infrastructure".into(), &"park_timeout".into())
    );
}
