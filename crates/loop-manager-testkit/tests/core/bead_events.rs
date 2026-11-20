//! Tracker writes as bead events on the scripted driver and the tracker in memory (docs/design/tracker-br.md,
//! docs/design/contracts.md 1).

use crate::common;

use common::{
    beads::{FIX, slow_turn},
    world,
};
use loop_manager_core::Journal;
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
    let new = w.tracker.add_in("w", &[], None);
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

/// A project trigger on a label gained by a ready bead, with a label other than devloop's `auto`.
const ON_REVIEW: &str = "[rigs.web]\npath = \"web\"\n\n[[trigger]]\nname = \"on-review\"\non = \"bead.labeled\"\n\
     where = { label = \"review\", ready = true }\nrun = \"fix\"\nrig = \"web\"\n\
     inputs = { bead = \"{{ event.bead.id }}\" }\n";

#[tokio::test(start_paused = true)]
async fn a_label_a_ready_bead_gains_fires_a_trigger_once_and_nothing_else_does() {
    let mut files = FIX.to_vec();
    files.push(("shop/loop-manager.toml", ON_REVIEW));
    let w = world(&files);
    let fired = || w.all("trigger.fired");
    let labeled = || w.all("bead.labeled");
    let ready = w.tracker.add_in("w", &["ui"], None);
    let held = w.tracker.add_in("w", &[], None);
    w.changed().await;
    // A bead a human holds is not ready: the label it gains is told, and starts nothing.
    w.tracker.assign(&held, "bob");
    w.tracker.label(&held, "review");
    w.changed().await;
    assert_eq!(
        (labeled().len(), &labeled()[0]["label"], &labeled()[0]["ready"]),
        (1, &json!("review"), &json!(false))
    );
    assert!(fired().is_empty(), "{:?}", fired());

    // Another label, then an edit of no label: no run.
    w.tracker.label(&ready, "docs");
    w.tracker.retitle(&ready, "Renamed");
    w.changed().await;
    let docs = labeled().pop().unwrap();
    assert_eq!(
        (labeled().len(), &docs["label"], &docs["ready"], &docs["actor"]),
        (2, &json!("docs"), &json!(true), &json!("human"))
    );
    assert!(fired().is_empty(), "{:?}", fired());

    w.driver.script(slow_turn());
    w.tracker.label(&ready, "review");
    w.changed().await;
    let fired_once = fired();
    assert_eq!(fired_once.len(), 1, "{fired_once:?}");
    assert_eq!(fired_once[0]["inputs"]["bead"], json!(ready));
    let review = labeled().pop().unwrap();
    assert_eq!(
        (&review["bead"]["labels"], &review["ready"]),
        (&json!(["ui", "docs", "review"]), &json!(true))
    );
    // Reading the same tracker again, or a write on another bead, sees no new label.
    w.changed().await;
    w.tracker.retitle(&held, "Renamed");
    w.changed().await;

    // A bead filed with the label, or labelled before the core saw it ready, is new to the ready set: `bead.ready`
    // tells it, and the label it gained is not of a ready bead.
    let filed = w.tracker.add_in("w", &["review"], None);
    let late = w.tracker.add_in("w", &[], None);
    w.tracker.label(&late, "review");
    w.changed().await;
    let of = |id: &str| {
        labeled()
            .into_iter()
            .filter(|e| e["bead"]["id"] == id)
            .collect::<Vec<_>>()
    };
    assert!(of(&filed).is_empty(), "{:?}", of(&filed));
    assert_eq!(of(&late)[0]["ready"], json!(false));
    let new_ready = w.all("bead.ready");
    assert!(
        [&filed, &late]
            .iter()
            .all(|id| new_ready.iter().any(|e| e["bead"]["id"] == ***id)),
        "{new_ready:?}"
    );
    assert_eq!(fired().len(), 1, "{:?}", fired());
}
