//! Criterion 31 on a real daemon, with no model: formula `ship` of project `shop` has a `formula` step on the
//! internal `child`, an exec step that prints its inputs. The child is a child run of the parent, inherits only the
//! inputs it declares (`goal`, not `scratch`) and gets `note` from the step; its outcome, pass or fail, is the
//! parent's. A direct start of `child` is refused; the API marks it `internal`, `loop-manager formula list` shows it only with
//! `--all`. Projects `shop` and `lab` each have a formula `same`: each start runs its own project's.

use crate::{
    Env,
    daemon::{Daemon, write},
};
use loop_manager_front::client::ClientError;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(60);

/// Prints the inputs it got, `scratch` as `-` when absent; fails with 3 on goal `bad`.
const CHILD: &str = "name = \"child\"\ninternal = true\n[inputs]\ngoal = { type = \"string\", required = true }\n\
    note = { type = \"string\" }\n[[step]]\nkind = \"exec\"\ntimeout = \"30s\"\n\
    command = [\"sh\", \"-c\", \"echo goal=$LOOP_MANAGER_INPUT_GOAL note=$LOOP_MANAGER_INPUT_NOTE scratch=${LOOP_MANAGER_INPUT_SCRATCH:--}; \
    test \\\"$LOOP_MANAGER_INPUT_GOAL\\\" != bad || exit 3\"]\n";

/// `scratch` is the parent's alone: the child does not declare it, so it does not inherit it.
const SHIP: &str = "name = \"ship\"\n[inputs]\ngoal = { type = \"string\", required = true }\n\
    scratch = { type = \"string\" }\n[[step]]\nname = \"sub\"\nkind = \"formula\"\nformula = \"child\"\n\
    inputs = { note = \"for {{ inputs.goal }}\" }\n";

/// Formula `name`: prints `project`.
fn echo(name: &str, project: &str) -> String {
    format!("name = \"{name}\"\n[[step]]\nkind = \"exec\"\ntimeout = \"30s\"\ncommand = [\"echo\", \"{project}\"]\n")
}

async fn start(d: &Daemon, project: &str, formula: &str, inputs: Value) -> Result<String, ClientError> {
    let body = json!({ "project": project, "formula": formula, "inputs": inputs });
    let reply = d.post("/v1/runs", body).await?;
    Ok(reply["run"].as_str().expect("a run").to_owned())
}

fn stdout(view: &Value) -> &str {
    view["outcome"]["data"]["stdout"].as_str().unwrap_or_default().trim()
}

/// Runs `ship` on `goal` to `state`: the stdout of its child, after checking the child is its child run and its
/// outcome the parent's.
async fn ship(d: &Daemon, goal: &str, state: &str) -> String {
    let run = start(d, "shop", "ship", json!({ "goal": goal, "scratch": "s" }))
        .await
        .unwrap();
    let view = d.until_state(&run, state, WITHIN).await;
    let kids = d.get(&format!("/v1/runs?parent={run}")).await;
    let kids = kids.as_array().expect("runs");
    assert_eq!(kids.len(), 1, "one child run: {kids:?}");
    let kid = kids[0]["id"].as_str().expect("an id");
    let kid = d.get(&format!("/v1/runs/{kid}")).await;
    assert_eq!(
        (&kid["formula"], &kid["state"], &kid["parent_run"], &kid["depth"]),
        (&json!("shop.child"), &json!(state), &json!(run), &json!(1)),
        "{kid}"
    );
    let note = format!("for {goal}");
    assert_eq!(kid["inputs"], json!({ "goal": goal, "note": note }), "{kid}");
    assert_eq!(
        view["outcome"], kid["outcome"],
        "the child's outcome is the step's: {view}"
    );
    stdout(&kid).to_owned()
}

/// The `formula` names `loop-manager formula list --project shop` gives, with `extra` args.
fn listed(d: &Daemon, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["--json", "formula", "list", "--project", "shop"];
    args.extend(extra);
    let out = d.cli(&args);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let list: Value = serde_json::from_slice(&out.stdout).expect("json");
    let names = list.as_array().expect("formulas").iter();
    names
        .map(|f| f["formula"].as_str().unwrap_or_default().to_owned())
        .collect()
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let d = Daemon::start(&env, |root| {
        let loop_manager = root.join("config/loop-manager");
        write(loop_manager.join("config.toml"), "projects = [\"shop\", \"lab\"]\n");
        write(loop_manager.join("lab/loop-manager.toml"), "");
        write(loop_manager.join("shop/formulas/child.toml"), CHILD);
        write(loop_manager.join("shop/formulas/ship.toml"), SHIP);
        write(loop_manager.join("shop/formulas/same.toml"), echo("same", "shop"));
        write(loop_manager.join("lab/formulas/same.toml"), echo("same", "lab"));
        write(loop_manager.join("lab/formulas/solo.toml"), echo("solo", "lab"));
    })
    .await;

    // Inherited `goal`, the step's `note`, nothing of the parent's `scratch`; a failed child fails the parent.
    assert_eq!(ship(&d, "g", "passed").await, "goal=g note=for g scratch=-");
    assert_eq!(ship(&d, "bad", "failed").await, "goal=bad note=for bad scratch=-");

    // Internal: a direct start is refused, the API marks it, the list shows it only with `--all`.
    let direct = start(&d, "shop", "child", json!({ "goal": "g" })).await;
    let Err(ClientError::Api { status: 409, error }) = direct else {
        panic!("a direct start of an internal formula: {direct:?}");
    };
    let message = error["message"].as_str().unwrap_or_default();
    assert!(message.contains("is internal"), "{error}");
    let formulas = d.get("/v1/formulas?project=shop").await;
    let internal = |name: &str| {
        let mut formulas = formulas.as_array().expect("formulas").iter();
        formulas.find(|f| f["formula"] == name).map(|f| f["internal"].clone())
    };
    assert_eq!(
        (internal("child"), internal("ship")),
        (Some(json!(true)), Some(json!(false))),
        "{formulas}"
    );
    assert!(!listed(&d, &[]).contains(&"child".to_owned()), "hidden without --all");
    assert!(listed(&d, &["--all"]).contains(&"child".to_owned()), "shown with --all");

    // One name in two projects: each start runs its own project's; a formula of `lab` is not one of `shop`.
    for project in ["shop", "lab"] {
        let run = start(&d, project, "same", json!({})).await.unwrap();
        let view = d.until_state(&run, "passed", WITHIN).await;
        assert_eq!(stdout(&view), project, "{view}");
    }
    let solo = start(&d, "shop", "solo", json!({})).await;
    assert!(solo.is_err(), "a formula of lab started in shop: {solo:?}");
    "ship ran internal child as its child run: inherited goal, the step's note, not scratch; its pass and fail were \
     ship's; a direct start was refused 409, the API marked it internal, formula list showed it only with --all; \
     `same` of shop and of lab each ran its own, lab's `solo` was unknown to shop"
        .into()
}
