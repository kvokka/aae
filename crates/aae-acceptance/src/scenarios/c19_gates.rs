//! Criterion 19: gates closed by the operator on the orchestrator, a project, an agent and a bead stop dispatch,
//! survive a `kill -9` of the daemon and stay closed until opened by hand; each one holds its runs on its own.

use super::{br, claude_agent, exec_formula, tracker};
use crate::{Env, daemon::Daemon};
use aae_testkit::anthropic::{Provider, Reply};
use serde_json::json;
use std::{sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = format!("{} finish pass --summary built", env.aae.display());
    let provider = Provider::start(vec![Reply::Bash(finish), Reply::Text("Done.".into())]).await;
    let mut d = Daemon::start(&env, |root| {
        claude_agent(root, "dev", &provider.base_url());
        exec_formula(root, "check", "", "\"true\"");
        exec_formula(root, "fix", "bead = { type = \"bead\", required = true }", "\"true\"");
        tracker(root);
    })
    .await;
    let bead = br(d.dir.path(), &["create", "Fix the cart", "--silent"]);
    let close = async |key: &str| {
        let gate = json!({ "key": key, "reason": "maintenance" });
        d.post("/v1/gates", gate).await.unwrap();
    };
    close("orchestrator").await;
    let exec = d.create("check").await;
    let agent = d.create("dev").await;
    let body = json!({ "project": "shop", "rig": "web", "formula": "fix", "inputs": { "bead": bead } });
    let on_bead = d.post("/v1/runs", body).await.unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    // The agent key carries the qualified agent name the run reports.
    let qualified = d.get(&format!("/v1/runs/{agent}")).await["agent"].clone();
    let keys = [
        "orchestrator".to_owned(),
        "project:shop".to_owned(),
        format!("agent:{}", qualified.as_str().expect("an agent")),
        format!("bead:shop/{bead}"),
    ];
    for key in &keys[1..] {
        close(key).await;
    }

    d.restart().await;
    let gates = d.get("/v1/gates").await;
    let closed: Vec<_> = gates.as_array().unwrap().iter().map(|g| &g["key"]).collect();
    assert_eq!(closed.len(), 4, "every gate survives the restart: {gates}");
    assert!(
        gates.as_array().unwrap().iter().all(|g| g["closed_by"] == "operator"),
        "{gates}"
    );
    for run in [&exec, &agent, &on_bead] {
        d.until_state(run, "queued", WITHIN).await;
    }

    // Each opening lets through only the runs no other closed gate holds. The reply comes after the transaction
    // that opened the gate and dispatched what it could has committed, so a run still `queued` then is held.
    let open = |key: &str| d.post("/v1/gates/open", json!({ "key": key }));
    open(&keys[0]).await.unwrap();
    for run in [&exec, &agent, &on_bead] {
        d.until_state(run, "queued", WITHIN).await;
    }
    open(&keys[1]).await.unwrap();
    d.until_state(&exec, "passed", WITHIN).await;
    assert_eq!(d.get(&format!("/v1/runs/{agent}")).await["state"], "queued");
    assert_eq!(d.get(&format!("/v1/runs/{on_bead}")).await["state"], "queued");
    open(&keys[2]).await.unwrap();
    d.until_state(&agent, "passed", WITHIN).await;
    assert_eq!(d.get(&format!("/v1/runs/{on_bead}")).await["state"], "queued");
    assert_eq!(
        d.get("/v1/gates").await[0]["key"],
        json!(keys[3]),
        "automation opened nothing"
    );
    open(&keys[3]).await.unwrap();
    d.until_state(&on_bead, "passed", WITHIN).await;
    assert_eq!(d.get("/v1/gates").await, json!([]));
    String::new()
}
