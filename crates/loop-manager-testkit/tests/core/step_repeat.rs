//! A step that does not pass runs again within its `max_attempts`, and its check script decides whether it passed
//! (docs/design/config.md, criterion 29, core part). The check script is a real process, so real time.

use crate::common;

use common::{POLICY, World, world};
use loop_manager_core::{
    driver::{Normalized, SessionState, StopReason},
    run::{Decision, Msg, Outcome, Reader, Verdict},
};
use loop_manager_testkit::Step;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// A check script that counts its calls in `check.sh.n` beside itself, runs `body`, then exits with the n-th of
/// `codes` (0 past them).
pub(crate) fn script(codes: &str, body: &str) -> String {
    format!(
        "#!/bin/sh\nn=$(($(cat \"$0.n\" 2>/dev/null || echo 0) + 1)); echo $n > \"$0.n\"\n{body}\n\
         set -- {codes}\neval \"code=\\${{$n:-0}}\"\nexit $code\n"
    )
}

/// The formula `build`: the ai step `work` with `max_attempts` and the check script `codes`, then `more` steps.
pub(crate) fn repeating(max_attempts: u32, codes: &str, body: &str, more: &str, extra: &[(&str, &str)]) -> World {
    let formula = format!(
        "name = \"build\"\n[[step]]\nname = \"work\"\nkind = \"ai\"\nagent = \"dev\"\nmax_attempts = {max_attempts}\n\
         step_check_script_path = \"check.sh\"\n{more}"
    );
    let script = script(codes, body);
    let mut files = vec![
        ("shop/formulas/build.toml", formula.as_str()),
        ("shop/formulas/check.sh", script.as_str()),
    ];
    files.extend_from_slice(extra);
    world(&files)
}

impl World {
    /// How many times the check script ran.
    pub(crate) fn check_calls(&self) -> u32 {
        let n = std::fs::read_to_string(self.dir.path().join("shop/formulas/check.sh.n"));
        n.map_or(0, |n| n.trim().parse().unwrap())
    }

    /// Scripts the turn of the next attempt: it ends once `repeat_declare` lets it.
    pub(crate) fn repeat_turn(&self) -> Arc<Notify> {
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

    /// Attempt `attempt` of `run` declares `result`, then its turn ends.
    pub(crate) async fn repeat_declare(&self, run: &str, attempt: u32, result: Verdict, turn: &Notify) {
        let started = Instant::now();
        let token = loop {
            if let Some(token) = self.driver.token(run, attempt) {
                break token;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no attempt {attempt}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let outcome = Outcome {
            result,
            summary: "planned".into(),
            data: None,
        };
        let msg = Msg::Finish {
            run: run.into(),
            token,
            outcome,
        };
        assert_eq!(self.core.send(None, msg).await.unwrap()["answer"], "accepted");
        turn.notify_one();
    }

    /// The event that ended `run` and its data, waiting for it up to 10 s.
    pub(crate) async fn repeat_end(&self, run: &str) -> (String, Value) {
        let started = Instant::now();
        loop {
            for kind in ["run.passed", "run.failed", "run.cancelled"] {
                if let Some(data) = self.all(kind).into_iter().find(|d| d["run"]["id"] == run) {
                    return (kind.to_owned(), data);
                }
            }
            assert!(started.elapsed() < Duration::from_secs(10), "`{run}` did not end");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The executions of each step `run` finished: `(step, step_attempt)`.
    pub(crate) fn executions(&self, run: &str) -> Vec<(String, Value)> {
        let all = self.all("run.step_finished").into_iter();
        let of = all.filter(|d| d["run"]["id"] == run).map(|d| d["run"].clone());
        of.map(|r| (r["step"].as_str().unwrap().to_owned(), r["step_attempt"].clone()))
            .collect()
    }

    pub(crate) fn repeat_view(&self, run: &str) -> Value {
        let reader = Reader::open(&self.dir.path().join("state.db")).unwrap();
        reader.run(run).unwrap().unwrap()
    }
}

/// The first prompt of the step, then each repeat: the template with the remark of the execution before.
pub(crate) const FIRST: &str = "Build web.";

pub(crate) fn again(remark: &str) -> String {
    format!("{FIRST}\n\nwork: {remark}")
}

#[tokio::test]
async fn a_step_runs_again_until_its_check_script_passes() {
    let w = repeating(3, "1 1 0", "echo \"check $n\"", "", &[]);
    let turns = [w.repeat_turn(), w.repeat_turn(), w.repeat_turn()];
    let run = w.create().await;
    for (attempt, turn) in (1..).zip(&turns) {
        w.repeat_declare(&run, attempt, Verdict::Pass, turn).await;
    }
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.check_calls(), 3);
    let sent = w.driver.sent(&run, 1).1;
    let remarks = ["check script: exit 1\ncheck 1", "check script: exit 1\ncheck 2"];
    assert_eq!(sent, [FIRST.to_owned(), again(remarks[0]), again(remarks[1])]);
    let view = w.repeat_view(&run);
    assert_eq!(
        (&view["round"], &view["step_attempt"]),
        (&json!(1), &json!(3)),
        "{view}"
    );
    let attempts = view["attempts"].as_array().unwrap();
    let kinds: Vec<&Value> = attempts.iter().map(|a| &a["kind"]).collect();
    assert_eq!(kinds, ["new_session"; 3], "each repeat is a new session: {view}");
    let sessions: std::collections::HashSet<String> = attempts.iter().map(|a| a["session"].to_string()).collect();
    assert_eq!(sessions.len(), 3, "{view}");
    assert!(
        attempts.iter().all(|a| a["failure"].is_null()),
        "a repeat is no failure: {view}"
    );
    let one = |n: u32| ("work".to_owned(), json!(n));
    assert_eq!(w.executions(&run), [one(1), one(2)]);
}

#[tokio::test]
async fn past_max_attempts_the_step_fails() {
    let w = repeating(2, "1 1 0", "", "", &[]);
    let turns = [w.repeat_turn(), w.repeat_turn()];
    let run = w.create().await;
    for (attempt, turn) in (1..).zip(&turns) {
        w.repeat_declare(&run, attempt, Verdict::Pass, turn).await;
    }
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("check")),
        "{data}"
    );
    assert!(
        data["reason"].as_str().unwrap().contains("check script: exit 1"),
        "{data}"
    );
    assert_eq!(w.check_calls(), 2);
}

#[tokio::test]
async fn exit_75_of_the_check_script_fails_the_step_at_once() {
    let w = repeating(3, "75 0", "", "", &[]);
    let turn = w.repeat_turn();
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Pass, &turn).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("check")),
        "{data}"
    );
    assert!(
        data["reason"].as_str().unwrap().contains("check script: exit 75"),
        "{data}"
    );
    assert_eq!(w.check_calls(), 1);
    assert_eq!(w.executions(&run), [], "no repeat");
}

#[tokio::test]
async fn a_step_that_fails_on_its_own_runs_again_without_its_script() {
    let w = repeating(2, "0", "", "", &[]);
    let turns = [w.repeat_turn(), w.repeat_turn()];
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Fail, &turns[0]).await;
    w.repeat_declare(&run, 2, Verdict::Pass, &turns[1]).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(
        w.check_calls(),
        1,
        "the script judges only a step that passed on its own"
    );
    assert_eq!(w.driver.sent(&run, 1).1, [FIRST.to_owned(), again("planned")]);
    // With no execution left, the step's own failure fails it.
    let w = repeating(1, "0", "", "", &[]);
    let turn = w.repeat_turn();
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Fail, &turn).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"], &data["reason"]),
        ("run.failed", &json!("agent"), &json!("planned"))
    );
    assert_eq!(w.check_calls(), 0);
}

#[tokio::test]
async fn a_repeat_stays_in_its_round_and_a_round_starts_the_count_again() {
    // The check `review` fails once and sends the run back to `work`.
    let review = "[[step]]\nname = \"review\"\nkind = \"exec\"\nback_to = \"work\"\ncommand = [\"sh\", \"-c\", \
        \"[ -f reviewed ] || { touch reviewed; exit 1; }\"]\n";
    let w = repeating(2, "1 0 0", "", review, &[]);
    let turns = [w.repeat_turn(), w.repeat_turn(), w.repeat_turn()];
    let run = w.create().await;
    // Attempt 3 is the command of `review`.
    for (attempt, turn) in [1, 2, 4].into_iter().zip(&turns) {
        w.repeat_declare(&run, attempt, Verdict::Pass, turn).await;
    }
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.check_calls(), 3);
    let at = |step: &str, n: u32| (step.to_owned(), json!(n));
    let finished = [at("work", 1), at("work", 2), at("review", 1), at("work", 1)];
    assert_eq!(w.executions(&run), finished);
    // The remark of a repeat stays with its round: the template has no place for the remarks of a return.
    let remark = again("check script: exit 1");
    assert_eq!(w.driver.sent(&run, 1).1, [FIRST.to_owned(), remark, FIRST.to_owned()]);
    assert_eq!(w.repeat_view(&run)["round"], 2);
}

/// A restart in the middle of the second execution: the count survives, so the step has no third.
#[tokio::test]
async fn a_crash_between_attempts_keeps_the_count() {
    let mut w = repeating(2, "1 1 0", "", "", POLICY);
    let turns = [w.repeat_turn(), w.repeat_turn()];
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Pass, &turns[0]).await;
    let started = Instant::now();
    while w.driver.token(&run, 2).is_none() {
        assert!(started.elapsed() < Duration::from_secs(10), "no attempt 2");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    w.restart().await;
    let turn = w.repeat_turn();
    w.until(Some(&run), "run.attempt_failed", 0).await;
    let resume = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, resume).await;
    w.repeat_declare(&run, 3, Verdict::Pass, &turn).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("check")),
        "{data}"
    );
    assert_eq!(w.check_calls(), 2, "a reset count would run the step a third time");
    let view = w.repeat_view(&run);
    assert_eq!(view["step_attempt"], 2);
    // The repeat's session was never recorded: the recovery starts another new one and does not go back to the
    // session of the first execution.
    let attempts = view["attempts"].as_array().unwrap();
    let kinds: Vec<&Value> = attempts.iter().map(|a| &a["kind"]).collect();
    assert_eq!(kinds, ["new_session"; 3], "{view}");
    assert_ne!(attempts[2]["session"], attempts[0]["session"], "{view}");
}

/// The script still runs when the daemon restarts: the new daemon runs it again, and the lost run counts for nothing.
#[tokio::test]
async fn a_check_script_cut_by_a_restart_runs_again_and_counts_once() {
    let body = "[ $n = 1 ] && { touch \"$0.reached\"; sleep 2; }";
    let mut w = repeating(2, "1 0", body, "", &[]);
    let turn = w.repeat_turn();
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Pass, &turn).await;
    let reached = w.dir.path().join("shop/formulas/check.sh.reached");
    let started = Instant::now();
    while !reached.exists() {
        assert!(started.elapsed() < Duration::from_secs(10), "the script never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    w.restart().await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.check_calls(), 2);
    assert_eq!(data["run"]["step_attempt"], 1, "{data}");
    assert_eq!(w.executions(&run), [], "no repeat");
}

#[tokio::test]
async fn a_check_script_gets_the_step_and_its_output_is_the_remark() {
    let formula = "name = \"build\"\n[[step]]\nname = \"make\"\nkind = \"exec\"\nmax_attempts = 2\n\
        step_check_script_path = \"check.sh\"\ncommand = [\"sh\", \"-c\", \"printf '[%s]\\\\n' \\\"$LOOP_MANAGER_REMARKS\\\" >> log\"]\n";
    let body =
        "echo \"$LOOP_MANAGER_RUN $LOOP_MANAGER_STEP_MAKE_RESULT $(pwd)\" >> seen; [ $n = 1 ] && echo 'add tests'";
    let script = script("1 0", body);
    let w = world(&[
        ("shop/formulas/build.toml", formula),
        ("shop/formulas/check.sh", &script),
    ]);
    let run = w.create().await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    let web = w.dir.path().join("shop/web");
    let seen = format!("{run} pass {}\n", web.display());
    assert_eq!(std::fs::read_to_string(web.join("seen")).unwrap(), seen.repeat(2));
    let log = std::fs::read_to_string(web.join("log")).unwrap();
    assert_eq!(log, "[]\n[make: check script: exit 1\nadd tests]\n");
}
