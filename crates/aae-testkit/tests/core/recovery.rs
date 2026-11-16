//! Daemon start (spec §5.6, criterion 8): a daemon that dies mid-run leaves no process behind once a new one starts,
//! and the attempt it was running fails with `daemon_crash` for the policy to decide. On the paused clock, except the
//! tests whose exec step runs a real command.

use crate::common;

use aae_core::{
    Journal,
    run::{Decision, ExecHooks, Msg},
};
use common::{POLICY, World, hollow_turn, last, world, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, task::Poll, time::Duration};

fn journal(w: &World, kind: &str) -> Vec<Value> {
    let all = Journal::new(&w.journal).read_from(1, 100_000).unwrap();
    all.into_iter().filter(|e| e["type"] == kind).collect()
}

#[tokio::test(start_paused = true)]
async fn a_restart_interrupts_the_running_attempt_stops_its_instance_and_the_run_goes_on() {
    let mut w = world(POLICY);
    let run = w.create().await;
    assert_eq!(w.states(&run).await.last().unwrap(), "running");
    w.restart().await;

    let events = w.events(&run, Duration::from_millis(300)).await;
    let changed = last(&events, "run.state_changed");
    assert_eq!(
        (&changed["to"], &changed["reason"]),
        (&json!("interrupted"), &json!("daemon_crash"))
    );
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "daemon_crash");
    let started = journal(&w, "daemon.started");
    assert_eq!(started.len(), 2, "one per start");
    assert_eq!(started[0]["data"]["interrupted_runs"], 0);
    assert_eq!(started[1]["data"]["interrupted_runs"], 1);
    let stopped = journal(&w, "instance.stopped");
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0]["data"]["reason"], "recovery");

    let decision = Decision::NewSession {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, decision).await;
    let states = w.states(&run).await;
    assert_eq!(states[states.len() - 3..], ["interrupted", "starting", "running"]);
    assert_eq!(
        journal(&w, "instance.starting").len(),
        2,
        "a new instance for the new attempt"
    );
}

/// A drain starts no process (spec §5.6): a retry decided meanwhile parks, and the next daemon starts it.
#[tokio::test(start_paused = true)]
async fn a_draining_daemon_parks_a_retry_and_the_next_daemon_starts_it() {
    let mut w = world(POLICY);
    let run = w.create().await;
    assert_eq!(w.states(&run).await.last().unwrap(), "running");
    w.restart().await;
    w.until(Some(&run), "run.attempt_failed", 0).await;
    let stopping = Msg::Stopping { reason: "drain".into() };
    w.core.send(None, stopping).await.unwrap();
    let decision = Decision::NewSession {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(&run, "run.attempt_failed", 0, decision).await;
    let parked = w.until(Some(&run), "run.parked", 0).await;
    assert_eq!(parked["gate"], Value::Null, "{parked}");
    assert_eq!(w.states(&run).await.last().unwrap(), "parked");
    assert_eq!(journal(&w, "instance.starting").len(), 1, "no process while draining");

    w.restart().await;
    w.until(Some(&run), "run.resumed", 0).await;
    w.events(&run, Duration::from_millis(300)).await;
    let states = w.states(&run).await;
    assert_eq!(states[states.len() - 3..], ["parked", "starting", "running"]);
    assert_eq!(journal(&w, "run.resumed")[0]["data"]["gate_key"], "daemon_stopping");
    assert_eq!(journal(&w, "instance.starting").len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_restart_with_nothing_running_changes_no_run() {
    let mut w = world(&[]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    w.restart().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = journal(&w, "daemon.started");
    assert_eq!(started.len(), 2);
    assert_eq!(started[1]["data"]["interrupted_runs"], 0);
    assert!(journal(&w, "run.state_changed").is_empty());
}

/// The command of an exec step survives its daemon in its own process group; the next daemon kills it by its marker
/// before any later attempt runs the command again.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_restart_kills_the_command_of_an_exec_step_the_dead_daemon_left() {
    let formula = "name = \"check\"\n[[step]]\nkind = \"exec\"\n\
        command = [\"sh\", \"-c\", \"echo $$ > pid; exec sleep 60\"]\ntimeout = \"120s\"\n";
    let mut w = world_with(&[("shop/formulas/check.toml", formula)], Arc::new(ExecHooks::default()));
    let reply = w.core.send(None, aae_core::run::Msg::Create(check())).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    let pid_file = w.dir.path().join("shop/web/pid");
    let pid = wait_for(|| std::fs::read_to_string(&pid_file).ok().filter(|p| p.ends_with('\n'))).await;
    let pid: i32 = pid.trim().parse().unwrap();
    assert!(alive(pid), "the command runs");

    w.restart().await;
    wait_for(|| (!alive(pid)).then_some(())).await;
    let events = w.events(&run, Duration::from_millis(100)).await;
    assert_eq!(last(&events, "run.attempt_failed")["failure"]["kind"], "daemon_crash");
}

/// The command of an exec step ended and its run with it, but the daemon died before it marked the effect done: the
/// next one does not run the command again (contracts 6.4).
#[tokio::test]
async fn a_restart_does_not_run_again_a_command_whose_run_has_ended() {
    let formula = "name = \"check\"\n[[step]]\nkind = \"exec\"\n\
        command = [\"sh\", \"-c\", \"echo ran >> runs.log; sleep 0.5; exit 3\"]\ntimeout = \"60s\"\n";
    let mut w = world_with(&[("shop/formulas/check.toml", formula)], Arc::new(ExecHooks::default()));
    let reply = w.core.send(None, aae_core::run::Msg::Create(check())).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let effect = wait_for(|| {
        db.query_row(
            "SELECT key, dedupe, payload FROM effects WHERE key = ?1",
            [format!("exec:{run}")],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .ok()
    })
    .await;
    w.until(Some(&run), "run.failed", 0).await;
    // The daemon dies between the end of the run and the end of its effect.
    w.killer().abort();
    tokio::time::sleep(Duration::from_millis(100)).await;
    db.execute(
        "INSERT OR IGNORE INTO effects (key, dedupe, payload, next_at_ms) VALUES (?1, ?2, ?3, 0)",
        [&effect.0, &effect.1, &effect.2],
    )
    .unwrap();
    w.restart().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let log = std::fs::read_to_string(w.dir.path().join("shop/web/runs.log")).unwrap();
    assert_eq!(log, "ran\n");
}

/// Invariant 6 on the exec step: the next attempt of the run, decided before the new daemon's first effect, starts
/// its command only once the sweep of the command the dead daemon left is done and nothing carrying its marker lives.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_next_attempt_of_an_exec_step_starts_only_once_the_command_left_is_gone() {
    // The first attempt leaves its marker and a process group behind; the next one notes a process still carrying
    // that marker.
    let script = "if [ -e old ]; then : > started; grep -qsxz \"AAE_INSTANCE=$(cat old)\" /proc/[0-9]*/environ \
        && echo overlap >> log; echo next >> log; exit 0; fi; \
        sleep 60 & echo $AAE_INSTANCE > old.tmp; mv old.tmp old; wait";
    let formula = format!(
        "name = \"check\"\n[[step]]\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", {script:?}]\ntimeout = \"120s\"\n"
    );
    let mut files = common::POLICY.to_vec();
    files.push(("shop/formulas/check.toml", &formula));
    let mut w = world_with(&files, Arc::new(ExecHooks::default()));
    let reply = w.core.send(None, Msg::Create(check())).await.unwrap();
    let run = reply["run"].as_str().unwrap().to_owned();
    let rig = w.dir.path().join("shop/web");
    wait_for(|| rig.join("old").exists().then_some(())).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let last = Journal::new(&w.journal).read_from(1, 100_000).unwrap().last().unwrap()["seq"]
        .as_i64()
        .unwrap();

    w.restart().await;
    // In the inbox before the new writer runs: the decision lands in the pass that queues the sweep. Every seq but
    // the one of `run.attempt_failed` is refused.
    let decide = |seq| {
        let msg = Msg::Decide {
            run: run.clone(),
            seq,
            decision: Decision::NewSession {
                after: Duration::ZERO,
                gate: None,
            },
        };
        Box::pin(w.core.send(None, msg))
    };
    let (mut sends, mut applied): (Vec<_>, _) = ((last + 1..=last + 40).map(decide).collect(), 0);
    // Polled together, so that all are queued at the first poll.
    std::future::poll_fn(|cx| {
        sends.retain_mut(|send| match send.as_mut().poll(cx) {
            Poll::Ready(reply) => {
                applied += usize::from(reply.is_ok());
                false
            }
            Poll::Pending => true,
        });
        if sends.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    assert_eq!(applied, 1, "one decision applies");
    // Looked at as soon as the command starts: a sweep still queued then may kill the old command after it.
    for _ in 0..5000 {
        if rig.join("started").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let sql = "SELECT COUNT(*) FROM effects WHERE json_extract(payload, '$.op') = 'sweep'";
    let sweeps: i64 = db.query_row(sql, [], |r| r.get(0)).unwrap();
    assert_eq!(sweeps, 0, "the sweep was done before the command ran again");
    w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(std::fs::read_to_string(rig.join("log")).unwrap(), "next\n");
}

fn check() -> aae_core::run::Create {
    aae_core::run::Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "check".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    }
}

/// Live and not a zombie.
fn alive(pid: i32) -> bool {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    stat.rsplit_once(')')
        .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
}

async fn wait_for<T>(mut probe: impl FnMut() -> Option<T>) -> T {
    for _ in 0..100 {
        if let Some(found) = probe() {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("not within 5 s");
}

/// `(attempt, n, key, state)` of every prompt of `run`, from the delivery record.
fn prompts(w: &World, run: &str) -> Vec<(u32, u32, String, String)> {
    let db = rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap();
    let mut stmt = db
        .prepare("SELECT attempt, n, key, state FROM prompts WHERE run = ?1 ORDER BY attempt, n")
        .unwrap();
    let rows = stmt.query_map([run], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)));
    rows.unwrap().collect::<Result<_, _>>().unwrap()
}

/// The run resumes its session after a restart under it.
async fn resume_after_restart(w: &mut World, run: &str) -> Vec<String> {
    w.restart().await;
    let decision = Decision::Resume {
        after: Duration::ZERO,
        gate: None,
    };
    w.decide(run, "run.attempt_failed", 0, decision).await;
    let states = w.states(run).await;
    assert_eq!(states[states.len() - 3..], ["interrupted", "starting", "running"]);
    w.driver.sent(run, 2).1
}

#[tokio::test(start_paused = true)]
async fn a_prompt_the_harness_never_got_goes_again_with_its_key_after_a_restart() {
    let mut w = world(POLICY);
    w.driver.lose_prompts(true);
    let run = w.create().await;
    wait_for(|| (!w.driver.keys().is_empty()).then_some(())).await;
    let texts = resume_after_restart(&mut w, &run).await;
    assert_eq!(texts, ["Build web.", "Build web."], "the same prompt, no reconcile");
    let keys = w.driver.keys();
    assert_eq!((keys.len(), keys[0]), (2, keys[1]), "with the same key");
    let rows = prompts(&w, &run);
    assert_eq!((&rows[0].3, &rows[1].3), (&"sent".to_owned(), &"acked".to_owned()));
    assert_eq!(rows[0].2, rows[1].2);
}

#[tokio::test(start_paused = true)]
async fn a_prompt_the_harness_got_brings_the_reconcile_prompt_with_a_new_key_after_a_restart() {
    let mut w = world(POLICY);
    let run = w.create().await;
    assert_eq!(w.states(&run).await.last().unwrap(), "running");
    let texts = resume_after_restart(&mut w, &run).await;
    assert!(texts[1].starts_with("Your previous turn was interrupted"), "{texts:?}");
    let keys = w.driver.keys();
    assert!(keys.len() == 2 && keys[0] != keys[1], "{keys:?}");
    let states: Vec<_> = prompts(&w, &run).into_iter().map(|p| p.3).collect();
    assert_eq!(states, ["acked", "acked"]);
}

#[tokio::test(start_paused = true)]
async fn a_prompt_whose_turn_ended_is_settled() {
    let w = world(&[]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    w.states(&run).await;
    let rows = prompts(&w, &run);
    assert_eq!((rows.len(), rows[0].1, rows[0].3.as_str()), (1, 1, "settled"));
}
