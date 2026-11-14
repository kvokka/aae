//! The reply schemas of the OpenAPI document are written by hand: the replies of a real daemon must fit them.

mod common;

use common::Daemon;
use serde_json::{Value, json};
use std::fs;

/// The schema of the 200 reply of `method path` in the generated document.
fn reply_schema(doc: &Value, method: &str, path: &str) -> Value {
    let schema = &doc["paths"][path][method]["responses"]["200"]["content"]["application/json"]["schema"];
    assert!(!schema.is_null(), "{method} {path}: no 200 reply schema");
    schema.clone()
}

/// The JSON Schema type of `value`.
fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `reply` has a type the schema allows, every key of it is a declared property and every required property is
/// there, down nested objects.
fn fits(doc: &Value, schema: &Value, reply: &Value, at: &str) {
    let schema = match schema["$ref"].as_str() {
        Some(r) => &doc["components"]["schemas"][r.trim_start_matches("#/components/schemas/")],
        None => schema,
    };
    // An optional reference: `null`, or a value that fits one of the other branches.
    if let Some(branches) = schema["oneOf"].as_array() {
        let null = branches.iter().any(|b| b["type"] == "null");
        if !(reply.is_null() && null) {
            let other: Vec<_> = branches.iter().filter(|b| b["type"] != "null").collect();
            assert_eq!(other.len(), 1, "{at}: a oneOf this check does not read");
            fits(doc, other[0], reply, at);
        }
        return;
    }
    let types: Vec<&str> = match &schema["type"] {
        Value::String(t) => vec![t],
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    let found = type_of(reply);
    let allowed = types.contains(&found) || (found == "integer" && types.contains(&"number"));
    assert!(
        types.is_empty() || allowed,
        "{at}: a {found}, the schema says {types:?}"
    );
    if let (Some(items), Some(reply)) = (schema.get("items"), reply.as_array()) {
        reply.iter().for_each(|r| fits(doc, items, r, at));
        return;
    }
    let (Some(properties), Some(reply)) = (schema["properties"].as_object(), reply.as_object()) else {
        return;
    };
    for (key, value) in reply {
        let property = properties.get(key);
        assert!(property.is_some(), "{at}: `{key}` is not in the schema");
        fits(doc, property.unwrap(), value, &format!("{at}.{key}"));
    }
    for required in schema["required"].as_array().into_iter().flatten() {
        assert!(
            reply.contains_key(required.as_str().unwrap()),
            "{at}: `{required}` is missing"
        );
    }
}

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
    let doc = serde_json::to_value(aae_front::api::openapi()).unwrap();
    let check = |method: &str, path: &str, reply: &Value| {
        let at = format!("{} {path}", method.to_uppercase());
        let empty = reply.as_array().is_some_and(Vec::is_empty);
        assert!(!empty, "{at}: an empty list checks nothing");
        fits(&doc, &reply_schema(&doc, method, path), reply, &at);
    };

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
    // Exec runs start no instance: the list stays empty here.
    assert_eq!(d.get("/v1/instances").await, json!([]));
    // Last: the daemon stops after the drain.
    let drained = d.post("/v1/daemon/drain?timeout_s=1", json!(null), None).await.unwrap();
    check("post", "/v1/daemon/drain", &drained);
}
