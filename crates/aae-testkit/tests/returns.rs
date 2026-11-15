//! A check sends the run back to an earlier step with its remarks, for at most the formula's rounds (spec §9.1,
//! §15 criterion 25, core part). Exec steps run real commands, so real time.

mod common;

use aae_core::{
    driver::{Normalized, SessionState, StopReason},
    run::{Create, ExecHooks, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{World, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// `sh -c <script>` as an exec step named `name`, a check when it goes `back` to a step.
fn exec(name: &str, script: &str, back: Option<&str>) -> String {
    let back = back.map(|to| format!("back_to = \"{to}\"\n")).unwrap_or_default();
    let script = json!(script);
    format!("[[step]]\nname = \"{name}\"\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {script}]\n{back}")
}

/// A script that fails with `remark` on its first `fails` runs, counted in the file `name`.
fn failing(name: &str, fails: u32, remark: &str) -> String {
    format!(
        "n=$(($(cat {name} 2>/dev/null || echo 0) + 1)); echo $n > {name}; echo {name} >> log; [ $n -gt {fails} ] || {{ echo '{remark}'; exit 1; }}"
    )
}

fn flow(head: &str, steps: &[String], extra: &[(&str, &str)]) -> World {
    let formula = format!("name = \"flow\"\n{head}{}", steps.concat());
    let mut files = vec![("shop/formulas/flow.toml", formula.as_str())];
    files.extend_from_slice(extra);
    world_with(&files, Arc::new(ExecHooks::default()))
}

impl World {
    async fn start_flow(&self) -> String {
        let create = Create {
            project: "shop".into(),
            rig: Some("web".into()),
            formula: "flow".into(),
            inputs: serde_json::Map::new(),
            correlation: None,
            depth: 0,
            parent_run: None,
            token: None,
            continue_from: None,
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("shop/web/log")).unwrap_or_default()
    }

    /// The kind and data of the event that ended `run`; each attempt of a prompt step declares `pass` on the way,
    /// but the attempts `fails` declare `fail`.
    async fn end(&self, run: &str, fails: &[u32]) -> (String, Value) {
        let mut done = Vec::new();
        for _ in 0..500 {
            for kind in ["run.passed", "run.failed"] {
                if let Some(data) = self.all(kind).into_iter().find(|d| d["run"]["id"] == run) {
                    return (kind.to_owned(), data);
                }
            }
            for attempt in 1..10 {
                let Some(token) = self.driver.token(run, attempt).filter(|_| !done.contains(&attempt)) else {
                    continue;
                };
                let outcome = Outcome {
                    result: if fails.contains(&attempt) {
                        Verdict::Fail
                    } else {
                        Verdict::Pass
                    },
                    summary: "planned".into(),
                    data: None,
                };
                let msg = Msg::Finish {
                    run: run.into(),
                    token,
                    outcome,
                };
                self.core.send(None, msg).await.unwrap();
                done.push(attempt);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("`{run}` did not end");
    }
}

/// Checks in a row all run; both remarks go back together, and the next round passes.
#[tokio::test]
async fn failed_checks_send_the_run_back_with_their_remarks_and_the_next_round_passes() {
    let steps = [
        exec("plan", "printf 'plan [%s]\\n' \"$AAE_REMARKS\" >> log", None),
        exec("work", "echo work >> log", None),
        exec("totals", &failing("totals", 1, "fix the totals"), Some("work")),
        exec("lint", &failing("lint", 1, "line too long"), Some("plan")),
        exec("post", "echo post >> log", None),
    ];
    let w = flow("", &steps, &[]);
    let run = w.start_flow().await;
    let (kind, data) = w.end(&run, &[]).await;
    assert_eq!(kind, "run.passed", "{data}");
    let remarks = "totals: exit 1\nfix the totals\n\nlint: exit 1\nline too long";
    assert_eq!(
        w.log(),
        format!("plan []\nwork\ntotals\nlint\nplan [{remarks}]\nwork\ntotals\nlint\npost\n"),
        "back to the earliest step named"
    );
}

/// A check that keeps failing fails the run after exactly the formula's rounds, its remarks in the result; a step
/// that is no check fails the run at once.
#[tokio::test]
async fn a_check_that_keeps_failing_fails_the_run_after_the_last_round() {
    let steps = [
        exec("plan", "echo plan >> log", None),
        exec("check", &failing("check", 99, "still red"), Some("plan")),
    ];
    let w = flow("rounds = 2\n", &steps, &[]);
    let run = w.start_flow().await;
    let (kind, data) = w.end(&run, &[]).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("agent")),
        "{data}"
    );
    assert_eq!(data["reason"], "still failing after 2 rounds: check: exit 1\nstill red");
    assert_eq!(w.log(), "plan\ncheck\nplan\ncheck\n");
}

/// The step the run went back to continues its session (spec §9.3), and its prompt shows the remarks; the first
/// round shows none.
#[tokio::test]
async fn the_step_gone_back_to_sees_the_remarks_in_its_prompt() {
    let steps = [
        "[[step]]\nname = \"plan\"\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"plan.md\"\n".to_owned(),
        exec("check", &failing("check", 1, "add tests"), Some("plan")),
    ];
    let w = flow(
        "",
        &steps,
        &[("shop/formulas/plan.md", "Plan. Remarks: [{{ remarks }}]")],
    );
    for _ in 0..2 {
        w.driver.script(turn());
    }
    let run = w.start_flow().await;
    let (kind, data) = w.end(&run, &[]).await;
    assert_eq!(kind, "run.passed", "{data}");
    let sent: Vec<String> = w
        .driver
        .sent(&run, 1)
        .1
        .into_iter()
        .filter(|t| t.starts_with("Plan"))
        .collect();
    assert_eq!(sent, ["Plan. Remarks: []", "Plan. Remarks: [check: exit 1\nadd tests]"]);
    let reader = aae_core::run::Reader::open(&w.dir.path().join("state.db")).unwrap();
    let view = reader.run(&run).unwrap().unwrap();
    let plans: Vec<&Value> = view["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["step"] == "plan")
        .collect();
    assert_eq!(plans.len(), 2, "{view}");
    assert_eq!(
        (&plans[0]["kind"], &plans[1]["kind"]),
        (&json!("new_session"), &json!("resume"))
    );
    assert_eq!(plans[1]["session"], plans[0]["session"], "{view}");
}

/// A prompt check: the agent declares `fail`, and the run goes back with its summary as the remark.
#[tokio::test]
async fn a_prompt_check_that_declares_fail_sends_the_run_back() {
    let prompt = |name: &str, back: &str| {
        format!("[[step]]\nname = \"{name}\"\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"{name}.md\"\n{back}")
    };
    let steps = [prompt("plan", ""), prompt("review", "back_to = \"plan\"\n")];
    let w = flow(
        "",
        &steps,
        &[
            ("shop/formulas/plan.md", "Plan. Remarks: [{{ remarks }}]"),
            ("shop/formulas/review.md", "Review."),
        ],
    );
    for _ in 0..4 {
        w.driver.script(turn());
    }
    let run = w.start_flow().await;
    let (kind, data) = w.end(&run, &[2]).await;
    assert_eq!(kind, "run.passed", "{data}");
    let sent: Vec<String> = w
        .driver
        .sent(&run, 1)
        .1
        .into_iter()
        .filter(|t| t.starts_with("Plan"))
        .collect();
    assert_eq!(sent, ["Plan. Remarks: []", "Plan. Remarks: [review: planned]"]);
}

/// A turn that leaves 1 s for the finisher.
fn turn() -> Vec<Step> {
    vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Wait(Duration::from_secs(1)),
        Step::Emit(Normalized::TurnEnded {
            stop_reason: StopReason::Completed,
        }),
        Step::Emit(Normalized::State {
            state: SessionState::Idle,
            background: false,
        }),
    ]
}
