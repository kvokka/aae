//! The exits of a check script other than its code (spec §9.1, contracts 6.1 `finishing`): a timeout, a signal and a
//! lost answer do not pass the step, a script that cannot start fails the run. Real processes, so real time.

use crate::{
    common::World,
    step_repeat::{FIRST, again, repeating},
};

use aae_core::run::{Msg, Verdict};
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

/// The step `work` with `more` keys whose check script runs `first` on its first call and passes on its second: the
/// run passes on the second execution, whose prompt carries `remark`. `between` runs while the first script runs.
async fn second_passes<T>(first: &str, more: &str, remark: &str, between: impl AsyncFnOnce(&World, &str) -> T) -> T {
    let w = repeating(
        2,
        "0",
        &format!("[ $n = 1 ] && {{ echo $$ > \"$0.pid\"; {first}; }}"),
        more,
        &[],
    );
    let turns = [w.repeat_turn(), w.repeat_turn()];
    let run = w.create().await;
    w.repeat_declare(&run, 1, Verdict::Pass, &turns[0]).await;
    let between = between(&w, &run).await;
    w.repeat_declare(&run, 2, Verdict::Pass, &turns[1]).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(kind, "run.passed", "{data}");
    assert_eq!(w.check_calls(), 2);
    assert_eq!(w.driver.sent(&run, 1).1, [FIRST.to_owned(), again(remark)]);
    between
}

async fn nothing(_: &World, _: &str) {}

#[tokio::test]
async fn a_check_script_past_the_step_timeout_does_not_pass_it() {
    let started = Instant::now();
    second_passes(
        "exec sleep 30",
        "timeout = \"1s\"\n",
        "check script: timed out",
        nothing,
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the timeout of the step bounds its script"
    );
}

#[tokio::test]
async fn a_check_script_killed_by_a_signal_does_not_pass_it() {
    second_passes("kill -KILL $$", "", "check script: signal 9", nothing).await;
}

/// The answer of the script never comes: the `state` deadline of `finishing` stands for it (invariant 14).
#[tokio::test]
async fn a_lost_check_script_is_a_timeout_at_the_deadline_of_finishing() {
    let lose = async |w: &World, run: &str| {
        let pid = w.dir.path().join("shop/formulas/check.sh.pid");
        let started = Instant::now();
        while !std::fs::read_to_string(&pid).is_ok_and(|p| p.ends_with('\n')) {
            assert!(started.elapsed() < Duration::from_secs(10), "the script never ran");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
        db.busy_timeout(Duration::from_secs(5)).unwrap();
        let id = format!("{run}:state");
        let moved = db
            .execute("UPDATE deadlines SET at_ms = 0 WHERE id = ?1", [&id])
            .unwrap();
        assert_eq!(moved, 1, "`finishing` has its `state` deadline");
        // Any message wakes the writer, which fires what is due; the second execution waits for it.
        w.core.send(None, Msg::TrackerChanged("shop".into())).await.unwrap();
        let pid = std::fs::read_to_string(&pid).unwrap();
        pid.trim().to_owned()
    };
    let pid = second_passes("exec sleep 30", "timeout = \"1h\"\n", "check script: timed out", lose).await;
    // The lost script still sleeps: its late answer would be of an attempt gone by.
    let _ = std::process::Command::new("kill").arg(pid).status();
}

#[tokio::test]
async fn a_check_script_that_cannot_start_fails_the_run() {
    let w = repeating(3, "0", "", "", &[]);
    let turn = w.repeat_turn();
    let run = w.create().await;
    let script = w.dir.path().join("shop/formulas/check.sh");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    w.repeat_declare(&run, 1, Verdict::Pass, &turn).await;
    let (kind, data) = w.repeat_end(&run).await;
    assert_eq!(
        (kind.as_str(), &data["class"]),
        ("run.failed", &json!("infrastructure")),
        "{data}"
    );
    assert!(data["reason"].as_str().unwrap().starts_with("check script: "), "{data}");
    assert_eq!(w.check_calls(), 0);
    assert_eq!(w.executions(&run), [], "no repeat");
}
