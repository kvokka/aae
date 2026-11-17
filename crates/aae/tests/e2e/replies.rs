//! The reply schemas of the OpenAPI document are written by hand: the replies of a real daemon must fit them.

use crate::common::{Daemon, schema::check};
use aae_front::client::ClientError;
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
    // A finished run, for the shape of its view.
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
    // A formula without a rig is explained in the project root.
    let explain = d.get("/v1/config/explain?project=shop&formula=slow").await;
    check("get", "/v1/config/explain", &explain);
    // The root stands apart from the rigs; `dir` is the directory of `aae.toml`, the root's default path.
    let projects = d.get("/v1/projects").await;
    let shop = &projects[0];
    assert!(shop["dir"].as_str().unwrap().ends_with("config/aae/shop"), "{projects}");
    assert_eq!(
        (&shop["root"]["name"], &shop["root"]["path"]),
        (&"root".into(), &shop["dir"]),
        "{projects}"
    );
    assert_eq!(shop["rigs"].as_array().unwrap().len(), 1, "only `web`: {projects}");
    assert_eq!(
        (&shop["rigs"][0]["prefix"], &shop["root"]["prefix"]),
        (&"w".into(), &json!(null)),
        "{projects}"
    );
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

/// The errors a real daemon answers are the statuses the document declares for the operation, and the description
/// of that status names the error's code.
#[tokio::test]
async fn the_errors_answered_are_declared_with_their_codes() {
    let d = Daemon::start(|_| {}).await;
    let doc = serde_json::to_value(aae_front::api::openapi()).unwrap();
    let checked = d.create("check", Some("k")).await;
    let config = d.dir.path().join("config/aae/config.toml");
    let reload = || {
        fs::write(&config, "projects = [\"shop\"\n").unwrap();
        None
    };
    let run = json!({ "project": "shop", "rig": "web", "formula": "slow" });
    let nope = json!({ "project": "shop", "rig": "web", "formula": "nope" });
    let no_rig = json!({ "project": "shop", "rig": "nope", "formula": "check" });
    let trigger = json!({ "project": "shop", "rig": "web", "trigger": "nope" });
    let cancel_checked = format!("/v1/runs/{checked}/cancel");
    d.until_state(&checked, "passed").await;
    // A request naming a thing there is none of is 404; one the thing that is refuses is 409.
    let cases = [
        ("GET", "/v1/runs", "/v1/runs?state=bogus", None, None, 400),
        ("GET", "/v1/runs/{id}", "/v1/runs/run_999", None, None, 404),
        (
            "POST",
            "/v1/runs/{id}/cancel",
            "/v1/runs/run_999/cancel",
            Some(json!({})),
            None,
            404,
        ),
        (
            "POST",
            "/v1/runs/{id}/cancel",
            cancel_checked.as_str(),
            Some(json!({})),
            None,
            409,
        ),
        (
            "POST",
            "/v1/instances/{id}/stop",
            "/v1/instances/nope/stop",
            None,
            None,
            404,
        ),
        (
            "POST",
            "/v1/triggers/fire",
            "/v1/triggers/fire",
            Some(trigger),
            None,
            404,
        ),
        ("GET", "/v1/agents", "/v1/agents?project=nope", None, None, 404),
        ("POST", "/v1/runs", "/v1/runs", Some(nope), None, 404),
        ("POST", "/v1/runs", "/v1/runs", Some(no_rig), None, 404),
        ("POST", "/v1/runs", "/v1/runs", Some(run), Some("k"), 422),
        ("POST", "/v1/config/reload", "/v1/config/reload", reload(), None, 400),
    ];
    for (method, op, path, body, key, expected) in cases {
        let at = format!("{method} {path}");
        let (status, error) = match d.client.request(method, path, body.as_ref(), key).await {
            Err(ClientError::Api { status, error }) => (status, error),
            other => panic!("{at}: not an error reply: {other:?}"),
        };
        assert_eq!(status, expected, "{at}: {error}");
        let declared = &doc["paths"][op][method.to_lowercase()]["responses"][status.to_string()]["description"];
        let code = format!("`{}`", error["code"].as_str().unwrap());
        assert!(
            declared.as_str().is_some_and(|d| d.contains(&code)),
            "{at}: answered {status} {code}, declared {declared}"
        );
    }
    assert!(d.get("/v1/doctor").await.is_array());
    // The query is part of the request a key stands for: a repeat with another timeout is not the same drain.
    let slow = d.create("slow", None).await;
    d.until_state(&slow, "running").await;
    let drain = async |timeout: u32| {
        let path = format!("/v1/daemon/drain?timeout_s={timeout}");
        d.client.request("POST", &path, None, Some("d")).await
    };
    drain(600).await.unwrap();
    match drain(1).await {
        Err(ClientError::Api { status: 422, .. }) => {}
        other => panic!("a drain with another timeout under the same key: {other:?}"),
    }
}
