//! The await step (docs/design/config.md, docs/design/contracts.md 6): an attempt with no process waits for an
//! outcome given from outside, until its timeout. Formulas of await steps alone run on the managed clock, starting at
//! the epoch; those with exec steps run real commands, so real time.

use crate::common;

use common::{POLICY, World, last, world, world_with};
use loop_manager_core::{
    Error, parse_rfc3339_ms,
    run::{Create, ExecHooks, Msg, Outcome, Verdict},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const FLOW: &str = "shop/formulas/flow.toml";
const HOUR: Duration = Duration::from_secs(3600);

/// An await step named `name` with `keys` besides.
fn gate(name: &str, keys: &str) -> String {
    format!("[[step]]\nname = \"{name}\"\nkind = \"await\"\n{keys}")
}

/// `sh -c <script>` as an exec step named `name`, a check when it goes `back` to a step.
fn exec(name: &str, script: &str, back: Option<&str>) -> String {
    let back = back.map(|to| format!("back_to = \"{to}\"\n")).unwrap_or_default();
    let script = json!(script);
    format!("[[step]]\nname = \"{name}\"\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {script}]\n{back}")
}

/// The formula `flow` of `steps`, as a file of the world.
fn formula(steps: &[String]) -> String {
    format!("name = \"flow\"\n{}", steps.concat())
}

/// A world on the managed clock with `flow` of `steps` and `extra` files.
fn waiting(steps: &[String], extra: &[(&str, &str)]) -> World {
    let flow = formula(steps);
    let mut files = vec![(FLOW, flow.as_str())];
    files.extend_from_slice(extra);
    world(&files)
}

/// A world on real time with `flow` of `steps`, whose exec steps run.
fn working(steps: &[String], extra: &[(&str, &str)]) -> World {
    let flow = formula(steps);
    let mut files = vec![(FLOW, flow.as_str())];
    files.extend_from_slice(extra);
    world_with(&files, Arc::new(ExecHooks::default()))
}

impl World {
    async fn begin(&self) -> String {
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

    async fn resolve(&self, run: &str, step: &str, outcome: Outcome) -> Result<Value, Error> {
        let msg = Msg::Resolve {
            run: run.into(),
            step: step.into(),
            outcome,
        };
        self.core.send(None, msg).await
    }

    /// The `n`-th `run.awaiting` of `run`.
    async fn awaiting(&self, run: &str, n: usize) -> Value {
        self.until(Some(run), "run.awaiting", n).await
    }

    fn work_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("shop/web/log")).unwrap_or_default()
    }

    /// The round of `run`, as the reader shows it.
    fn round(&self, run: &str) -> Value {
        let reader = loop_manager_core::run::Reader::open(&self.dir.path().join("state.db")).unwrap();
        reader.run(run).unwrap().unwrap()["round"].clone()
    }

    /// The time of the event `kind` of `run`, in ms.
    fn at(&self, run: &str, kind: &str) -> i64 {
        let all = loop_manager_core::Journal::new(&self.journal)
            .read_from(1, 100_000)
            .unwrap();
        let event = all.into_iter().find(|e| e["run"] == run && e["type"] == kind);
        parse_rfc3339_ms(event.unwrap_or_else(|| panic!("no {kind}"))["ts"].as_str().unwrap()).unwrap()
    }
}

fn outcome(result: Verdict, summary: &str) -> Outcome {
    Outcome {
        result,
        summary: summary.into(),
        data: None,
    }
}

fn count(events: &[(String, Value)], kind: &str) -> usize {
    events.iter().filter(|(t, _)| t == kind).count()
}

#[tokio::test(start_paused = true)]
async fn a_pass_from_outside_moves_the_run_to_the_next_step() {
    let w = waiting(&[gate("gate", "timeout = \"1h\"\n"), gate("ship", "")], &[]);
    let run = w.begin().await;
    let waits = w.awaiting(&run, 0).await;
    assert_eq!(
        (&waits["run"]["step"], &waits["run"]["step_kind"]),
        (&json!("gate"), &json!("await"))
    );
    let deadline = parse_rfc3339_ms(waits["deadline"].as_str().unwrap()).unwrap();
    assert_eq!(deadline - w.at(&run, "run.awaiting"), 3_600_000, "{waits}");
    let given = Outcome {
        data: Some(json!({ "pr": 1 })),
        ..outcome(Verdict::Pass, "ok")
    };
    assert_eq!(w.resolve(&run, "gate", given).await.unwrap(), Value::Null);
    let next = w.awaiting(&run, 1).await;
    assert_eq!(next["run"]["step"], "ship");
    let events = w.events(&run, Duration::ZERO).await;
    let finished = last(&events, "run.step_finished");
    assert_eq!(
        finished["outcome"],
        json!({ "result": "pass", "summary": "ok", "data": { "pr": 1 } })
    );
    w.resolve(&run, "ship", outcome(Verdict::Pass, "shipped"))
        .await
        .unwrap();
    let passed = w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(passed["outcome"]["summary"], "shipped");
}

#[tokio::test]
async fn a_fail_sends_the_run_back_with_its_summary_as_remarks() {
    let work = exec("work", "printf 'work [%s]\\n' \"$LOOP_MANAGER_REMARKS\" >> log", None);
    let w = working(&[work, gate("gate", "back_to = \"work\"\n")], &[]);
    let run = w.begin().await;
    w.awaiting(&run, 0).await;
    w.resolve(&run, "gate", outcome(Verdict::Fail, "redo")).await.unwrap();
    w.awaiting(&run, 1).await;
    assert_eq!(w.work_log(), "work []\nwork [gate: redo]\n");
    assert_eq!(w.round(&run), 2);
    w.resolve(&run, "gate", outcome(Verdict::Pass, "fine")).await.unwrap();
    w.until(Some(&run), "run.passed", 0).await;
}

#[tokio::test(start_paused = true)]
async fn a_fail_without_back_to_fails_the_run() {
    let w = waiting(&[gate("gate", "")], &[]);
    let run = w.begin().await;
    w.awaiting(&run, 0).await;
    w.resolve(&run, "gate", outcome(Verdict::Fail, "not now"))
        .await
        .unwrap();
    let failed = w.until(Some(&run), "run.failed", 0).await;
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&json!("agent"), &json!("not now"))
    );
}

#[tokio::test(start_paused = true)]
async fn a_wait_past_its_timeout_fails_the_run_with_no_return() {
    let w = waiting(
        &[gate("a", ""), gate("gate", "timeout = \"1h\"\nback_to = \"a\"\n")],
        &[],
    );
    let run = w.begin().await;
    w.awaiting(&run, 0).await;
    w.resolve(&run, "a", outcome(Verdict::Pass, "")).await.unwrap();
    let deadline = w.awaiting(&run, 1).await["deadline"].as_str().unwrap().to_owned();
    tokio::time::sleep(HOUR + Duration::from_secs(60)).await;
    let events = w.events(&run, Duration::from_millis(200)).await;
    let failed = last(&events, "run.failed");
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&json!("check"), &json!("await_timeout"))
    );
    assert_eq!(
        w.at(&run, "run.failed"),
        parse_rfc3339_ms(&deadline).unwrap(),
        "at the deadline"
    );
    assert_eq!(count(&events, "run.awaiting"), 2, "no return to `a`");
    assert_eq!(count(&events, "run.attempt_failed"), 0);
}

#[tokio::test(start_paused = true)]
async fn a_wait_past_its_timeout_passes_when_told_so() {
    let w = waiting(&[gate("gate", "timeout = \"1h\"\nfail_on_timeout = false\n")], &[]);
    let run = w.begin().await;
    w.awaiting(&run, 0).await;
    tokio::time::sleep(HOUR + Duration::from_secs(60)).await;
    let passed = w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(
        passed["outcome"],
        json!({ "result": "pass", "summary": "timeout", "data": null })
    );
}

#[tokio::test(start_paused = true)]
async fn an_outcome_for_another_step_is_rejected_and_changes_nothing() {
    let w = waiting(&[gate("gate", ""), gate("ship", "")], &[]);
    let run = w.begin().await;
    w.awaiting(&run, 0).await;
    // A run of `build` in its ai step `1`, running.
    let busy = w.create().await;
    w.until(Some(&busy), "run.started", 0).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.states(&busy).await.last().unwrap(), "running");
    let before = w.events(&run, Duration::ZERO).await.len();
    let rejected = |reply: Result<Value, Error>, case: &str| match reply {
        Err(Error::Rejected(_)) => {}
        other => panic!("{case}: not rejected: {other:?}"),
    };
    rejected(
        w.resolve(&run, "ship", outcome(Verdict::Pass, "")).await,
        "another step",
    );
    rejected(w.resolve(&busy, "1", outcome(Verdict::Pass, "")).await, "an ai step");
    let long = outcome(Verdict::Pass, &"s".repeat(4 * 1024 + 1));
    rejected(w.resolve(&run, "gate", long).await, "the summary");
    let big = Outcome {
        data: Some(json!({ "d": "d".repeat(64 * 1024) })),
        ..outcome(Verdict::Pass, "")
    };
    rejected(w.resolve(&run, "gate", big).await, "the data");
    match w.resolve("r-none", "gate", outcome(Verdict::Pass, "")).await {
        Err(Error::NotFound(_)) => {}
        other => panic!("not missing: {other:?}"),
    }
    assert_eq!(w.events(&run, Duration::from_millis(200)).await.len(), before);
    assert_eq!(w.states(&busy).await.last().unwrap(), "running");
    // The same outcome twice: the second finds the run ended.
    let one = waiting(&[gate("gate", "")], &[]);
    let done = one.begin().await;
    one.awaiting(&done, 0).await;
    one.resolve(&done, "gate", outcome(Verdict::Pass, "")).await.unwrap();
    one.until(Some(&done), "run.passed", 0).await;
    let ended = one.events(&done, Duration::ZERO).await.len();
    rejected(
        one.resolve(&done, "gate", outcome(Verdict::Pass, "")).await,
        "an ended run",
    );
    assert_eq!(one.events(&done, Duration::from_millis(200)).await.len(), ended);
}

#[tokio::test(start_paused = true)]
async fn a_restart_leaves_the_wait_and_its_deadline_alone() {
    let mut w = waiting(&[gate("gate", "timeout = \"1h\"\n")], POLICY);
    let (first, second) = (w.begin().await, w.begin().await);
    w.awaiting(&first, 0).await;
    let deadline = w.awaiting(&second, 0).await["deadline"].as_str().unwrap().to_owned();
    w.restart_after(Duration::from_secs(20 * 60)).await;
    let started = w.until(None, "daemon.started", 1).await;
    assert_eq!(started["interrupted_runs"], 0, "{started}");
    w.resolve(&first, "gate", outcome(Verdict::Pass, "after"))
        .await
        .unwrap();
    w.until(Some(&first), "run.passed", 0).await;
    tokio::time::sleep(HOUR).await;
    let events = w.events(&second, Duration::from_millis(200)).await;
    assert_eq!(last(&events, "run.failed")["reason"], "await_timeout");
    assert_eq!(w.at(&second, "run.failed"), parse_rfc3339_ms(&deadline).unwrap());
    for run in [&first, &second] {
        let events = w.events(run, Duration::ZERO).await;
        assert_eq!(count(&events, "run.awaiting"), 1, "{run}");
        assert_eq!(count(&events, "run.attempt_failed"), 0, "{run}");
    }
}

#[tokio::test]
async fn a_waiting_run_leaves_its_slot_to_the_next_run() {
    let one = (
        "config.toml",
        "projects = [\"shop\"]\nmax_runs = 1\nbuiltin = { default-policy = { enabled = false } }\n[harness.scripted]\n",
    );
    // The first run holds the slot in its exec step while the second is queued.
    let w = working(&[exec("work", "sleep 0.3", None), gate("gate", "")], &[one]);
    let (first, second) = (w.begin().await, w.begin().await);
    w.awaiting(&first, 0).await;
    let waits = w.awaiting(&second, 0).await;
    assert_eq!(waits["run"]["step"], "gate");
    assert_eq!(w.states(&first).await.last().unwrap(), "running");
}

#[tokio::test]
async fn a_failed_check_before_an_await_sends_the_run_back_first() {
    let test = "n=$(($(cat n 2>/dev/null || echo 0) + 1)); echo $n > n; echo test >> log; [ $n -gt 1 ] || exit 1";
    let steps = [
        exec("work", "echo work >> log", None),
        exec("test", test, Some("work")),
        gate("gate", "back_to = \"work\"\n"),
    ];
    let w = working(&steps, &[]);
    let run = w.begin().await;
    w.awaiting(&run, 0).await;
    assert_eq!(w.work_log(), "work\ntest\nwork\ntest\n");
    assert_eq!(w.round(&run), 2);
    let events = w.events(&run, Duration::from_millis(200)).await;
    assert_eq!(count(&events, "run.awaiting"), 1);
}

/// The checks that sent the run back into its round survive a restart: a step waiting for one runs after it
/// (docs/design/config.md, invariants "Recovery").
#[tokio::test(start_paused = true)]
async fn a_restart_keeps_the_checks_that_sent_the_run_back() {
    let fix = gate("fix", "if = { back_from = \"check\" }\n");
    let steps = [gate("a", ""), gate("check", "back_to = \"a\"\n"), gate("hold", ""), fix];
    let mut w = waiting(&steps, POLICY);
    let run = w.begin().await;
    w.awaiting(&run, 0).await;
    w.resolve(&run, "a", outcome(Verdict::Pass, "")).await.unwrap();
    w.resolve(&run, "check", outcome(Verdict::Fail, "redo")).await.unwrap();
    w.resolve(&run, "a", outcome(Verdict::Pass, "")).await.unwrap();
    w.resolve(&run, "check", outcome(Verdict::Pass, "")).await.unwrap();
    assert_eq!(w.awaiting(&run, 4).await["run"]["step"], "hold");
    w.restart().await;
    w.resolve(&run, "hold", outcome(Verdict::Pass, "")).await.unwrap();
    assert_eq!(w.awaiting(&run, 5).await["run"]["step"], "fix");
}
