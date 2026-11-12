//! Triggers (spec §9.5, criteria 5b and 8): event triggers chain runs in one correlation up to the depth limit,
//! schedules fire on the managed clock, and a restart neither loses nor doubles a firing.

mod common;

use aae_core::{
    BoxFuture, Clock, CoreHandle, Error, Journal, config, open,
    run::{self, Create, HookRunner, Msg},
};
use aae_testkit::ScriptedDriver;
use serde_json::Value;
use std::{fs, path::Path, sync::Arc, time::Duration};
use tokio::task::JoinHandle;

const NEXT: &str = "name = \"next\"\n[inputs]\nfrom = { type = \"string\" }\n\
    [step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n";

/// `after-build` starts `next` from every `build` run; `chain` starts `next` from every `next` run, so only the depth
/// limit ends the chain; `gated` names a gate the queued run does not have.
const CHAIN: &str = r#"
[rigs.web]
path = "web"

[[trigger]]
name = "after-build"
on = "run.queued"
where = { "run.formula" = "shop.build" }
run = "next"
rig = "web"
inputs = { from = "{{ event.run.id }}" }

[[trigger]]
name = "chain"
on = "run.queued"
where = { "run.formula" = "shop.next" }
run = "next"
rig = "web"
inputs = { from = "{{ event.run.id }}" }

[[trigger]]
name = "gated"
on = "run.queued"
where = { "run.formula" = "shop.build" }
run = "next"
rig = "web"
inputs = { from = "{{ event.gate.key }}" }
"#;

const SCHEDULES: &str = r#"
[rigs.web]
path = "web"

[[trigger]]
name = "nightly"
cron = "0 3 * * *"
run = "build"
rig = "web"

[[trigger]]
name = "hourly"
every = "1h"
run = "build"
rig = "web"
"#;

/// 2026-11-13T02:59:30Z.
const T0: i64 = 1_794_538_770_000;
const HOUR: u64 = 3_600;

struct Lost;

impl HookRunner for Lost {
    fn run<'a>(&'a self, _: &'a CoreHandle<Msg>, _: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

fn write(dir: &Path, project: &str) {
    let extra = [("shop/aae.toml", project), ("shop/formulas/next.toml", NEXT)];
    for (path, text) in common::FILES.iter().chain(&extra) {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// The core on the state of `dir`, as a daemon (re)started at `start_ms`.
fn daemon(dir: &Path, start_ms: i64) -> (CoreHandle<Msg>, JoinHandle<Result<(), Error>>) {
    let config = config::load(&dir.join("config.toml")).unwrap();
    run::start(
        open(&dir.join("state.db")).unwrap(),
        Clock::managed(start_ms),
        Arc::new(config),
        vec![Arc::new(ScriptedDriver::new(1))],
        Default::default(),
        Arc::new(Lost),
        Journal::new(&dir.join("journal")),
    )
}

fn journal(dir: &Path, kind: &str) -> Vec<Value> {
    let all = Journal::new(&dir.join("journal")).read_from(1, 100_000).unwrap();
    all.into_iter().filter(|e| e["type"] == kind).collect()
}

/// Every firing of `trigger`, fired or skipped (the run of an earlier firing may still be alive).
fn firings(dir: &Path, trigger: &str) -> usize {
    let fired = by_trigger(&journal(dir, "trigger.fired"), trigger).len();
    fired + by_trigger(&journal(dir, "trigger.skipped"), trigger).len()
}

fn by_trigger(events: &[Value], trigger: &str) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e["data"]["trigger"] == trigger)
        .cloned()
        .collect()
}

#[tokio::test]
async fn an_event_chain_keeps_its_correlation_stops_at_the_depth_limit_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), CHAIN);
    let (core, task) = daemon(dir.path(), 0);
    let create = Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: "build".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        continue_from: None,
    };
    let build = core.send(None, Msg::Create(create)).await.unwrap()["run"].clone();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let queued = journal(dir.path(), "run.queued");
    let build_queued = queued.iter().find(|e| e["run"] == build).unwrap();
    let fired = journal(dir.path(), "trigger.fired");
    let first = by_trigger(&fired, "shop.after-build");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["data"]["cause_seq"], build_queued["seq"]);
    assert_eq!(first[0]["data"]["inputs"]["from"], build);
    // The build run, then nine `next` runs from `chain`: depths 1 to 10.
    assert_eq!(by_trigger(&fired, "shop.chain").len(), 9);
    let views: Vec<&Value> = queued.iter().map(|e| &e["data"]["run"]).collect();
    let depths: Vec<i64> = views.iter().filter_map(|v| v["depth"].as_i64()).collect();
    assert_eq!(depths.iter().max(), Some(&10));
    assert!(queued.iter().all(|e| e["correlation"] == build_queued["correlation"]));
    let parent = views.iter().find(|v| v["depth"] == 1).unwrap();
    assert_eq!(parent["parent_run"], build);

    let skipped = journal(dir.path(), "trigger.skipped");
    let reasons = |trigger: &str| -> Vec<String> {
        let mine = by_trigger(&skipped, trigger).into_iter();
        mine.map(|e| e["data"]["reason"].as_str().unwrap().to_owned()).collect()
    };
    // A queued run is queued again when the reason it waits for changes; its trigger then overlaps the live child.
    let chain = reasons("shop.chain");
    assert_eq!(chain.iter().filter(|r| *r == "depth").count(), 1);
    assert!(chain.iter().all(|r| r == "depth" || r == "overlap"));
    assert!(reasons("shop.gated").iter().all(|r| r == "input_unresolved"));

    // kill -9 and start again: every firing was committed with its cause, none happens twice.
    task.abort();
    let _ = task.await;
    let (_core, _task) = daemon(dir.path(), 60_000);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(journal(dir.path(), "trigger.fired").len(), 10);
}

#[tokio::test(start_paused = true)]
async fn a_schedule_fires_on_the_managed_clock_and_once_after_downtime() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), SCHEDULES);
    let (_core, task) = daemon(dir.path(), T0);
    tokio::time::sleep(Duration::from_secs(29)).await;
    assert!(journal(dir.path(), "trigger.fired").is_empty());
    tokio::time::sleep(Duration::from_secs(2)).await;
    let nightly = by_trigger(&journal(dir.path(), "trigger.fired"), "shop.nightly");
    assert_eq!(nightly.len(), 1);
    assert_eq!(nightly[0]["ts"], "2026-11-13T03:00:00.000Z");
    assert_eq!(nightly[0]["data"]["cause_seq"], Value::Null);

    // Down from 03:00 to 08:00: the hourly schedule missed five firings and makes up exactly one.
    task.abort();
    let _ = task.await;
    let restart = T0 + 30_000 + 5 * 3_600_000;
    let (_core, _task) = daemon(dir.path(), restart);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let hourly = by_trigger(&journal(dir.path(), "trigger.fired"), "shop.hourly");
    assert_eq!(hourly.len(), 1);
    assert_eq!(hourly[0]["ts"], "2026-11-13T08:00:00.000Z");
    // The next one comes an hour after the late one, not on the old grid.
    tokio::time::sleep(Duration::from_secs(HOUR - 2)).await;
    assert_eq!(firings(dir.path(), "shop.hourly"), 1);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(firings(dir.path(), "shop.hourly"), 2);
}
