//! The scope of an event trigger (spec §9.5): it hears its own rig unless its `where` names another, and never
//! another project.

mod common;

use aae_core::run::{Create, Msg};
use common::triggers::{CHAIN, NEXT, by_trigger, daemon, firings, journal, write};
use std::{fs, time::Duration};

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
