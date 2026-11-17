//! The `if` of a step (docs/design/config.md): a step whose `if` does not hold on entry is skipped, with no hook
//! asked and no outcome; `back_from` names the checks that sent the run back. Exec steps run real commands, so real
//! time.

use crate::common;

use aae_core::run::{Create, ExecHooks, Msg};
use common::{World, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// `sh -c <script>` as an exec step named `name`, with `keys` besides.
fn exec(name: &str, script: &str, keys: &str) -> String {
    let script = json!(script);
    format!("[[step]]\nname = \"{name}\"\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {script}]\n{keys}")
}

/// A script that logs `name` and fails on the runs `fails` of it, counted in the file `name`.
fn counted(name: &str, fails: &str) -> String {
    format!(
        "n=$(($(cat {name} 2>/dev/null || echo 0) + 1)); echo $n > {name}; echo {name} >> log; \
         case $n in {fails}) exit 1;; esac"
    )
}

/// A world on real time with the formula `flow`: `head`, then `steps`.
fn flow(head: &str, steps: &[String]) -> World {
    let formula = format!("name = \"flow\"\n{head}{}", steps.concat());
    world_with(&[("shop/formulas/flow.toml", &formula)], Arc::new(ExecHooks::default()))
}

impl World {
    /// Starts `flow` with `inputs` and waits for its end: the kind and data of the event that ended it.
    async fn run_to_end(&self, inputs: Value) -> (String, String, Value) {
        let create = Create {
            project: "shop".into(),
            rig: Some("web".into()),
            formula: "flow".into(),
            inputs: inputs.as_object().cloned().unwrap_or_default(),
            correlation: None,
            depth: 0,
            parent_run: None,
            token: None,
            continue_from: None,
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        let run = reply["run"].as_str().unwrap().to_owned();
        for _ in 0..500 {
            for kind in ["run.passed", "run.failed"] {
                if let Some(data) = self.all(kind).into_iter().find(|d| d["run"]["id"] == run) {
                    return (run, kind.to_owned(), data);
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("`{run}` did not end");
    }

    fn step_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("shop/web/log")).unwrap_or_default()
    }

    /// The events `kind` whose run is at step `step`.
    fn at_step(&self, kind: &str, step: &str) -> Vec<Value> {
        (self.all(kind).into_iter())
            .filter(|d| d["run"]["step"] == step)
            .collect()
    }
}

/// A step waiting for a check runs only in the round that check sent the run back into: skipped in the first round
/// and in a round another check sent it back into. A return to it does not undo its `if`.
#[tokio::test]
async fn a_step_waiting_for_a_check_runs_only_in_the_round_that_check_sent_back() {
    let w = flow(
        "",
        &[
            exec("work", "echo work >> log", ""),
            exec("fix", "echo fix >> log", "if = { back_from = \"review\" }\n"),
            exec("review", &counted("review", "1"), "back_to = \"fix\"\n"),
            exec("lint", &counted("lint", "2"), "back_to = \"work\"\n"),
        ],
    );
    let (_, kind, data) = w.run_to_end(json!({})).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(
        w.step_log(),
        "work\nreview\nlint\nfix\nreview\nlint\nwork\nreview\nlint\n"
    );
    let skipped = w.all("run.step_skipped");
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    for data in &skipped {
        let keys: Vec<_> = data.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["run", "step"]);
        assert_eq!((&data["step"], &data["run"]["step"]), (&json!("fix"), &json!("fix")));
    }
    assert_eq!(w.at_step("run.preparing", "fix").len(), 1, "no hook for a skipped step");
    assert_eq!(w.at_step("run.step_finished", "fix").len(), 1);
    let same = w.all("run.state_changed").into_iter().filter(|d| d["from"] == d["to"]);
    assert_eq!(same.count(), 0);
}

/// An `if` on an input and on the outcome of an earlier step; a skipped step leaves no `AAE_STEP_<NAME>_*`.
#[tokio::test]
async fn a_step_runs_by_an_input_and_by_the_outcome_of_an_earlier_step() {
    let steps = [
        exec("probe", "echo probe >> log", ""),
        exec("ship", "echo ship >> log", "if = { \"inputs.deploy\" = true }\n"),
        exec(
            "after",
            "echo after >> log",
            "if = { \"steps.ship.result\" = \"pass\" }\n",
        ),
        exec("env", "echo ${AAE_STEP_SHIP_RESULT:-none} >> log", ""),
    ];
    let head = "[inputs]\ndeploy = { type = \"boolean\" }\n";
    let w = flow(head, &steps);
    let (_, kind, data) = w.run_to_end(json!({ "deploy": true })).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.step_log(), "probe\nship\nafter\npass\n");
    assert!(w.all("run.step_skipped").is_empty());

    let w = flow(head, &steps);
    let (_, kind, data) = w.run_to_end(json!({})).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.step_log(), "probe\nnone\n");
    let skipped: Vec<_> = w
        .all("run.step_skipped")
        .into_iter()
        .map(|d| d["step"].clone())
        .collect();
    assert_eq!(skipped, ["ship", "after"]);
}

/// A skipped check sends nothing back, and what it left in an earlier round is no longer its outcome.
#[tokio::test]
async fn a_skipped_check_sends_nothing_back_and_loses_its_old_outcome() {
    let w = flow(
        "",
        &[
            exec(
                "work",
                "n=$(($(cat n 2>/dev/null || echo 0) + 1)); echo $n > n; printf $n",
                "",
            ),
            exec(
                "gate",
                "exit 1",
                "back_to = \"work\"\nif = { \"steps.work.data.stdout\" = \"1\" }\n",
            ),
            exec("tail", "echo ${AAE_STEP_GATE_RESULT:-none} >> log", ""),
        ],
    );
    let (run, kind, data) = w.run_to_end(json!({})).await;
    assert_eq!(kind, "run.passed", "{data}");
    let rounds = std::fs::read_to_string(w.dir.path().join("shop/web/n")).unwrap();
    assert_eq!(rounds.trim(), "2", "the gate sent the run back once");
    assert_eq!(w.step_log(), "none\n");
    let skipped = w.all("run.step_skipped");
    assert_eq!(skipped.len(), 1);
    assert_eq!(
        (&skipped[0]["run"]["id"], &skipped[0]["step"]),
        (&json!(run), &json!("gate"))
    );
}

/// When the last step is skipped, the run's outcome is that of the last step that ran.
#[tokio::test]
async fn a_skipped_last_step_leaves_the_outcome_of_the_last_step_that_ran() {
    let steps = [
        exec("work", "true", ""),
        exec("note", "true", "if = { \"inputs.deploy\" = true }\n"),
    ];
    let w = flow("[inputs]\ndeploy = { type = \"boolean\" }\n", &steps);
    let (_, kind, data) = w.run_to_end(json!({})).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(data["outcome"]["summary"], "exit 0", "{data}");
    assert_eq!(data["run"]["step"], "note");
    assert_eq!(data["run"]["attempt"], 1, "no attempt for the skipped step");
}

/// A run whose every step is skipped has no outcome to pass with: it fails.
#[tokio::test]
async fn a_run_whose_every_step_is_skipped_fails() {
    let steps = [exec("note", "true", "if = { \"inputs.deploy\" = true }\n")];
    let w = flow("[inputs]\ndeploy = { type = \"boolean\" }\n", &steps);
    let (_, kind, data) = w.run_to_end(json!({})).await;
    assert_eq!(kind, "run.failed", "{data}");
    assert_eq!(
        (&data["class"], &data["reason"], &data["outcome"]),
        (&json!("infrastructure"), &json!("every step was skipped"), &Value::Null),
        "{data}"
    );
    assert_eq!(w.all("run.started").len(), 1);
    assert!(w.all("run.preparing").is_empty());
}

/// A repeat of a step under `max_attempts` does not ask its `if` again.
#[tokio::test]
async fn a_repeat_of_a_step_does_not_ask_its_if_again() {
    let keys = "max_attempts = 2\nif = { \"run.step_attempt\" = 1 }\n";
    let w = flow("", &[exec("flaky", &counted("flaky", "1"), keys)]);
    let (_, kind, data) = w.run_to_end(json!({})).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.step_log(), "flaky\nflaky\n");
    assert!(w.all("run.step_skipped").is_empty());
}

/// A run continues a session in its first step only, so a formula whose first step has an `if` cannot be continued.
#[tokio::test]
async fn a_formula_whose_first_step_has_an_if_cannot_be_continued() {
    let ai = "[[step]]\nname = \"a\"\nkind = \"ai\"\nagent = \"dev\"\nif = { \"inputs.x\" = true }\n\
              [[step]]\nname = \"b\"\nkind = \"ai\"\nagent = \"dev\"\n";
    let w = flow("[inputs]\nx = { type = \"boolean\" }\n", &[ai.to_owned()]);
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "flow".into(),
        inputs: Default::default(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: Some("run_1".into()),
    };
    let refused = w.core.send(None, Msg::Create(create)).await.unwrap_err().to_string();
    assert!(refused.contains("starts with a step with an `if`"), "{refused}");
    assert!(w.all("run.queued").is_empty());
}
