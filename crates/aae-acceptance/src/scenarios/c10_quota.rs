//! Criterion 10 for codex: a usage limit with a reset time closes the gate of the account; a second run of the
//! same account does not start while it is closed; at the reset both runs pass without a human.

use super::codex_agent;
use crate::{Env, daemon::Daemon};
use aae_testkit::openai::{Provider, Reply};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// When the fake's quota resets after the first request.
const RESET: Duration = Duration::from_secs(10);
const WITHIN: Duration = Duration::from_secs(60);
/// `default-policy` keeps the account gate a minute past the reset (`quota_margin_s`).
const MARGIN: Duration = Duration::from_secs(60);

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = format!("{} finish pass --summary built", env.aae.display());
    let plan = vec![Reply::UsageLimit(RESET.as_secs()), Reply::Task(finish)];
    let provider = Provider::start(plan).await;
    let d = Daemon::start(&env, |root| codex_agent(root, "dev", &provider.base_url())).await;
    let first = d.create("dev").await;
    let failed = d
        .journal_until(WITHIN, |seen| seen.iter().any(|(k, _)| k == "run.attempt_failed"))
        .await;
    let limited = Instant::now();
    let failure = &failed.last().unwrap().1["data"]["failure"];
    assert_eq!(failure["kind"], "quota_exhausted", "{failure}");
    let gates = d.get("/v1/gates").await;
    let gate = gates
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["key"].as_str().is_some_and(|k| k.starts_with("account:")));
    let gate = gate.unwrap_or_else(|| panic!("no account gate in {gates}"));
    assert!(gate["until"].is_string(), "the gate opens at the reset: {gate}");

    let second = d.create("dev").await;
    tokio::time::sleep(RESET / 2).await;
    assert!(limited.elapsed() < RESET, "checked before the reset");
    let view = d.get(&format!("/v1/runs/{second}")).await;
    assert_eq!(
        view["state"], "queued",
        "the second run of the account waits for the gate: {view}"
    );
    assert_eq!(
        provider.requests().len(),
        1,
        "nothing reached the provider while the gate was closed"
    );

    d.until_state(&first, "passed", RESET + MARGIN + WITHIN).await;
    d.until_state(&second, "passed", WITHIN).await;
    let after = limited.elapsed();
    assert!(
        after + Duration::from_secs(1) >= RESET,
        "passed only after the reset: {after:?}"
    );
    format!(
        "gate {} until {}; both runs passed {:.1} s after the limit",
        gate["key"],
        gate["until"],
        after.as_secs_f64()
    )
}
