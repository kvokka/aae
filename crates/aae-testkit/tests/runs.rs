//! The run machine on the scripted driver (docs/design/contracts.md 6; spec §15 criterion 4, core part), on the
//! managed clock.

use aae_core::{
    Clock, CoreHandle, Error, Journal, config,
    driver::{Answer, Normalized, RequestKind, SessionState, StopReason},
    open,
    run::{self, Create, Decision, MAX_ATTEMPTS, Msg},
};
use aae_testkit::{ScriptedDriver, Step};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

const FILES: &[(&str, &str)] = &[
    ("config.toml", "projects = [\"shop\"]\n[harness.scripted]\n"),
    ("shop/aae.toml", "[rigs.web]\npath = \"web\"\n"),
    ("shop/web/.keep", ""),
    ("shop/agents/dev/agent.toml", "harness = \"scripted\"\n"),
    (
        "shop/formulas/build.toml",
        "name = \"build\"\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n",
    ),
    ("shop/formulas/build.md", "Build {{ rig.name }}."),
];

/// A plugin with a hook on `run.attempt_failed`: decisions on it wait for its deadline instead of applying at once.
const POLICY: &[(&str, &str)] = &[
    (
        "config.toml",
        "projects = [\"shop\"]\n[harness.scripted]\n[[plugin]]\npath = \"policy\"\n",
    ),
    (
        "policy/plugin.toml",
        "name = \"policy\"\nversion = \"0.1.0\"\n[[hook]]\non = \"run.attempt_failed\"\ncommand = [\"true\"]\ntimeout = \"10s\"\n",
    ),
];

struct World {
    core: CoreHandle<Msg>,
    driver: ScriptedDriver,
    journal: PathBuf,
    _dir: tempfile::TempDir,
}

fn world(extra: &[(&str, &str)]) -> World {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in FILES
        .iter()
        .filter(|(p, _)| !extra.iter().any(|(e, _)| e == p))
        .chain(extra)
    {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let config = config::load(&dir.path().join("config.toml")).unwrap();
    let driver = ScriptedDriver::new(1);
    let journal = dir.path().join("journal");
    let (core, _task) = run::start(
        open(&dir.path().join("state.db")).unwrap(),
        Clock::managed(0),
        Arc::new(config),
        vec![Arc::new(driver.clone())],
        Journal::new(&journal),
    );
    World {
        core,
        driver,
        journal,
        _dir: dir,
    }
}

impl World {
    async fn create(&self) -> String {
        let create = Create {
            project: "shop".into(),
            rig: "web".into(),
            formula: "build".into(),
            inputs: serde_json::Map::new(),
            correlation: None,
            depth: 0,
            parent_run: None,
        };
        let reply = self.core.send(None, Msg::Create(create)).await.unwrap();
        reply["run"].as_str().unwrap().to_owned()
    }

    /// The journal events of `run`, `(type, data)`, after letting the system run for `wait`.
    async fn events(&self, run: &str, wait: Duration) -> Vec<(String, Value)> {
        tokio::time::sleep(wait).await;
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        all.into_iter()
            .filter(|e| e["run"] == run)
            .map(|e| (e["type"].as_str().unwrap().to_owned(), e["data"].clone()))
            .collect()
    }

    async fn states(&self, run: &str) -> Vec<String> {
        let events = self.events(run, Duration::from_millis(200)).await;
        let changes = events.into_iter().filter(|(t, _)| t == "run.state_changed");
        changes.map(|(_, d)| d["to"].as_str().unwrap().to_owned()).collect()
    }

    async fn seq_of(&self, run: &str, kind: &str, nth: usize) -> i64 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        let found = all
            .into_iter()
            .filter(|e| e["run"] == run && e["type"] == kind)
            .nth(nth);
        found.unwrap_or_else(|| panic!("no {kind} #{nth}"))["seq"]
            .as_i64()
            .unwrap()
    }
}

fn last<'a>(events: &'a [(String, Value)], kind: &str) -> &'a Value {
    &events
        .iter()
        .rev()
        .find(|(t, _)| t == kind)
        .unwrap_or_else(|| panic!("no {kind}"))
        .1
}

fn hollow_turn() -> Vec<Step> {
    vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Emit(Normalized::TurnEnded {
            stop_reason: StopReason::Completed,
        }),
        Step::Emit(Normalized::State {
            state: SessionState::Idle,
            background: false,
        }),
    ]
}

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
        decision: Decision::Fail { reason: "x".into() },
    };
    assert!(
        matches!(w.core.send(None, decide).await, Err(Error::Rejected(_))),
        "run.queued decides nothing"
    );
    for n in 0..MAX_ATTEMPTS {
        let seq = w.seq_of(&run, "run.attempt_failed", n as usize).await;
        let decision = Decision::NewSession {
            after: Duration::from_secs(30),
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
async fn a_permission_request_waits_for_its_answer() {
    let w = world(&[]);
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Permission,
        tool: "Bash".into(),
        input: json!({ "command": "ls" }),
        text: String::new(),
    };
    w.driver.script(vec![Step::Emit(request)]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    assert_eq!(w.states(&run).await.last().unwrap(), "requires_action");
    let answer = |request: &str| Msg::Answer {
        run: run.clone(),
        request: request.into(),
        answer: Answer::Allow { updated_input: None },
    };
    assert!(matches!(
        w.core.send(None, answer("req-2")).await,
        Err(Error::Rejected(_))
    ));
    w.core.send(None, answer("req-1")).await.unwrap();
    assert_eq!(w.states(&run).await[3..], ["requires_action", "running", "failed"]);
}

#[tokio::test(start_paused = true)]
async fn a_request_nobody_answers_is_denied_after_the_human_timeout() {
    let w = world(&[]);
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Question,
        tool: String::new(),
        input: Value::Null,
        text: "Which branch?".into(),
    };
    w.driver.script(vec![Step::Emit(request)]);
    let run = w.create().await;
    let states = w.states(&run).await;
    assert_eq!(states.last().unwrap(), "requires_action");
    tokio::time::sleep(Duration::from_secs(3600)).await;
    assert_eq!(w.states(&run).await[4..], ["running"]);
}

#[tokio::test(start_paused = true)]
async fn a_run_must_name_a_runnable_formula_with_its_inputs() {
    let w = world(&[]);
    let create = |formula: &str, inputs: Value| Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: formula.into(),
        inputs: inputs.as_object().unwrap().clone(),
        correlation: None,
        depth: 0,
        parent_run: None,
    };
    for (formula, inputs) in [("nope", json!({})), ("build", json!({ "extra": 1 }))] {
        let sent = w.core.send(None, Msg::Create(create(formula, inputs))).await;
        assert!(matches!(sent, Err(Error::Rejected(_))), "{formula}");
    }
}
