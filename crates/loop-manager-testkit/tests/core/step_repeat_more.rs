//! Repeats of a drain step and the cancel of a run whose check script runs (docs/design/config.md). Real processes, so
//! real time.

use crate::{
    common::{self, World, drain::SWEEP, world},
    step_repeat::{repeating, script},
};

use loop_manager_core::run::{Msg, Verdict};
use serde_json::json;
use std::time::{Duration, Instant};

/// Waits up to 10 s for `done`.
async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(10), "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The children of `run`, waiting up to 10 s for `n` of them.
async fn kids(w: &World, run: &str, n: usize) -> Vec<common::drain::Kid> {
    until(&format!("no child #{n} of {run}"), || w.kids(run).len() >= n).await;
    w.kids(run)
}

/// The bead whose child failed in the first entry is given again in the second: a repeat is a new entry.
#[tokio::test]
async fn a_repeat_of_a_drain_step_is_a_new_entry() {
    let sweep = "name = \"sweep\"\n[inputs]\nbead = { type = \"task\", required = true }\n[[step]]\nname = \"all\"\n\
        kind = \"drain\"\nformula = \"work\"\nmax_attempts = 2\nstep_check_script_path = \"check.sh\"\n";
    let check = script("1 0", "");
    let files = [
        SWEEP,
        &[("shop/formulas/sweep.toml", sweep), ("shop/formulas/check.sh", &check)],
    ]
    .concat();
    let w = world(&files);
    let root = w.tracker.add(&[], None);
    let bead = w.tracker.add_in("w", &[], Some(&root));
    let turns = [w.repeat_turn(), w.repeat_turn()];
    let reply = w.start_bead("sweep", Some("web"), json!(root)).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    let first = kids(&w, &run, 1).await[0].0.clone();
    w.repeat_declare(&first, 1, Verdict::Fail, &turns[0]).await;
    let second = kids(&w, &run, 2).await[1].0.clone();
    w.repeat_declare(&second, 1, Verdict::Pass, &turns[1]).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.check_calls(), 2);
    let kids: Vec<(String, String)> = w.kids(&run).into_iter().map(|k| (k.1, k.2)).collect();
    let at = |state: &str| (bead.clone(), state.to_owned());
    assert_eq!(kids, [at("failed"), at("passed")]);
    let entries = w.all("run.step_finished").into_iter().filter(|d| d["run"]["id"] == run);
    // The word of the check script is the outcome of the first entry.
    let said: Vec<_> = entries.map(|d| d["outcome"]["summary"].clone()).collect();
    assert_eq!(said, ["check script: exit 1"]);
    assert_eq!(w.tracker.bead(&bead).unwrap().status, "closed");
}

/// Whether process `pid` is gone: no entry in `/proc`, or a zombie.
fn gone(pid: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit_once(") ").is_some_and(|(_, rest)| rest.starts_with('Z')))
        || !std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[tokio::test]
async fn a_cancel_stops_the_check_script_and_the_session() {
    let w = repeating(2, "0", "echo $$ > \"$0.pid\"; exec sleep 30", "", &[]);
    let turn = w.repeat_turn();
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Pass, &turn).await;
    let pid_file = w.dir.path().join("shop/formulas/check.sh.pid");
    let read = || std::fs::read_to_string(&pid_file).unwrap_or_default();
    until("the script never ran", || read().ends_with('\n')).await;
    let pid = read().trim().to_owned();
    assert_eq!(w.states(&run).await.last().unwrap(), "finishing");
    assert!(!gone(&pid));
    let session = w.repeat_view(&run)["attempts"][0]["session"].clone();
    // A session still held by its process keeps its transcript when the detached ones are lost.
    w.driver.lose_transcripts();
    assert!(w.driver.has_transcript(&session), "{session}");
    let cancel = Msg::Cancel {
        run: run.clone(),
        reason: "operator".into(),
    };
    w.core.send(None, cancel).await.unwrap();
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.cancelled", "{data}");
    until("the script outlived the cancel", || gone(&pid)).await;
    let closed = || {
        w.driver.lose_transcripts();
        !w.driver.has_transcript(&session)
    };
    until("the session was not stopped", closed).await;
    assert_eq!(w.check_calls(), 1);
}

/// The check script of an ai step gets the environment of an exec step, `[exec]` of the project, not the agent's `env`
/// (docs/design/config.md).
#[tokio::test]
async fn the_check_script_of_an_ai_step_gets_the_exec_environment() {
    let extra = [
        (
            "shop/loop-manager.toml",
            "[rigs.web]\npath = \"web\"\n[exec]\nenv = { LIT = \"x\" }\n",
        ),
        (
            "shop/agents/dev/agent.toml",
            "harness = \"scripted\"\nenv = { AGENT_ONLY = \"y\" }\n",
        ),
    ];
    let w = repeating(1, "0", "echo \"$LIT ${AGENT_ONLY-none}\" > \"$0.env\"", "", &extra);
    let turn = w.repeat_turn();
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Pass, &turn).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    let seen = std::fs::read_to_string(w.dir.path().join("shop/formulas/check.sh.env")).unwrap();
    assert_eq!(seen, "x none\n");
    let agent = w.driver.sent(&run, 1).0.unwrap();
    assert_eq!(
        (agent.get("AGENT_ONLY").map(String::as_str), agent.get("LIT")),
        (Some("y"), None),
        "the session has the agent's env only"
    );
}

/// A check script whose `[exec]` names a variable the daemon lacks is not started: the run fails as `config_error`
/// naming the key and the variable, with no repeat.
#[tokio::test]
async fn a_check_script_with_a_missing_variable_fails_the_run_as_config_error() {
    let own = "[rigs.web]\npath = \"web\"\n[exec]\nenv = { GH_TOKEN = \"${LOOP_MANAGER_TEST_NO_SUCH_VARIABLE}\" }\n";
    let w = repeating(2, "0", "", "", &[("shop/loop-manager.toml", own)]);
    let turn = w.repeat_turn();
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Pass, &turn).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("infrastructure")),
        "{data}"
    );
    let reason = data["reason"].as_str().unwrap();
    assert!(
        reason.starts_with("config_error: check script: env `GH_TOKEN`")
            && reason.contains("`LOOP_MANAGER_TEST_NO_SUCH_VARIABLE`"),
        "{reason}"
    );
    assert_eq!(w.check_calls(), 0);
}
