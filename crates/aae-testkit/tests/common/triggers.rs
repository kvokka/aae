//! Helpers of the trigger tests: a project with triggers on the scripted driver, the core on the managed clock,
//! and the firings in its journal.

use super::{FILES, Lost};
use aae_core::{
    Clock, CoreHandle, Error, Journal, config, open,
    run::{self, Msg},
};
use aae_testkit::ScriptedDriver;
use serde_json::Value;
use std::{fs, path::Path, sync::Arc};
use tokio::task::JoinHandle;

pub(crate) const NEXT: &str = "name = \"next\"\n[inputs]\nfrom = { type = \"string\" }\n\
    [step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n";

/// `after-build` starts `next` from every `build` run; `chain` starts `next` from every `next` run, so only the depth
/// limit ends the chain; `gated` names a gate the queued run does not have.
pub(crate) const CHAIN: &str = r#"
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

pub(crate) fn write(dir: &Path, project: &str) {
    // The schedules below are written in UTC, whatever the zone of the machine.
    let config = format!("timezone = \"UTC\"\n{}", FILES[0].1);
    let extra = [
        ("shop/aae.toml", project),
        ("shop/formulas/next.toml", NEXT),
        ("config.toml", &config),
    ];
    for (path, text) in FILES.iter().chain(&extra) {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// The core on the state of `dir`, as a daemon (re)started at `start_ms`.
pub(crate) fn daemon(dir: &Path, start_ms: i64) -> (CoreHandle<Msg>, JoinHandle<Result<(), Error>>) {
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

pub(crate) fn journal(dir: &Path, kind: &str) -> Vec<Value> {
    let all = Journal::new(&dir.join("journal")).read_from(1, 100_000).unwrap();
    all.into_iter().filter(|e| e["type"] == kind).collect()
}

/// Every firing of `trigger`, fired or skipped (the run of an earlier firing may still be alive).
pub(crate) fn firings(dir: &Path, trigger: &str) -> usize {
    let fired = by_trigger(&journal(dir, "trigger.fired"), trigger).len();
    fired + by_trigger(&journal(dir, "trigger.skipped"), trigger).len()
}

pub(crate) fn by_trigger(events: &[Value], trigger: &str) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e["data"]["trigger"] == trigger)
        .cloned()
        .collect()
}
