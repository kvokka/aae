//! A start fills an input it is not given with the value the config gives (docs/design/config.md "Formula",
//! decision 130): the caller's value — the start's, or that of a `formula` step — beats the project's, which beats
//! the orchestrator's.

use crate::common;

use common::{World, world};
use loop_manager_core::run::{Create, Msg, Reader};
use serde_json::{Value, json};

const CONFIG: (&str, &str) = (
    "config.toml",
    "projects = [\"shop\"]\nbuiltin = { default-policy = { enabled = false } }\n[inputs]\ngoal = \"top\"\n\
     note = \"top\"\nlevel = \"3\"\n[harness.scripted]\n",
);

const PROJECT: (&str, &str) = (
    "shop/loop-manager.toml",
    "[inputs]\nnote = \"own\"\n[rigs.web]\npath = \"web\"\n",
);

const CHILD: (&str, &str) = (
    "shop/formulas/child.toml",
    "name = \"child\"\n[inputs]\ngoal = { type = \"string\", required = true }\nnote = { type = \"string\" }\n\
     level = { type = \"integer\" }\n[[step]]\nkind = \"ai\"\nagent = \"dev\"\n",
);

const SHIP: (&str, &str) = (
    "shop/formulas/ship.toml",
    "name = \"ship\"\n[inputs]\ngoal = { type = \"string\", required = true }\n\
     [[step]]\nkind = \"formula\"\nformula = \"child\"\ninputs = { note = \"for {{ inputs.goal }}\" }\n",
);

/// Starts `formula` with `inputs` and returns the run it made with its inputs.
async fn started(w: &World, formula: &str, inputs: Value) -> (String, Value) {
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: formula.into(),
        inputs: inputs.as_object().cloned().unwrap(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    (run.clone(), inputs_of(w, &run))
}

fn inputs_of(w: &World, run: &str) -> Value {
    let reader = Reader::open(&w.dir.path().join("state.db")).unwrap();
    reader.run(run).unwrap().unwrap()["inputs"].clone()
}

#[tokio::test(start_paused = true)]
async fn the_caller_beats_the_project_which_beats_the_orchestrator() {
    let w = world(&[CONFIG, PROJECT, CHILD, SHIP]);
    let all = json!({ "goal": "top", "note": "own", "level": 3 });
    assert_eq!(
        started(&w, "child", json!({})).await.1,
        all,
        "the project over the orchestrator"
    );
    let null = json!({ "goal": null, "level": "5" });
    let given = json!({ "goal": "top", "note": "own", "level": 5 });
    assert_eq!(started(&w, "child", null).await.1, given, "a null is no value");
    let given = json!({ "goal": "g", "note": "n", "level": 1 });
    assert_eq!(
        started(&w, "child", given.clone()).await.1,
        given,
        "the start over the config"
    );
    // The step's `note` and the inherited `goal` beat the config; the child takes `level` from it.
    let (ship, inputs) = started(&w, "ship", json!({ "goal": "g" })).await;
    assert_eq!(inputs, json!({ "goal": "g" }));
    // The fifth run queued: three of `child`, `ship`, then its child.
    let child = w.until(None, "run.queued", 4).await;
    assert_eq!(child["run"]["parent_run"], ship);
    let inputs = inputs_of(&w, child["run"]["id"].as_str().unwrap());
    assert_eq!(inputs, json!({ "goal": "g", "note": "for g", "level": 3 }));
}
