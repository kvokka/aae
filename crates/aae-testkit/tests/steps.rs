//! The steps of a formula (spec §9.1, §9.3; docs/design/contracts.md 6.1): in order, each once, a session of its own
//! for each prompt step, the outcome of a step in the template and the environment of the later ones, and a kill of
//! the daemon at every step boundary. Exec steps run real commands, so real time.

mod common;

use aae_core::{
    driver::{Normalized, SessionState, StopReason},
    run::{Create, Decision, ExecHooks, HookRunner, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{Hanging, POLICY, World, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// `sh -c <script>` as an exec step named `name`.
fn exec(name: &str, script: &str) -> String {
    format!(
        "[[step]]\nname = \"{name}\"\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {}]\n",
        json!(script)
    )
}

/// A prompt step named `name` of agent `dev` with the prompt file `<name>.md`.
fn prompt(name: &str) -> String {
    format!("[[step]]\nname = \"{name}\"\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"{name}.md\"\n")
}

/// A world with formula `flow` of `steps` and the extra files `extra`, on `hooks`.
fn flow(steps: &[String], extra: &[(&str, &str)], hooks: Arc<dyn HookRunner>) -> World {
    let formula = format!("name = \"flow\"\n{}", steps.concat());
    let mut files = vec![("shop/formulas/flow.toml", formula.as_str())];
    files.extend_from_slice(extra);
    world_with(&files, hooks)
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

    /// `(step, session)` of every attempt, in order.
    fn attempts(&self) -> Vec<(String, Option<String>)> {
        let db = rusqlite::Connection::open(self.dir.path().join("state.db")).unwrap();
        let mut stmt = db.prepare("SELECT step, session FROM attempts ORDER BY n").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.map(Result::unwrap).collect()
    }

    fn steps_finished(&self) -> Vec<Value> {
        self.all("run.step_finished")
            .iter()
            .map(|d| d["run"]["step"].clone())
            .collect()
    }

    /// Declares `pass` with `data` for the attempt `attempt` of `run` once its session has the token.
    async fn pass(&self, run: &str, attempt: u32, data: Value) -> Value {
        let token = loop {
            if let Some(token) = self.driver.token(run, attempt) {
                break token;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let outcome = Outcome {
            result: Verdict::Pass,
            summary: "done".into(),
            data: Some(data),
        };
        let msg = Msg::Finish {
            run: run.into(),
            token,
            outcome,
        };
        self.core.send(None, msg).await.unwrap()["answer"].clone()
    }

    async fn end(&self, run: &str) -> (String, Value) {
        for _ in 0..500 {
            for kind in ["run.passed", "run.failed", "run.cancelled"] {
                if let Some(data) = self.all(kind).into_iter().find(|d| d["run"]["id"] == run) {
                    return (kind.to_owned(), data);
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("`{run}` did not end");
    }
}

#[tokio::test]
async fn three_exec_steps_run_in_order_once_each_and_see_the_earlier_outcomes() {
    let steps = [
        exec("a", "echo a >> log; echo out-a"),
        exec(
            "b",
            "printf '%s\\n' \"b $AAE_STEP_A_RESULT $AAE_STEP_A_SUMMARY $AAE_STEP_A_DATA\" >> log",
        ),
        exec("c", "echo c >> log"),
    ];
    let w = flow(&steps, &[], Arc::new(ExecHooks::default()));
    let run = w.start_flow().await;
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(data["run"]["step"], "c");
    let log = w.log();
    assert!(log.starts_with("a\nb pass ") && log.ends_with("}\nc\n"), "{log}");
    assert!(log.contains(r#""stdout":"out-a\n""#), "{log}");
    assert_eq!(w.steps_finished(), [json!("a"), json!("b")]);
    let steps: Vec<String> = w.attempts().into_iter().map(|(s, _)| s).collect();
    assert_eq!(steps, ["a", "b", "c"]);
}

#[tokio::test]
async fn a_failed_step_fails_the_run_and_the_next_does_not_start() {
    let steps = [
        exec("a", "echo a >> log"),
        exec("b", "exit 3"),
        exec("c", "echo c >> log"),
    ];
    let w = flow(&steps, &[], Arc::new(ExecHooks::default()));
    let run = w.start_flow().await;
    let (kind, data) = w.end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("agent")),
        "{data}"
    );
    assert_eq!(data["run"]["step"], "b");
    assert_eq!(w.log(), "a\n");
}

#[tokio::test]
async fn exec_then_prompt_then_exec_pass_the_outcome_along() {
    let steps = [
        exec("pre", "printf word"),
        prompt("work"),
        exec(
            "post",
            "printf '%s\\n' \"$AAE_STEP_WORK_RESULT $AAE_STEP_WORK_DATA\" >> log",
        ),
    ];
    let w = flow(
        &steps,
        &[(
            "shop/formulas/work.md",
            "Use {{ steps.pre.data.stdout }} in {{ run.step }}.",
        )],
        Arc::new(ExecHooks::default()),
    );
    w.driver.script(turn());
    let run = w.start_flow().await;
    assert_eq!(w.pass(&run, 2, json!({ "commit": "abc" })).await, "accepted");
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    let (_, prompts) = w.driver.sent(&run, 2);
    assert_eq!(prompts, ["Use word in work."]);
    assert_eq!(w.log(), "pass {\"commit\":\"abc\"}\n");
    let attempts = w.attempts();
    let sessions: Vec<bool> = attempts.iter().map(|(_, s)| s.is_some()).collect();
    assert_eq!(sessions, [false, true, false], "only the prompt step has a session");
    // The token of the prompt step is stale once the run moved on.
    let token = w.driver.token(&run, 2).unwrap();
    let outcome = Outcome {
        result: Verdict::Pass,
        summary: "late".into(),
        data: None,
    };
    let late = Msg::Finish {
        run: run.clone(),
        token,
        outcome,
    };
    assert_ne!(w.core.send(None, late).await.unwrap()["answer"], "accepted");
}

#[tokio::test]
async fn each_prompt_step_has_a_session_of_its_own() {
    let w = flow(
        &[prompt("one"), prompt("two")],
        &[("shop/formulas/one.md", "One."), ("shop/formulas/two.md", "Two.")],
        Arc::new(ExecHooks::default()),
    );
    w.driver.script(turn());
    w.driver.script(turn());
    let run = w.start_flow().await;
    assert_eq!(w.pass(&run, 1, json!(1)).await, "accepted");
    assert_eq!(w.pass(&run, 2, json!(2)).await, "accepted");
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    let attempts = w.attempts();
    assert_eq!(attempts.len(), 2);
    assert!(
        attempts[0].1.is_some() && attempts[0].1 != attempts[1].1,
        "{attempts:?}"
    );
    assert_eq!(w.driver.sent(&run, 2).1, ["One.", "Two."]);
}

/// The daemon dies at every step boundary — a step in `finishing`, the next in `preparing`, a command in flight —
/// and the run goes on from the step it was at: no finished step runs again.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_kill_at_every_step_boundary_keeps_the_step() {
    let plugin = "name = \"policy\"\nversion = \"0.1.0\"\n\
        [[hook]]\non = \"run.preparing\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n\
        [[hook]]\non = \"run.finishing\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n\
        [[hook]]\non = \"run.attempt_failed\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n";
    let steps = [
        exec("a", "echo a >> log"),
        exec("b", "echo b >> log"),
        exec("c", "sleep 1; echo c >> log"),
    ];
    let hooks = Arc::new(Hanging::default());
    let mut w = flow(&steps, &[POLICY[0], ("policy/plugin.toml", plugin)], hooks);
    let run = w.start_flow().await;
    let latest = |w: &World, kind: &str| w.all(kind).len() - 1;
    for (n, step) in ["a", "b", "c"].into_iter().enumerate() {
        // The step waits to prepare: kill, then let it start.
        w.until(Some(&run), "run.preparing", latest(&w, "run.preparing").max(n))
            .await;
        w.restart().await;
        w.decide(&run, "run.preparing", latest(&w, "run.preparing"), Decision::Abstain)
            .await;
        if step == "c" {
            // Its command is in flight: kill; the sweep stops it and a new attempt of the same step runs it.
            tokio::time::sleep(Duration::from_millis(300)).await;
            w.restart().await;
            w.until(Some(&run), "run.attempt_failed", 0).await;
            let again = Decision::NewSession {
                after: Duration::ZERO,
                gate: None,
            };
            w.decide(&run, "run.attempt_failed", 0, again).await;
        }
        // The step's outcome waits for its hooks: kill, then accept it.
        w.until(Some(&run), "run.finishing", n).await;
        w.restart().await;
        w.decide(&run, "run.finishing", latest(&w, "run.finishing"), Decision::Abstain)
            .await;
    }
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.log(), "a\nb\nc\n");
    assert_eq!(w.steps_finished(), [json!("a"), json!("b")]);
    let steps: Vec<String> = w.attempts().into_iter().map(|(s, _)| s).collect();
    assert_eq!(steps, ["a", "b", "c", "c"]);
}
