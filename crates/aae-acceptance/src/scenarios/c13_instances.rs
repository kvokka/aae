//! Criterion 13: two codex agents with the same process part share one app-server, an agent with another gets a
//! second; no instance before the first run needs one, and each stops after its idle time without sessions.

use super::codex_agent;
use crate::{Env, daemon::Daemon};
use aae_testkit::openai::{Provider, Reply};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);
/// codex's `instance_idle` (contracts 4.5) and the slack to see the stop.
const IDLE: Duration = Duration::from_secs(60);

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = format!("{} finish pass --summary built", env.aae.display());
    let shared = Provider::start(vec![Reply::Task(finish.clone())]).await;
    let other = Provider::start(vec![Reply::Task(finish)]).await;
    let d = Daemon::start(&env, |root| {
        codex_agent(root, "dev", &shared.base_url());
        codex_agent(root, "ops", &shared.base_url());
        // Another provider is another process part of codex (contracts 4.6).
        codex_agent(root, "qa", &other.base_url());
    })
    .await;
    let instances = |seen: &[(String, Value)], kind: &str| -> Vec<String> {
        let of = seen
            .iter()
            .filter(|(k, e)| k == kind && e["data"]["instance"]["harness"] == "codex");
        of.map(|(_, e)| e["data"]["instance"]["id"].as_str().unwrap().to_owned())
            .collect()
    };
    // Lazy: the daemon idles a while first, and no instance starts before a run needs one.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let runs = [d.create("dev").await, d.create("ops").await, d.create("qa").await];
    for run in &runs {
        d.until_state(run, "passed", WITHIN).await;
    }
    let seen = d
        .journal_until(WITHIN + IDLE, |seen| {
            instances(seen, "instance.stopped").len() >= instances(seen, "instance.ready").len()
                && !instances(seen, "instance.ready").is_empty()
        })
        .await;
    let first = |kind: &str| seen.iter().position(|(k, _)| k == kind);
    assert!(
        first("run.queued") < first("instance.starting"),
        "an instance before a run: {seen:?}"
    );
    let ready = instances(&seen, "instance.ready");
    assert_eq!(ready.len(), 2, "one app-server per process part: {ready:?}");
    let stopped: Vec<&Value> = seen
        .iter()
        .filter(|(k, _)| k == "instance.stopped")
        .map(|(_, e)| e)
        .collect();
    assert!(stopped.iter().all(|e| e["data"]["reason"] == "idle"), "{stopped:?}");
    format!("3 runs on 2 app-servers ({}); both stopped as idle", ready.join(", "))
}
