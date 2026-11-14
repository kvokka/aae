//! The step timeout of a turn (`runaway_turn`, docs/design/contracts.md 6.1) and its pauses while nothing works:
//! waiting for an answer and waiting between turns; the thresholds of `[watchdog]`. Managed clock, starting at the
//! epoch.

mod common;

use aae_core::{
    Journal,
    driver::{Answer, Normalized, RequestKind, SessionState},
    parse_rfc3339_ms,
    run::{Decision, GateSpec, Msg},
};
use aae_testkit::Step;
use common::{FILES, POLICY, World, hollow_turn, world};
use serde_json::Value;
use std::time::Duration;

const MIN: u64 = 60;

/// Tool work for `minutes` (a multiple of 9): never silent, since a running tool may be quiet for 10 minutes.
fn busy(minutes: u64) -> Vec<Step> {
    (0..minutes / 9)
        .flat_map(|_| {
            [
                Step::Emit(Normalized::Activity { tool_running: true }),
                Step::Wait(Duration::from_secs(9 * MIN)),
            ]
        })
        .collect()
}

impl World {
    /// The time of the first `run.attempt_failed` of `run` and its failure kind.
    fn attempt_failed(&self, run: &str) -> (String, String) {
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        let failed = all
            .into_iter()
            .find(|e| e["run"] == run && e["type"] == "run.attempt_failed")
            .expect("no run.attempt_failed");
        let kind = failed["data"]["failure"]["kind"].as_str().unwrap().to_owned();
        (failed["ts"].as_str().unwrap().to_owned(), kind)
    }

    /// The time of the first event `kind` of `run` whose data `pick` accepts.
    fn ts(&self, run: &str, kind: &str, pick: impl Fn(&Value) -> bool) -> String {
        let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap();
        let found = all
            .into_iter()
            .find(|e| e["run"] == run && e["type"] == kind && pick(&e["data"]));
        found.unwrap_or_else(|| panic!("no {kind}"))["ts"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

/// Seven distinct thresholds, none of them a default.
const WATCHDOG: &str = "[watchdog]\nstart_timeout = \"30s\"\nsilence = \"1m\"\ntool_silence = \"2m\"\n\
    background_silence = \"3m\"\nhuman_timeout = \"4m\"\nqueue_timeout = \"6m\"\npark_timeout = \"7m\"\n";

/// A world whose `config.toml` is the one of `files` with [`WATCHDOG`].
fn watched(files: &[(&str, &str)]) -> World {
    let config = format!("{}{WATCHDOG}", files[0].1);
    let mut extra = files.to_vec();
    extra[0] = ("config.toml", &config);
    world(&extra)
}

/// The time the attempt of a run playing `scenario` fails with `stall`.
async fn stall_at(scenario: Vec<Step>) -> String {
    let w = watched(FILES);
    w.driver.script(scenario);
    let run = w.create().await;
    w.events(&run, Duration::from_secs(10 * MIN)).await;
    let (ts, kind) = w.attempt_failed(&run);
    assert_eq!(kind, "stall");
    ts
}

#[tokio::test(start_paused = true)]
async fn every_deadline_comes_from_the_config() {
    assert_eq!(stall_at(vec![Step::NoAck]).await, "1970-01-01T00:00:30.000Z");
    assert_eq!(stall_at(Vec::new()).await, "1970-01-01T00:01:00.000Z");
    let tool = vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Emit(Normalized::Activity { tool_running: true }),
    ];
    assert_eq!(stall_at(tool).await, "1970-01-01T00:02:00.000Z");
    let mut background = hollow_turn();
    background[2] = Step::Emit(Normalized::State {
        state: SessionState::Idle,
        background: true,
    });
    assert_eq!(stall_at(background).await, "1970-01-01T00:03:00.000Z");

    let w = watched(FILES);
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Question,
        tool: String::new(),
        input: Value::Null,
        text: "Which branch?".into(),
    };
    w.driver.script(vec![Step::Emit(request)]);
    let run = w.create().await;
    w.events(&run, Duration::from_secs(10 * MIN)).await;
    let answered = w.ts(&run, "run.state_changed", |d| d["from"] == "requires_action");
    assert_eq!(answered, "1970-01-01T00:04:00.000Z");

    let rig = |until_ms| GateSpec {
        key: "rig:shop/web".into(),
        until_ms,
        reason: "quota".into(),
    };
    let w = watched(FILES);
    w.core.send(None, Msg::CloseGate(rig(None))).await.unwrap();
    let run = w.create().await;
    w.events(&run, Duration::from_secs(10 * MIN)).await;
    let failed = w.ts(&run, "run.failed", |d| d["reason"] == "queue_timeout");
    assert_eq!(failed, "1970-01-01T00:06:00.000Z");

    // As in `a_retry_under_an_operator_pause_parks_and_times_out` of gates.rs.
    let w = watched(POLICY);
    let crash = Step::Exit {
        code: None,
        signal: Some(9),
    };
    w.driver.script(vec![Step::Wait(Duration::from_secs(10)), crash]);
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.core.send(None, Msg::CloseGate(rig(None))).await.unwrap();
    tokio::time::sleep(Duration::from_secs(10)).await;
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, resume).await;
    w.events(&run, Duration::from_secs(10 * MIN)).await;
    let parked = parse_rfc3339_ms(&w.ts(&run, "run.parked", |_| true)).unwrap();
    let failed = parse_rfc3339_ms(&w.ts(&run, "run.failed", |d| d["reason"] == "park_timeout")).unwrap();
    assert_eq!(failed - parked, 7 * 60_000);
}

#[tokio::test(start_paused = true)]
async fn a_turn_past_the_step_timeout_is_a_runaway() {
    let w = world(&[]);
    let mut turn = vec![Step::Emit(Normalized::TurnStarted)];
    turn.extend(busy(54));
    w.driver.script(turn);
    let run = w.create().await;
    w.events(&run, Duration::from_secs(46 * MIN)).await;
    assert_eq!(
        w.attempt_failed(&run),
        ("1970-01-01T00:45:00.000Z".into(), "runaway_turn".into())
    );
}

#[tokio::test(start_paused = true)]
async fn the_step_timeout_pauses_while_a_request_waits_for_its_answer() {
    let w = world(&[]);
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Permission,
        tool: "Bash".into(),
        input: serde_json::Value::Null,
        text: String::new(),
    };
    let mut turn = vec![Step::Emit(Normalized::TurnStarted)];
    turn.extend(busy(36));
    turn.push(Step::Emit(request));
    w.driver.script(turn);
    w.driver.script(busy(27));
    let run = w.create().await;
    // 36 minutes of work, then 50 minutes without an answer: 9 minutes of the step are left.
    w.events(&run, Duration::from_secs(86 * MIN)).await;
    let answer = Msg::Answer {
        run: run.clone(),
        request: "req-1".into(),
        answer: Answer::Allow { updated_input: None },
    };
    w.core.send(None, answer).await.unwrap();
    w.events(&run, Duration::from_secs(10 * MIN)).await;
    assert_eq!(
        w.attempt_failed(&run),
        ("1970-01-01T01:35:00.000Z".into(), "runaway_turn".into())
    );
}

#[tokio::test(start_paused = true)]
async fn the_step_timeout_pauses_between_turns() {
    let plugin = POLICY[1]
        .1
        .replace("run.attempt_failed", "run.turn_ended_without_outcome");
    let w = world(&[POLICY[0], ("policy/plugin.toml", &plugin)]);
    let mut first = vec![Step::Emit(Normalized::TurnStarted)];
    first.extend(busy(36));
    first.extend(hollow_turn().into_iter().skip(1));
    w.driver.script(first);
    w.driver.script(busy(18));
    let run = w.create().await;
    // The decision comes 10 s after the turn ended: the next turn has the 9 minutes that were left.
    w.events(&run, Duration::from_secs(36 * MIN + 10)).await;
    let seq = w.seq_of(&run, "run.turn_ended_without_outcome", 0).await;
    let message = "Go on.".to_owned();
    let decide = Msg::Decide {
        run: run.clone(),
        seq,
        decision: Decision::Continue { message },
    };
    w.core.send(None, decide).await.unwrap();
    w.events(&run, Duration::from_secs(10 * MIN)).await;
    assert_eq!(
        w.attempt_failed(&run),
        ("1970-01-01T00:45:10.200Z".into(), "runaway_turn".into())
    );
}
