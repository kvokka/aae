//! Tracker writes as bead events on the scripted driver and the tracker in memory (spec §10.3, §5.4).

mod common;

use aae_core::Journal;
use common::{
    beads::{FIX, slow_turn},
    world,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn tracker_writes_become_bead_events_and_a_write_by_a_run_carries_its_chain() {
    let w = world(FIX);
    let old = w.tracker.add(&[], None);
    w.changed().await;
    assert!(
        w.all("bead.created").is_empty(),
        "the history before the first read raises nothing"
    );
    let ready = w.all("bead.ready");
    assert_eq!((ready.len(), &ready[0]["bead"]["id"]), (1, &json!(old)));
    let new = w.tracker.add(&["rig:web"], None);
    w.changed().await;
    let created = w.all("bead.created");
    assert_eq!(created.len(), 1, "{created:?}");
    assert_eq!(
        (&created[0]["bead"]["id"], &created[0]["actor"]),
        (&json!(new), &json!("human"))
    );
    assert_eq!(w.all("bead.ready").len(), 2, "each bead is ready once");
    w.driver.script(slow_turn());
    let run = w.run_on(json!(new)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    w.changed().await;
    let all = Journal::new(&w.journal).read_from(1, 100_000).unwrap();
    let claimed = all.iter().rfind(|e| e["type"] == "bead.updated").unwrap();
    let queued = all.iter().find(|e| e["type"] == "run.queued").unwrap();
    assert_eq!(
        (&claimed["run"], &claimed["rig"], &claimed["correlation"]),
        (&json!(run), &json!("web"), &queued["correlation"]),
        "the claim is the run's write"
    );
    assert_eq!(claimed["data"]["changed"], json!(["status", "assignee"]));
}

#[tokio::test(start_paused = true)]
async fn a_write_whose_signal_is_lost_is_read_by_the_periodic_resync() {
    let w = world(FIX);
    w.changed().await;
    let lost = w.tracker.add(&[], None);
    tokio::time::sleep(Duration::from_secs(4 * 60)).await;
    assert!(w.all("bead.created").is_empty(), "no signal, no read yet");
    tokio::time::sleep(Duration::from_secs(61)).await;
    let created = w.all("bead.created");
    assert_eq!(created.len(), 1, "{created:?}");
    assert_eq!(created[0]["bead"]["id"], json!(lost));
}
