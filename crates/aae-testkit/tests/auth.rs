//! Postmortem 0001 on the scripted driver and the bundled `default-policy`: a 403 parks the account for a growing
//! wait, the attempt after it probes the cause, and a 403 that lasts keeps waiting, past the attempts ceiling, with no
//! operator (spec §2.1 item 12). Bundled hooks run on blocking threads, so real time.

mod common;

use aae_core::{
    driver::{Failure, FailureKind, Normalized, Source},
    run::{ExecHooks, MAX_ATTEMPTS, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{World, hollow_turn, world_with};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const ACCOUNT: &str = "account:scripted/a";

/// A world with the bundled policy on `[builtin.default-policy]` `params` and an agent of account `a`.
fn world(params: &str) -> World {
    let config = format!("projects = [\"shop\"]\n[harness.scripted]\n[builtin.default-policy]\n{params}");
    let agent = "harness = \"scripted\"\n[native.scripted]\naccount = \"a\"\n";
    let hooks = ExecHooks {
        socket: None,
        bundled: Some(aae_plugins::run),
    };
    let files = [("config.toml", config.as_str()), ("shop/agents/dev/agent.toml", agent)];
    world_with(&files, Arc::new(hooks))
}

/// The 403 of the incident: an `authentication_failed` that a WAF page answered.
fn forbidden() -> Step {
    let mut failure = Failure::new(FailureKind::AuthRequired, Source::Adapter);
    failure.retried_by_harness = true;
    failure.raw = "API Error: 403 <html><title>Attention Required! | Cloudflare</title></html>".into();
    Step::Emit(Normalized::Failure(failure))
}

impl World {
    fn every(&self, run: &str, kind: &str) -> Vec<Value> {
        let all = aae_core::Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        let of = all.into_iter().filter(|e| e["run"] == run && e["type"] == kind);
        of.map(|e| e["data"].clone()).collect()
    }
}

#[tokio::test]
async fn a_passing_403_with_a_lost_or_doubled_end_recovers_by_itself() {
    let w = world("auth_backoff_base_s = 1\nauth_backoff_cap_s = 1\n");
    // The failure doubled and the process exits after it; then the process dies without a word.
    w.driver.script(vec![
        forbidden(),
        forbidden(),
        Step::Exit {
            code: Some(1),
            signal: None,
        },
    ]);
    w.driver.script(vec![forbidden(), Step::Vanish]);
    // The provider answers again: a turn long enough to declare an outcome in.
    let mut turn = hollow_turn();
    turn.insert(1, Step::Wait(Duration::from_secs(2)));
    w.driver.script(turn);
    let run = w.create().await;
    let started = Instant::now();
    let token = loop {
        if let Some(token) = w.driver.token(&run, 3) {
            break token;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no third attempt");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let outcome = Outcome {
        result: Verdict::Pass,
        summary: "done".into(),
        data: None,
    };
    let finish = Msg::Finish {
        run: run.clone(),
        token,
        outcome,
    };
    w.core.send(None, finish).await.unwrap();
    w.until(Some(&run), "run.passed", 0).await;
    // The journal tells it: failure → bounded park → probe by timer, twice, then the pass; no failure counted twice.
    let failed = w.every(&run, "run.attempt_failed");
    assert_eq!(failed.len(), 2);
    assert!(failed.iter().all(|f| f["failure"]["kind"] == "auth_required"));
    assert!(failed[0]["failure"]["raw"].as_str().unwrap().contains("Cloudflare"));
    let parked = w.every(&run, "run.parked");
    assert_eq!(parked.len(), 2);
    assert!(
        parked
            .iter()
            .all(|p| p["gate"]["key"] == ACCOUNT && p["gate"]["until"].is_string())
    );
    let resumed = w.every(&run, "run.resumed");
    assert_eq!(resumed, [json!({"gate_key": ACCOUNT}), json!({"gate_key": ACCOUNT})]);
}

#[tokio::test]
async fn a_lasting_403_keeps_waiting_past_the_attempts_ceiling() {
    // Spec §2.1 item 12: no operator, and the waits on the account gate do not count against the ceiling.
    let w = world("auth_backoff_base_s = 0\nauth_backoff_cap_s = 0\n");
    let past = MAX_ATTEMPTS + 2;
    for _ in 0..past {
        w.driver.script(vec![forbidden()]);
    }
    let mut turn = hollow_turn();
    turn.insert(1, Step::Wait(Duration::from_secs(2)));
    w.driver.script(turn);
    let run = w.create().await;
    let started = Instant::now();
    let token = loop {
        if let Some(token) = w.driver.token(&run, past + 1) {
            break token;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "no attempt past the ceiling"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let outcome = Outcome {
        result: Verdict::Pass,
        summary: "done".into(),
        data: None,
    };
    w.core
        .send(
            None,
            Msg::Finish {
                run: run.clone(),
                token,
                outcome,
            },
        )
        .await
        .unwrap();
    w.until(Some(&run), "run.passed", 0).await;
    let failed = w.every(&run, "run.attempt_failed");
    assert_eq!(failed.len(), past as usize);
    assert!(failed.iter().all(|f| f["failure"]["kind"] == "auth_required"));
}
