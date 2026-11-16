//! The steps of a formula (spec §9.1, §9.3; docs/design/contracts.md 6.1): in order, each once, a session of its own
//! for each ai step, the outcome of a step in the template and the environment of the later ones, and a kill of
//! the daemon at every step boundary. Exec steps run real commands, so real time.

use crate::common;

use aae_core::{
    driver::{Normalized, SessionState, StopReason},
    run::{Create, Decision, ExecHooks, HookRunner, MAX_ATTEMPTS, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{Hanging, POLICY, World, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

/// `sh -c <script>` as an exec step named `name`.
fn exec(name: &str, script: &str) -> String {
    format!(
        "[[step]]\nname = \"{name}\"\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {}]\n",
        json!(script)
    )
}

/// An ai step named `name` of agent `dev` with the prompt file `<name>.md`.
fn prompt(name: &str) -> String {
    format!("[[step]]\nname = \"{name}\"\nkind = \"ai\"\nagent = \"dev\"\nprompt = \"{name}.md\"\n")
}

/// A world with formula `flow` of `steps` and the extra files `extra`, on `hooks`.
fn flow(steps: &[String], extra: &[(&str, &str)], hooks: Arc<dyn HookRunner>) -> World {
    let formula = format!("name = \"flow\"\n{}", steps.concat());
    let mut files = vec![("shop/formulas/flow.toml", formula.as_str())];
    files.extend_from_slice(extra);
    world_with(&files, hooks)
}

impl World {
    /// Scripts a turn that ends only once `pass` is told its attempt has declared: no race with the wall clock.
    fn turn(&self) -> Arc<Notify> {
        let declared = Arc::new(Notify::new());
        self.driver.script(vec![
            Step::Emit(Normalized::TurnStarted),
            Step::Await(declared.clone()),
            Step::Emit(Normalized::TurnEnded {
                stop_reason: StopReason::Completed,
            }),
            Step::Emit(Normalized::State {
                state: SessionState::Idle,
                background: false,
            }),
        ]);
        declared
    }

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

    /// Declares `pass` with `data` for the attempt `attempt` of `run` once its session has the token, then lets
    /// its turn end.
    async fn pass(&self, run: &str, attempt: u32, data: Value, turn: &Notify) -> Value {
        let started = std::time::Instant::now();
        let token = loop {
            if let Some(token) = self.driver.token(run, attempt) {
                break token;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "attempt {attempt} of `{run}` got no token"
            );
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
        let answer = self.core.send(None, msg).await.unwrap()["answer"].clone();
        turn.notify_one();
        answer
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
    let turn = w.turn();
    let run = w.start_flow().await;
    assert_eq!(w.pass(&run, 2, json!({ "commit": "abc" }), &turn).await, "accepted");
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    let (_, prompts) = w.driver.sent(&run, 2);
    assert_eq!(prompts, ["Use word in work."]);
    assert_eq!(w.log(), "pass {\"commit\":\"abc\"}\n");
    let attempts = w.attempts();
    let sessions: Vec<bool> = attempts.iter().map(|(_, s)| s.is_some()).collect();
    assert_eq!(sessions, [false, true, false], "only the ai step has a session");
    // The token of the ai step is stale once the run moved on.
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
    let (one, two) = (w.turn(), w.turn());
    let run = w.start_flow().await;
    assert_eq!(w.pass(&run, 1, json!(1), &one).await, "accepted");
    assert_eq!(w.pass(&run, 2, json!(2), &two).await, "accepted");
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

/// A step resumed before it ever had a session starts a new one: it never takes the session of an earlier step.
#[tokio::test]
async fn a_resumed_step_never_takes_the_session_of_an_earlier_step() {
    let texts = [("shop/formulas/one.md", "One."), ("shop/formulas/two.md", "Two.")];
    let mut w = flow(
        &[prompt("one"), prompt("two")],
        &[POLICY, &texts].concat(),
        Arc::new(Hanging::default()),
    );
    let one = w.turn();
    let run = w.start_flow().await;
    assert_eq!(w.pass(&run, 1, json!(1), &one).await, "accepted");
    // Step `two` starts its attempt, its harness never reports a session, and the daemon dies.
    w.driver.stall_sessions();
    let started = std::time::Instant::now();
    while w.attempts().len() < 2 {
        assert!(started.elapsed() < Duration::from_secs(10), "step `two` did not start");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    w.restart().await;
    let two = w.turn();
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, resume).await;
    assert_eq!(w.pass(&run, 3, json!(3), &two).await, "accepted");
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    let attempts = w.attempts();
    assert_eq!(attempts[1], ("two".to_owned(), None), "{attempts:?}");
    assert!(
        attempts[2].1.is_some() && attempts[2].1 != attempts[0].1,
        "{attempts:?}"
    );
}

/// The ceiling counts failed attempts only (invariant 16): a passed step does not use it up for the later ones.
#[tokio::test]
async fn the_attempts_ceiling_counts_no_passed_step() {
    let plugin = r#"name = "policy"
version = "0.1.0"
[[hook]]
on = "run.attempt_failed"
command = ["sh", "-c", "echo '{\"decision\":\"new_session\"}'"]
timeout = "10s"
"#;
    let last = MAX_ATTEMPTS;
    let steps = [
        exec("a", "true"),
        // Fails with `EX_TEMPFAIL` until its attempt `last`: 1 passed and `last - 1` failed attempts in all.
        exec(
            "b",
            &format!("n=$(($(cat n 2>/dev/null || echo 0) + 1)); echo $n > n; [ $n -ge {last} ] || exit 75"),
        ),
    ];
    let w = flow(
        &steps,
        &[POLICY[0], ("policy/plugin.toml", plugin)],
        Arc::new(ExecHooks::default()),
    );
    let run = w.start_flow().await;
    let (kind, data) = w.end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.attempts().len(), 1 + last as usize);
}

/// A remark asks the agent of an ai step again: an exec step has none, so a remark on it is refused.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_remark_on_an_exec_step_is_refused() {
    let plugin = "name = \"policy\"\nversion = \"0.1.0\"\n\
        [[hook]]\non = \"run.finishing\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n";
    let hooks = Arc::new(Hanging::default());
    let w = flow(
        &[exec("a", "true")],
        &[POLICY[0], ("policy/plugin.toml", plugin)],
        hooks,
    );
    let run = w.start_flow().await;
    w.until(Some(&run), "run.finishing", 0).await;
    let seq = w.seq_of(&run, "run.finishing", 0).await;
    let message = "again".to_owned();
    let decision = Decision::Remark { message };
    let refused = w
        .core
        .send(
            None,
            Msg::Decide {
                run: run.clone(),
                seq,
                decision,
            },
        )
        .await;
    assert!(refused.unwrap_err().to_string().contains("does not apply"));
    w.decide(&run, "run.finishing", 0, Decision::Abstain).await;
    assert_eq!(w.end(&run).await.0, "run.passed");
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
