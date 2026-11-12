//! Daemon start (spec §5.6, criterion 8): a daemon that dies mid-run leaves no process behind once a new one starts,
//! and the attempt it was running fails with `daemon_crash` for the policy to decide.

mod common;

use aae_core::{
    Journal,
    run::{Decision, ExecHooks},
};
use common::{POLICY, World, last, world, world_with};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

fn journal(w: &World, kind: &str) -> Vec<Value> {
    let all = Journal::new(&w.journal).read_from(1, 100_000).unwrap();
    all.into_iter().filter(|e| e["type"] == kind).collect()
}

#[tokio::test]
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

#[tokio::test]
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
    let formula = "name = \"check\"\n[step]\nkind = \"exec\"\n\
        command = [\"sh\", \"-c\", \"echo $$ > pid; exec sleep 60\"]\ntimeout = \"120s\"\n";
    let mut w = world_with(&[("shop/formulas/check.toml", formula)], Arc::new(ExecHooks::default()));
    let create = aae_core::run::Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: "check".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        continue_from: None,
    };
    let reply = w.core.send(None, aae_core::run::Msg::Create(create)).await.unwrap();
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
