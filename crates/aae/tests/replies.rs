//! The reply schemas of the OpenAPI document are written by hand: the replies of a real daemon must fit them.

mod common;

use common::{Daemon, schema::check};
use serde_json::json;
use std::fs;

#[tokio::test]
async fn the_replies_fit_their_schemas() {
    let d = Daemon::start(|dir| {
        let aae = dir.join("config/aae");
        fs::write(
            aae.join("config.toml"),
            "projects = [\"shop\"]\n[harness.claude]\nbinary = \"true\"\n",
        )
        .unwrap();
        fs::create_dir_all(aae.join("shop/agents/dev")).unwrap();
        fs::write(aae.join("shop/agents/dev/agent.toml"), "harness = \"claude\"\n").unwrap();
        let project = "[rigs.web]\npath = \"web\"\n\
            [[trigger]]\nname = \"sweep\"\nevery = \"24h\"\nrun = \"slow\"\nrig = \"web\"\n";
        fs::write(aae.join("shop/aae.toml"), project).unwrap();
    })
    .await;
    // A run that ends before `slow` holds the rig.
    let passed = d.create("check", None).await;
    let view = d.until_state(&passed, "passed").await;
    assert!(!view["attempts"].as_array().unwrap().is_empty(), "{view}");
    assert!(
        view["outcome"]["data"]["stdout"].is_string(),
        "the exec step's output: {view}"
    );
    check("get", "/v1/runs/{id}", &view);
    check("get", "/v1/runs", &d.get("/v1/runs").await);
    let run = d.post(
        "/v1/runs",
        json!({ "project": "shop", "rig": "web", "formula": "slow" }),
        None,
    );
    let run = run.await.unwrap();
    check("post", "/v1/runs", &run);
    let run = run["run"].as_str().unwrap().to_owned();
    let trigger = json!({ "project": "shop", "rig": "web", "trigger": "sweep" });
    let gate = json!({ "key": "rig:shop/web", "until": "2999-01-01T00:00:00Z", "reason": "test" });
    for (path, body) in [
        ("/v1/triggers/fire", trigger.clone()),
        ("/v1/triggers/disable", trigger.clone()),
        ("/v1/triggers/enable", trigger),
        ("/v1/config/check", json!(null)),
        ("/v1/config/reload", json!(null)),
        ("/v1/gates", gate),
    ] {
        check("post", path, &d.post(path, body, None).await.unwrap());
    }
    let finish = json!({ "token": "nope", "outcome": { "result": "pass", "summary": "done" } });
    let finished = d.post(&format!("/v1/runs/{run}/finish"), finish, None).await.unwrap();
    assert_eq!(finished["answer"], "invalid");
    check("post", "/v1/runs/{id}/finish", &finished);
    let explain = d.get("/v1/config/explain?project=shop&agent=dev").await;
    check("get", "/v1/config/explain", &explain);
    for path in [
        "/v1/daemon",
        "/v1/gates",
        "/v1/triggers",
        "/v1/projects",
        "/v1/agents",
        "/v1/formulas",
        "/v1/packs",
        "/v1/plugins",
    ] {
        check("get", path, &d.get(path).await);
    }
    // Exec runs start no instance: the list stays empty here; `codex.rs` checks a live one against its schema.
    assert_eq!(d.get("/v1/instances").await, json!([]));
    // Last: the daemon stops after the drain.
    let drained = d.post("/v1/daemon/drain?timeout_s=1", json!(null), None).await.unwrap();
    check("post", "/v1/daemon/drain", &drained);
}
