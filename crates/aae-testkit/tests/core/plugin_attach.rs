//! A plugin's orchestrator hooks go only to the runs whose agent attaches it, with the params in force for that agent
//! (docs/design/contracts.md 3.1).

use crate::common::{hollow_turn, world_with};
use aae_core::{
    BoxFuture, CoreHandle,
    run::{Create, HookRunner, Msg},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// A hook runner that keeps every payload it is given and runs nothing.
#[derive(Default)]
struct Seen(Mutex<Vec<Value>>);

impl HookRunner for Seen {
    fn run<'a>(&'a self, _: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        self.0.lock().unwrap().push(payload.clone());
        Box::pin(async {})
    }
}

const FILES: &[(&str, &str)] = &[
    (
        "config.toml",
        "projects = [\"shop\"]\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n\
         [[plugin]]\npath = \"policy\"\n",
    ),
    (
        "policy/plugin.toml",
        "name = \"policy\"\nversion = \"0.1.0\"\nparams = { level = 1, mode = \"a\" }\n\
         [[hook]]\non = \"run.state_changed\"\ncommand = [\"true\"]\n",
    ),
    (
        "shop/agents/qa/agent.toml",
        "harness = \"scripted\"\nplugins = [{ name = \"policy\", params = { level = 3 } }]\n",
    ),
    ("shop/agents/qa/prompt.template.md", "Review {{ rig.name }}."),
    (
        "shop/formulas/review.toml",
        "name = \"review\"\n[[step]]\nkind = \"ai\"\nagent = \"qa\"\n",
    ),
];

#[tokio::test(start_paused = true)]
async fn a_hook_goes_only_to_the_runs_of_an_agent_that_attaches_its_plugin() {
    let seen = Arc::new(Seen::default());
    let w = world_with(FILES, seen.clone());
    w.driver.script(hollow_turn());
    w.driver.script(hollow_turn());
    let build = w.create().await;
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "review".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let review = reply["run"].as_str().unwrap().to_owned();
    for run in [&build, &review] {
        w.until(Some(run), "run.failed", 0).await;
    }

    let seen = seen.0.lock().unwrap();
    let of = |run: &str| -> Vec<&Value> { seen.iter().filter(|p| p["event"]["run"] == run).collect() };
    assert!(
        of(&build).is_empty(),
        "the agent of `build` attaches no plugin: {:?}",
        of(&build)
    );
    let states: Vec<&Value> = of(&review).iter().map(|p| &p["event"]["data"]["to"]).collect();
    assert!(states.contains(&&json!("running")), "{states:?}");
    for payload in of(&review) {
        let hooks = payload["hooks"].as_array().unwrap();
        assert_eq!(hooks.len(), 1, "{payload}");
        assert_eq!(hooks[0]["params"], json!({ "level": 3, "mode": "a" }));
    }
}
