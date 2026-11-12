//! The step timeout of a turn (`runaway_turn`, docs/design/contracts.md 6.1) and its pauses while nothing works:
//! waiting for an answer and waiting between turns. Managed clock, starting at the epoch.

mod common;

use aae_core::{
    Journal,
    driver::{Answer, Normalized, RequestKind},
    run::{Decision, Msg},
};
use aae_testkit::Step;
use common::{POLICY, World, hollow_turn, world};
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
