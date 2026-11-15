//! The run machine on the scripted driver (docs/design/contracts.md 6; spec §15 criterion 4, core part), on the
//! managed clock.

mod common;

use aae_core::{
    Error,
    driver::{Normalized, SessionState, StopReason},
    run::{Create, Decision, MAX_ATTEMPTS, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{POLICY, World, hollow_turn, last, world};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

#[tokio::test(start_paused = true)]
async fn a_turn_that_ends_without_an_outcome_is_hollow_done() {
    let w = world(&[]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    assert_eq!(w.states(&run).await, ["preparing", "starting", "running", "failed"]);
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.started")["run"]["state"], "preparing");
    assert_eq!(
        last(&events, "run.turn_ended_without_outcome")["stop_reason"],
        "completed"
    );
    let failed = last(&events, "run.failed");
    assert_eq!(
        (failed["class"].as_str(), failed["reason"].as_str()),
        (Some("hollow_done"), Some("no_decision"))
    );
}

#[tokio::test(start_paused = true)]
async fn background_work_after_the_turn_keeps_the_run_running_until_idle() {
    let w = world(&[]);
    let mut steps = hollow_turn();
    steps.insert(
        2,
        Step::Emit(Normalized::State {
            state: SessionState::Idle,
            background: true,
        }),
    );
    steps.insert(3, Step::Wait(Duration::from_secs(20 * 60)));
    w.driver.script(steps);
    let run = w.create().await;
    assert_eq!(w.states(&run).await, ["preparing", "starting", "running"]);
    tokio::time::sleep(Duration::from_secs(20 * 60)).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.failed")["class"], "hollow_done", "not a stall");
}

#[tokio::test(start_paused = true)]
async fn an_attempt_failure_without_a_policy_fails_the_run() {
    let w = world(&[]);
    w.driver.script(vec![Step::Exit {
        code: Some(1),
        signal: None,
    }]);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let failed = last(&events, "run.attempt_failed");
    assert_eq!(failed["failure"]["kind"], "harness_crash");
    assert_eq!(failed["attempts"][0]["failure_kind"], "harness_crash");
    assert_eq!(last(&events, "run.failed")["class"], "infrastructure");
}

#[tokio::test(start_paused = true)]
async fn silence_after_the_prompt_is_a_stall_at_its_budget() {
    let w = world(&[]);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(299)).await;
    assert!(!events.iter().any(|(t, _)| t == "run.attempt_failed"));
    let events = w.events(&run, Duration::from_secs(2)).await;
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "stall");
}

#[tokio::test(start_paused = true)]
async fn decisions_start_new_attempts_until_the_ceiling() {
    let w = world(POLICY);
    let crash = || {
        vec![Step::Exit {
            code: None,
            signal: Some(9),
        }]
    };
    w.driver.script(crash());
    let run = w.create().await;
    let decide = Msg::Decide {
        run: run.clone(),
        seq: 1,
        decision: Decision::Fail {
            reason: "x".into(),
            gate: None,
        },
    };
    assert!(
        matches!(w.core.send(None, decide).await, Err(Error::Rejected(_))),
        "run.queued decides nothing"
    );
    for n in 0..MAX_ATTEMPTS {
        let seq = w.seq_of(&run, "run.attempt_failed", n as usize).await;
        let decision = Decision::NewSession {
            after: Duration::from_secs(30),
            gate: None,
        };
        w.core
            .send(
                None,
                Msg::Decide {
                    run: run.clone(),
                    seq,
                    decision: decision.clone(),
                },
            )
            .await
            .unwrap();
        let stale = Msg::Decide {
            run: run.clone(),
            seq,
            decision,
        };
        assert!(
            matches!(w.core.send(None, stale).await, Err(Error::Rejected(_))),
            "decided once"
        );
        w.driver.script(crash());
        tokio::time::sleep(Duration::from_secs(31)).await;
    }
    let events = w.events(&run, Duration::from_millis(200)).await;
    let failed = last(&events, "run.failed");
    assert_eq!(failed["reason"], "attempts_ceiling");
    assert_eq!(failed["run"]["attempt"], MAX_ATTEMPTS);
}

#[tokio::test(start_paused = true)]
async fn without_a_decision_the_policy_deadline_fails_the_run() {
    let w = world(POLICY);
    w.driver.script(vec![Step::Exit {
        code: Some(1),
        signal: None,
    }]);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(14)).await;
    assert_eq!(
        last(&events, "run.attempt_failed")["deadline"],
        "1970-01-01T00:00:15.000Z"
    );
    assert!(!events.iter().any(|(t, _)| t == "run.failed"));
    let events = w.events(&run, Duration::from_secs(2)).await;
    assert_eq!(last(&events, "run.failed")["reason"], "no_decision");
}

#[tokio::test(start_paused = true)]
async fn a_veto_event_blocks_at_its_deadline_and_proceeds_on_abstain() {
    let plugin = POLICY[1].1.replace("run.attempt_failed", "run.preparing");
    let w = world(&[POLICY[0], ("policy/plugin.toml", &plugin)]);
    let run = w.create().await;
    let events = w.events(&run, Duration::from_secs(16)).await;
    let failed = last(&events, "run.failed");
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&json!("check"), &json!("hook_deadline"))
    );
    let run = w.create().await;
    let seq = w.seq_of(&run, "run.preparing", 0).await;
    let abstain = Msg::Decide {
        run: run.clone(),
        seq,
        decision: Decision::Abstain,
    };
    w.core.send(None, abstain).await.unwrap();
    assert_eq!(w.states(&run).await, ["preparing", "starting", "running"]);
}

#[tokio::test(start_paused = true)]
async fn one_workdir_holds_one_live_run_and_cancel_frees_it() {
    let w = world(&[]);
    let first = w.create().await;
    let second = w.create().await;
    let events = w.events(&second, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.queued")["waiting_for"], "workdir");
    assert!(!events.iter().any(|(t, _)| t == "run.started"));
    w.core
        .send(
            None,
            Msg::Cancel {
                run: first.clone(),
                reason: "operator".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(w.states(&first).await.last().unwrap(), "cancelled");
    assert_eq!(w.states(&second).await, ["preparing", "starting", "running"]);
    let again = Msg::Cancel {
        run: first,
        reason: "twice".into(),
    };
    assert!(matches!(w.core.send(None, again).await, Err(Error::Rejected(_))));
}
#[tokio::test(start_paused = true)]
async fn the_orchestrator_and_the_agent_limit_started_runs() {
    let rigs = (
        "shop/aae.toml",
        "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
    );
    let orchestrator = (
        "config.toml",
        "projects = [\"shop\"]\nbuiltin = { default-policy = { enabled = false } }\nmax_runs = 1\n[harness.scripted]\n",
    );
    let agent = ("shop/agents/dev/agent.toml", "harness = \"scripted\"\nmax_runs = 1\n");
    for limit in [orchestrator, agent] {
        let w = world(&[rigs, ("shop/api/.keep", ""), limit]);
        let first = w.create_on("web").await;
        let second = w.create_on("api").await;
        let events = w.events(&second, Duration::from_millis(200)).await;
        assert_eq!(last(&events, "run.queued")["waiting_for"], "concurrency", "{}", limit.0);
        let cancel = Msg::Cancel {
            run: first,
            reason: "operator".into(),
        };
        w.core.send(None, cancel).await.unwrap();
        assert_eq!(w.states(&second).await, ["preparing", "starting", "running"]);
    }
}

/// A turn that ends once the test notifies: it holds its run until then.
fn held_turn(world: &World) -> Arc<Notify> {
    let release = Arc::new(Notify::new());
    world.driver.script(vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Await(release.clone()),
        Step::Emit(Normalized::TurnEnded {
            stop_reason: StopReason::Completed,
        }),
        Step::Emit(Normalized::State {
            state: SessionState::Idle,
            background: false,
        }),
    ]);
    release
}

/// Declares `pass` for attempt `attempt` of `run`, then lets its turn end.
async fn pass(w: &World, run: &str, attempt: u32, turn: &Notify) {
    let token = loop {
        match w.driver.token(run, attempt) {
            Some(token) => break token,
            None => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };
    let outcome = Outcome {
        result: Verdict::Pass,
        summary: "done".into(),
        data: None,
    };
    let msg = Msg::Finish {
        run: run.into(),
        token,
        outcome,
    };
    assert_eq!(w.core.send(None, msg).await.unwrap()["answer"], "accepted");
    turn.notify_one();
}

#[tokio::test(start_paused = true)]
async fn a_step_of_an_agent_at_its_limit_parks_until_a_slot_frees() {
    let step = |name: &str, agent: &str| {
        format!("[[step]]\nname = \"{name}\"\nkind = \"prompt\"\nagent = \"{agent}\"\nprompt = \"build.md\"\n")
    };
    let formula = format!("name = \"build\"\n{}{}", step("plan", "lead"), step("work", "dev"));
    let w = world(&[
        (
            "shop/aae.toml",
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        ),
        ("shop/api/.keep", ""),
        ("shop/agents/dev/agent.toml", "harness = \"scripted\"\nmax_runs = 1\n"),
        ("shop/agents/lead/agent.toml", "harness = \"scripted\"\n"),
        ("shop/formulas/build.toml", &formula),
    ]);
    let (plan, work) = (held_turn(&w), held_turn(&w));
    let first = w.create_on("web").await;
    pass(&w, &first, 1, &plan).await;
    // A script goes to the next prompt sent: `work` to the first run's second step, not to the second run.
    while w.driver.sent(&first, 2).1.len() < 2 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let plan = held_turn(&w);
    let second = w.create_on("api").await;
    pass(&w, &second, 1, &plan).await;
    let events = w.events(&second, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.parked")["then"], "new_session");
    assert_eq!(
        w.states(&second).await.last().unwrap(),
        "parked",
        "dev already runs `{first}`"
    );
    w.driver.script(hollow_turn());
    work.notify_one();
    let states = w.states(&second).await;
    assert_eq!(states[states.len() - 4..], ["parked", "starting", "running", "failed"]);
    let events = w.events(&second, Duration::ZERO).await;
    assert_eq!(last(&events, "run.resumed")["gate_key"], "concurrency");
}

#[tokio::test(start_paused = true)]
async fn a_run_continues_the_session_of_a_finished_run_only_when_asked() {
    let w = world(&[]);
    let mut slow = hollow_turn();
    slow.insert(1, Step::Wait(Duration::from_secs(10)));
    w.driver.script(slow);
    let first = w.create().await;
    let create = |from: &str| Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "build".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: Some(from.into()),
    };
    let early = w.core.send(None, Msg::Create(create(&first))).await;
    assert!(matches!(early, Err(Error::Rejected(_))), "still running");
    w.events(&first, Duration::from_secs(11)).await;
    let nobody = w.core.send(None, Msg::Create(create("run_99"))).await;
    assert!(matches!(nobody, Err(Error::Rejected(_))));
    w.driver.script(hollow_turn());
    let reply = w.core.send(None, Msg::Create(create(&first))).await.unwrap();
    let events = w
        .events(reply["run"].as_str().unwrap(), Duration::from_millis(200))
        .await;
    let attempts = &last(&events, "run.turn_ended_without_outcome")["attempts"];
    assert_eq!(attempts[0]["kind"], "resume");
    w.driver.script(hollow_turn());
    let plain = w.create().await;
    let events = w.events(&plain, Duration::from_millis(200)).await;
    let attempts = &last(&events, "run.turn_ended_without_outcome")["attempts"];
    assert_eq!(attempts[0]["kind"], "new_session", "no implicit reuse");
}

#[tokio::test(start_paused = true)]
async fn a_run_must_name_a_runnable_formula_with_its_inputs() {
    let w = world(&[]);
    let create = |formula: &str, inputs: Value| Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: formula.into(),
        inputs: inputs.as_object().unwrap().clone(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    };
    for (formula, inputs) in [("nope", json!({})), ("build", json!({ "extra": 1 }))] {
        let sent = w.core.send(None, Msg::Create(create(formula, inputs))).await;
        assert!(matches!(sent, Err(Error::Rejected(_))), "{formula}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_run_whose_workdir_is_gone_fails_with_the_reason_and_no_launch() {
    let w = world(&[]);
    std::fs::remove_dir_all(w.dir.path().join("shop/web")).unwrap();
    let run = w.create().await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let failed = last(&events, "run.failed");
    assert_eq!(failed["class"], "infrastructure");
    assert!(
        failed["reason"].as_str().unwrap().contains("does not exist"),
        "{failed}"
    );
    assert_eq!(failed["run"]["attempt"], 0, "no launch: {failed}");
}

#[tokio::test(start_paused = true)]
async fn a_prompt_empty_after_substitution_fails_the_run_before_a_launch() {
    let formula = "name = \"build\"\n[inputs.goal]\ntype = \"string\"\n\
                   [[step]]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n";
    let w = world(&[
        ("shop/formulas/build.toml", formula),
        ("shop/formulas/build.md", "{{ inputs.goal }}\n"),
    ]);
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "build".into(),
        inputs: json!({ "goal": " " }).as_object().unwrap().clone(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let events = w
        .events(reply["run"].as_str().unwrap(), Duration::from_millis(200))
        .await;
    let failed = last(&events, "run.failed");
    assert_eq!(failed["reason"], "prompt: empty after substitution", "{failed}");
    assert_eq!(failed["run"]["attempt"], 0, "no launch: {failed}");
}
