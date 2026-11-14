//! Criterion 10 for codex: a usage limit with a reset time closes the gate of the account; a second run of the
//! same account does not start while it is closed; at the reset both runs pass without a human.

use super::codex_agent;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_core::parse_rfc3339_ms;
use aae_testkit::openai::{Provider, Reply};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// When the fake's quota resets after the first request.
const RESET: Duration = Duration::from_secs(10);
const WITHIN: Duration = Duration::from_secs(60);
/// `default-policy` keeps the account gate this long past the reset (`quota_margin_s`, set below; default 60).
const MARGIN: Duration = Duration::from_secs(1);

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = format!("{} finish pass --summary built", env.aae.display());
    let plan = vec![Reply::UsageLimit(RESET.as_secs()), Reply::Task(finish)];
    let provider = Provider::start(plan).await;
    let d = Daemon::start(&env, |root| {
        codex_agent(root, "dev", &provider.base_url());
        // A second rig, so only the account gate can hold the second run: a workdir holds one run (spec §9.3).
        let shop = root.join("config/aae/shop");
        write(
            shop.join("aae.toml"),
            format!(
                "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n\
                 [builtin.default-policy]\nquota_margin_s = {}\n",
                MARGIN.as_secs()
            ),
        );
        write(shop.join("api/.keep"), "");
    })
    .await;
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let first = d.create("dev").await;
    // The policy closes the gate after the failure, by a hook: wait for both.
    let seen = d
        .journal_until(WITHIN, |seen| {
            seen.iter().any(|(k, _)| k == "run.attempt_failed") && seen.iter().any(|(k, _)| k == "gate.closed")
        })
        .await;
    let failed = seen.iter().find(|(k, _)| k == "run.attempt_failed").unwrap();
    let failure = &failed.1["data"]["failure"];
    assert_eq!(failure["kind"], "quota_exhausted", "{failure}");
    let gates = d.get("/v1/gates").await;
    let gate = gates
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["key"].as_str().is_some_and(|k| k.starts_with("account:")));
    let gate = gate.unwrap_or_else(|| panic!("no account gate in {gates}"));
    let until = gate["until"].as_str().and_then(parse_rfc3339_ms);
    let until = until.unwrap_or_else(|| panic!("the gate opens at the reset: {gate}"));
    let limited = parse_rfc3339_ms(failed.1["ts"].as_str().unwrap()).unwrap();
    // The fake resets at `floor(request, 1 s) + RESET`, and the request falls between `before` and `limited`:
    // bounds on whole seconds hold whatever the fraction of a second or the delay of the failure.
    let reset = failure["retry_at"].as_str().and_then(parse_rfc3339_ms);
    let reset = reset.unwrap_or_else(|| panic!("the failure names the reset: {failure}"));
    let ms = |d: Duration| i64::try_from(d.as_millis()).unwrap();
    let low = i64::try_from(before.as_secs()).unwrap() * 1000 + ms(RESET);
    assert!(
        (low..=limited + ms(RESET)).contains(&reset),
        "the reset of the provider: {low} <= {reset} <= {limited} + {RESET:?}"
    );
    assert_eq!(
        until,
        reset + ms(MARGIN),
        "the gate holds until the reset and the margin: {gate}"
    );

    let body = json!({ "project": "shop", "rig": "api", "formula": "dev" });
    let second = d.post("/v1/runs", body).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    d.until_state(&first, "passed", RESET + MARGIN + WITHIN).await;
    d.until_state(&second, "passed", WITHIN).await;

    // What the daemon did, from its journal: the times are its own, not when this test happened to look.
    let seen = d
        .journal_until(WITHIN, |seen| {
            seen.iter().filter(|(k, _)| k == "run.passed").count() >= 2
        })
        .await;
    let ts = |e: &Value| parse_rfc3339_ms(e["ts"].as_str().unwrap()).unwrap();
    let opened = seen
        .iter()
        .find(|(k, e)| k == "gate.opened" && e["data"]["key"] == gate["key"])
        .map(|(_, e)| ts(e))
        .unwrap_or_else(|| panic!("the gate never opened: {seen:?}"));
    assert!(opened >= until, "the gate opened at its time: {opened} < {until}");
    let held = seen.iter().any(|(_, e)| {
        e["run"] == second.as_str()
            && (e["data"]["waiting_for"] == gate["key"] || e["data"]["gate"]["key"] == gate["key"])
    });
    assert!(held, "the account gate held the second run: {seen:?}");
    // An attempt launches its agent on the way to `starting`; a parked run waits in `parked`.
    let launches = |run: &str| -> Vec<i64> {
        let to_starting = |e: &Value| e["run"] == run && e["data"]["to"] == "starting";
        let of = seen.iter().filter(|(k, e)| k == "run.state_changed" && to_starting(e));
        of.map(|(_, e)| ts(e)).collect()
    };
    let (first_launches, second_launches) = (launches(&first), launches(&second));
    assert_eq!(
        first_launches.len(),
        2,
        "the first run once before the limit, once after: {seen:?}"
    );
    assert!(
        first_launches[1] >= opened,
        "the first run again only after the reset: {seen:?}"
    );
    assert!(!second_launches.is_empty(), "the second run launched: {seen:?}");
    assert!(
        second_launches.iter().all(|&at| at >= opened),
        "nothing of the second run while the gate was closed: {seen:?}"
    );
    format!(
        "gate {} until {}; opened {} ms after the limit, nothing of the second run before",
        gate["key"],
        gate["until"],
        opened - limited
    )
}
