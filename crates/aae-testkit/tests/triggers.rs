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
    // The schedules below are written in UTC, whatever the zone of the machine.
    let config = format!("timezone = \"UTC\"\n{}", common::FILES[0].1);
    let extra = [
        ("shop/aae.toml", project),
        ("shop/formulas/next.toml", NEXT),
        ("config.toml", &config),
    ];
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

#[tokio::test(start_paused = true)]
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

/// A reload re-arms a schedule whose source changed, drops a removed one and arms a new one; the trigger listing
/// shows each next firing; a reload that needs a restart is refused and changes nothing.
#[tokio::test(start_paused = true)]
async fn a_reload_rearms_changed_schedules_and_the_listing_shows_the_next_firings() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), SCHEDULES);
    let (core, _task) = daemon(dir.path(), T0);
    // On the paused clock a sleep ends once every task is idle: the core has armed its schedules.
    tokio::time::sleep(Duration::from_millis(10)).await;
    let reader = run::Reader::open(&dir.path().join("state.db")).unwrap();
    let next = |config: &config::Config| {
        let list = reader.triggers(config).unwrap();
        let at = |name: &str| list.iter().find(|t| t["trigger"] == name).map(|t| t["next_at"].clone());
        (at("shop.nightly"), at("shop.hourly"), at("shop.weekly"))
    };
    let before = config::load(&dir.path().join("config.toml")).unwrap();
    let listed = reader.triggers(&before).unwrap();
    assert_eq!(listed[0]["schedule"], "0 3 * * *", "{listed:?}");
    assert_eq!(
        next(&before),
        (
            Some("2026-11-13T03:00:00.000Z".into()),
            Some("2026-11-13T03:59:30.000Z".into()),
            None
        )
    );

    let changed = SCHEDULES.replace("0 3 * * *", "0 4 * * *").replace(
        "name = \"hourly\"\nevery = \"1h\"",
        "name = \"weekly\"\nevery = \"168h\"",
    );
    write(dir.path(), &changed);
    let after = Arc::new(config::load(&dir.path().join("config.toml")).unwrap());
    let reply = core.send(None, Msg::Reload(after.clone())).await.unwrap();
    assert_eq!(reply["config_generation"], 2, "{reply}");
    let (nightly, hourly, weekly) = next(&after);
    assert_eq!((nightly, hourly), (Some("2026-11-13T04:00:00.000Z".into()), None));
    // A week from the reload, 10 ms of the managed clock after the start.
    assert_eq!(weekly, Some("2026-11-20T02:59:30.010Z".into()));

    // Another zone re-arms the cron: the next 04:00 in Tokyo (+9) is 19:00Z.
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        fs::read_to_string(&file).unwrap().replace("\"UTC\"", "\"Asia/Tokyo\""),
    )
    .unwrap();
    let after = Arc::new(config::load(&file).unwrap());
    core.send(None, Msg::Reload(after.clone())).await.unwrap();
    assert_eq!(next(&after).0, Some("2026-11-13T19:00:00.000Z".into()));

    let mut other = (*after).clone();
    other.harnesses.insert("codex".into(), Default::default());
    let refused = core.send(None, Msg::Reload(Arc::new(other))).await.unwrap_err();
    assert!(refused.to_string().contains("restart the daemon"), "{refused}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reloaded = journal(dir.path(), "config.reloaded");
    assert_eq!(reloaded.len(), 2);
    assert_eq!(reloaded[0]["data"]["restarted_instances"], serde_json::json!([]));
}

fn switch(trigger: &str, enabled: bool) -> Msg {
    Msg::SwitchTrigger {
        project: "shop".into(),
        rig: "web".into(),
        trigger: trigger.into(),
        enabled,
    }
}

/// A schedule switched off keeps its next firing and does not fire, across a restart; a hand firing still does; on
/// again, it fires on its grid.
#[tokio::test(start_paused = true)]
async fn a_trigger_switched_off_does_not_fire_until_it_is_on_again() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), SCHEDULES);
    let (core, task) = daemon(dir.path(), T0);
    let reply = core.send(None, switch("hourly", false)).await.unwrap();
    assert_eq!(reply, serde_json::json!({ "trigger": "shop.hourly", "enabled": false }));
    core.send(None, switch("hourly", false)).await.unwrap();
    assert_eq!(
        journal(dir.path(), "trigger.switched").len(),
        1,
        "a repeated switch changes nothing"
    );
    let reader = run::Reader::open(&dir.path().join("state.db")).unwrap();
    let config = config::load(&dir.path().join("config.toml")).unwrap();
    let listed = reader.triggers(&config).unwrap();
    let hourly = listed.iter().find(|t| t["trigger"] == "shop.hourly").unwrap();
    assert_eq!(
        (&hourly["enabled"], &hourly["next_at"]),
        (&Value::Bool(false), &"2026-11-13T03:59:30.000Z".into())
    );
    tokio::time::sleep(Duration::from_secs(2 * HOUR)).await;
    assert_eq!(
        (firings(dir.path(), "shop.nightly"), firings(dir.path(), "shop.hourly")),
        (1, 0)
    );

    task.abort();
    let _ = task.await;
    let (core, _task) = daemon(dir.path(), T0 + 3 * 3_600_000);
    tokio::time::sleep(Duration::from_secs(2 * HOUR)).await;
    assert_eq!(firings(dir.path(), "shop.hourly"), 0, "the switch outlives a restart");
    let fire = Msg::Fire {
        project: "shop".into(),
        rig: "web".into(),
        trigger: "hourly".into(),
    };
    assert!(core.send(None, fire).await.unwrap()["run"].is_string());
    core.send(None, switch("shop.hourly", true)).await.unwrap();
    // Past the next firing on the grid (the run fired by hand still lives: that one is skipped as an overlap).
    tokio::time::sleep(Duration::from_secs(HOUR + 60)).await;
    assert_eq!(firings(dir.path(), "shop.hourly"), 2);
}

/// An event trigger switched off does not fire on its events.
#[tokio::test(start_paused = true)]
async fn an_event_trigger_switched_off_does_not_fire() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), CHAIN);
    let (core, _task) = daemon(dir.path(), 0);
    core.send(None, switch("after-build", false)).await.unwrap();
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
    core.send(None, Msg::Create(create)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(firings(dir.path(), "shop.after-build"), 0);
    assert_eq!(by_trigger(&journal(dir.path(), "trigger.fired"), "shop.chain").len(), 0);
}

/// A trigger sees the events of its own rig, unless its `where` names the rig: `cross` on `web` follows `build` on
/// `api`, `own` on `web` does not.
#[tokio::test(start_paused = true)]
async fn a_where_on_rig_crosses_the_rigs_of_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = r#"
[rigs.web]
path = "web"
[rigs.api]
path = "web"

[[trigger]]
name = "cross"
on = "run.queued"
where = { rig = "api", "run.formula" = "shop.build" }
run = "next"
rig = "web"
inputs = { from = "{{ event.run.id }}" }

[[trigger]]
name = "own"
on = "run.queued"
where = { "run.formula" = "shop.build" }
run = "next"
rig = "web"
inputs = { from = "{{ event.run.id }}" }
"#;
    write(dir.path(), project);
    let (core, _task) = daemon(dir.path(), 0);
    for rig in ["api", "web"] {
        let create = Create {
            project: "shop".into(),
            rig: rig.into(),
            formula: "build".into(),
            inputs: serde_json::Map::new(),
            correlation: None,
            depth: 0,
            parent_run: None,
            continue_from: None,
        };
        core.send(None, Msg::Create(create)).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let fired = journal(dir.path(), "trigger.fired");
    let cross = by_trigger(&fired, "shop.cross");
    assert_eq!(cross.len(), 1, "{fired:?}");
    assert_eq!(cross[0]["rig"], "web");
    let api = journal(dir.path(), "run.queued")
        .into_iter()
        .find(|e| e["rig"] == "api")
        .unwrap();
    assert_eq!(cross[0]["data"]["cause_seq"], api["seq"]);
    // `own` has inputs from the event, so no overlap would hide a firing on the build of `api`.
    let own = by_trigger(&fired, "shop.own");
    assert_eq!(own.len(), 1, "only the build on web: {fired:?}");
    let web = journal(dir.path(), "run.queued")
        .into_iter()
        .find(|e| e["rig"] == "web" && e["data"]["run"]["formula"] == "shop.build");
    assert_eq!(own[0]["data"]["cause_seq"], web.unwrap()["seq"]);
    assert_eq!(by_trigger(&journal(dir.path(), "trigger.skipped"), "shop.own").len(), 0);
}

/// A trigger sees the events of its own project only: a build of `shop` does not wake the trigger of `mall`, whose
/// rig has the same name.
#[tokio::test(start_paused = true)]
async fn an_event_trigger_does_not_hear_another_project() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), CHAIN);
    let mall = r#"
[rigs.web]
path = "web"

[[trigger]]
name = "after-shop"
on = "run.queued"
where = { "run.formula" = "shop.build" }
run = "next"
rig = "web"
inputs = { from = "{{ event.run.id }}" }
"#;
    let config = "timezone = \"UTC\"\nprojects = [\"shop\", \"mall\"]\n\
        builtin = { default-policy = { enabled = false } }\n[harness.scripted]\n";
    for (path, text) in [
        ("config.toml", config),
        ("mall/aae.toml", mall),
        ("mall/web/.keep", ""),
        ("mall/agents/dev/agent.toml", "harness = \"scripted\"\n"),
        ("mall/formulas/next.toml", NEXT),
        ("mall/formulas/build.md", "Next."),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let (core, _task) = daemon(dir.path(), 0);
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
    core.send(None, Msg::Create(create)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(firings(dir.path(), "shop.after-build"), 1);
    assert_eq!(firings(dir.path(), "mall.after-shop"), 0);
}
