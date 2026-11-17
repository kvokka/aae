//! The drain step across rounds and a root gone from the tracker (docs/design/config.md): a round back to the step is a
//! new entry, so a bead whose run failed gets another; a root gone mid-drain has nothing more under it.

use crate::common;

use common::{beads::slow_turn, drain::SWEEP, world};
use loop_manager_core::run::Verdict;
use serde_json::json;
use std::{collections::BTreeSet, time::Duration};

/// `sweep` with a check after the drain that goes back to it.
const AGAIN: &[(&str, &str)] = &[
    (
        "shop/formulas/again.toml",
        "name = \"again\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
         [[step]]\nname = \"all\"\nkind = \"drain\"\nformula = \"work\"\npool = 8\n\
         [[step]]\nname = \"check\"\nkind = \"ai\"\nagent = \"checker\"\nback_to = \"all\"\n",
    ),
    ("shop/agents/checker/agent.toml", "harness = \"scripted\"\n"),
    (
        "shop/agents/checker/prompt.template.md",
        "Check {{ steps.all.summary }}.",
    ),
];

#[tokio::test(start_paused = true)]
async fn a_round_back_to_the_drain_gives_a_failed_bead_another_run() {
    let w = world(&[SWEEP, AGAIN].concat());
    let root = w.tracker.add(&[], None);
    let bad = w.tracker.add_in("w", &[], Some(&root));
    let good = w.tracker.add_in("w", &[], Some(&root));
    for _ in 0..8 {
        w.driver.script(slow_turn());
    }
    let reply = w.start_bead("again", Some("web"), json!(root)).await.unwrap();
    let run = reply["run"].as_str().unwrap();
    let mut done = BTreeSet::new();
    let first = w.settle_nth(run, 0, &[bad.as_str()], &[], &mut done).await;
    assert_eq!(
        first["data"],
        json!({ "closed": [good], "failed": [bad], "refused": {} })
    );
    for _ in 0..60 {
        if w.driver.token(run, 2).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // The entry is told apart by milliseconds: the round must not start in the millisecond of the first children.
    tokio::time::sleep(Duration::from_secs(5)).await;
    w.end_attempt(run, 2, Verdict::Fail).await;
    // End #1 of a step is the check's `fail`.
    let second = w.settle_nth(run, 2, &[], &[], &mut done).await;
    assert_eq!(second["data"], json!({ "closed": [bad], "failed": [], "refused": {} }));
    let kids: Vec<String> = w.kids(run).into_iter().map(|k| k.1).collect();
    assert_eq!(kids, [bad.clone(), good, bad.clone()]);
    for _ in 0..60 {
        if w.driver.token(run, 4).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    w.end_attempt(run, 4, Verdict::Pass).await;
    w.until(Some(run), "run.passed", 0).await;
}

#[tokio::test(start_paused = true)]
async fn a_root_gone_mid_drain_ends_the_step_with_the_beads_given() {
    let w = world(SWEEP);
    let root = w.tracker.add(&[], None);
    let one = w.tracker.add_in("w", &[], Some(&root));
    let two = w.tracker.add_in("a", &[], Some(&root));
    let run = w.sweep(&root).await;
    let mut done = BTreeSet::new();
    w.settle(&run, &[], &[one.as_str()], &mut done).await;
    w.tracker.remove(&root);
    let outcome = w.settle(&run, &[], &[], &mut done).await;
    assert_eq!(
        outcome["data"],
        json!({ "closed": [one, two], "failed": [], "refused": {} })
    );
}
