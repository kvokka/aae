//! A prompt shows the comments of the run's bead and its parents with theirs, read from the tracker each time the
//! run enters the step, as TOON (docs/design/contracts.md 2.3, 5.1).

use crate::common;

use common::{World, beads::slow_turn, last, world};
use loop_manager_core::run::{Msg, Verdict};
use loop_manager_core::tracker::Tracker;
use serde_json::json;
use std::time::Duration;

const FIXER: &str =
    "Fix {{ bead.title }}.\nComments:\n{{ bead.comments }}\nParents:\n{{ bead.parents }}\n{{ remarks }}";

/// `fix` on a task: `code` by the fixer (with the keys `code`), then `review` by qa, which sends it back to `code`.
fn comments_world(code: &str) -> World {
    let formula = format!(
        "name = \"fix\"\n[inputs]\nbead = {{ type = \"task\", required = true }}\n\
         [[step]]\nname = \"code\"\nkind = \"ai\"\nagent = \"fixer\"\n{code}\n\
         [[step]]\nname = \"review\"\nkind = \"ai\"\nagent = \"qa\"\nback_to = \"code\"\n"
    );
    let files = [
        ("shop/agents/fixer/agent.toml", "harness = \"scripted\"\n"),
        ("shop/agents/fixer/prompt.template.md", FIXER),
        ("shop/agents/qa/agent.toml", "harness = \"scripted\"\n"),
        ("shop/agents/qa/prompt.template.md", "Review {{ bead.title }}."),
        ("shop/formulas/fix.toml", formula.as_str()),
    ];
    world(&files)
}

/// The last prompt sent to any session.
fn last_prompt(w: &World, run: &str, attempt: u32) -> String {
    w.driver.sent(run, attempt).1.last().cloned().unwrap_or_default()
}

impl World {
    /// Starts `fix` on `bead` and plays it to the `review` step: `code` passes, the review's prompt is sent.
    async fn to_review(&self, bead: &str) -> String {
        for _ in 0..3 {
            self.driver.script(slow_turn());
        }
        let run = self.run_on(json!(bead)).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.finish_attempt(&run, 1, Verdict::Pass, "coded").await;
        tokio::time::sleep(Duration::from_secs(12)).await;
        assert_eq!(last_prompt(self, &run, 2), format!("Review Task {bead}."));
        run
    }
}

#[tokio::test(start_paused = true)]
async fn a_prompt_shows_the_comments_of_the_bead_and_its_parents_from_the_root_down_as_toon() {
    let w = comments_world("");
    let root = w.tracker.add(&[], None);
    let mid = w.tracker.add(&[], Some(&root));
    let bead = w.tracker.add(&[], Some(&mid));
    w.tracker.comment(&root, "alice", "The goal: ship it");
    w.tracker.comment(&bead, "bob", "start with the parser");
    w.tracker.comment(&bead, "carol", "two lines\nhere");
    w.driver.script(slow_turn());
    let run = w.run_on(json!(bead)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let prompt = last_prompt(&w, &run, 1);
    let comments = "Comments:\n[2]{author,created_at,text}:\n  bob,\"2026-11-13T00:00:00Z\",start with the parser\n  \
                    carol,\"2026-11-13T00:00:00Z\",\"two lines\\nhere\"\nParents:\n[2]:\n";
    assert!(prompt.contains(comments), "{prompt}");
    assert!(
        prompt.contains("\n      alice,\"2026-11-13T00:00:00Z\",\"The goal: ship it\"\n"),
        "{prompt}"
    );
    let at = |line: &str| prompt.find(line).unwrap_or_else(|| panic!("no `{line}` in {prompt}"));
    assert!(at(&format!("    id: {root}\n")) < at(&format!("    id: {mid}\n")));
    let mid_part = &prompt[at(&format!("    id: {mid}\n"))..];
    assert!(prompt.contains("    comments: []\n"), "{prompt}");
    assert!(
        !mid_part.contains("alice"),
        "the root's comments are the root's: {prompt}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_return_to_a_step_reads_the_comments_written_since_and_last_comments_keeps_the_last() {
    let w = comments_world("last_comments = 1");
    let parent = w.tracker.add(&[], None);
    let bead = w.tracker.add(&[], Some(&parent));
    w.tracker.comment(&parent, "alice", "the old goal");
    w.tracker.comment(&parent, "alice", "the new goal");
    let run = w.to_review(&bead).await;
    let first = &w.driver.sent(&run, 1).1[0];
    assert!(first.contains("Comments:\n[]\n"), "{first}");
    w.tracker.comment(&bead, "qa", "first pass");
    w.tracker.comment(&bead, "qa", "redo: the parser drops the last line");
    w.finish_attempt(&run, 2, Verdict::Fail, "redo").await;
    tokio::time::sleep(Duration::from_secs(12)).await;
    let third = last_prompt(&w, &run, 3);
    assert!(
        third.contains(
            "[1]{author,created_at,text}:\n  qa,\"2026-11-13T00:00:00Z\",\"redo: the parser drops the last line\""
        ),
        "{third}"
    );
    assert!(!third.contains("first pass"), "{third}");
    assert!(third.contains("the new goal"), "{third}");
    assert!(
        !third.contains("the old goal"),
        "the parents keep their last too: {third}"
    );
}

#[tokio::test(start_paused = true)]
async fn an_answer_to_an_earlier_entry_of_the_step_is_dropped() {
    let w = comments_world("");
    let bead = w.tracker.add(&[], None);
    let earlier = w.tracker.lineage(&bead).await.unwrap();
    let run = w.to_review(&bead).await;
    w.tracker.comment(&bead, "qa", "redo: the parser drops the last line");
    w.tracker.set_down(true);
    w.finish_attempt(&run, 2, Verdict::Fail, "redo").await;
    tokio::time::sleep(Duration::from_secs(12)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "preparing");

    // The read of the first entry of `code` answers late, while the run prepares its second.
    let payload = json!({ "run": run, "step": "code", "attempt": 1, "bead": bead });
    let msg = Msg::Lineage {
        payload,
        result: Ok(earlier),
    };
    w.core.send(None, msg).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "preparing");
    assert_eq!(last_prompt(&w, &run, 3), format!("Review Task {bead}."));

    w.driver.script(slow_turn());
    w.tracker.set_down(false);
    let mut third = String::new();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        third = last_prompt(&w, &run, 3);
        if third.starts_with("Fix ") {
            break;
        }
    }
    assert!(third.contains("redo: the parser drops the last line"), "{third}");
}

#[tokio::test(start_paused = true)]
async fn a_run_waiting_for_the_comments_survives_a_restart_and_fails_once_the_tracker_never_answers() {
    let mut w = comments_world("");
    let bead = w.tracker.add(&[], None);
    let run = w.to_review(&bead).await;
    w.tracker.set_down(true);
    w.finish_attempt(&run, 2, Verdict::Fail, "redo").await;
    tokio::time::sleep(Duration::from_secs(12)).await;
    let failed = w.all("tracker.sync_failed");
    let read = failed
        .iter()
        .find(|d| d["op"] == "lineage")
        .unwrap_or_else(|| panic!("{failed:?}"));
    assert_eq!(read["bead"], json!(bead));
    assert!(read["next_retry_at"].is_string(), "{read}");
    assert_eq!(w.states(&run).await.last().unwrap(), "preparing");
    w.restart().await;
    w.tracker.comment(&bead, "qa", "after the restart");
    w.driver.script(slow_turn());
    w.tracker.set_down(false);
    let mut third = String::new();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        third = last_prompt(&w, &run, 3);
        if third.starts_with("Fix ") {
            break;
        }
    }
    assert!(third.contains("after the restart"), "{third}");

    // A run whose read never gets an answer fails when `start_timeout` runs out.
    let w = comments_world("");
    let other = w.tracker.add(&[], None);
    let run = w.to_review(&other).await;
    w.tracker.set_down(true);
    w.finish_attempt(&run, 2, Verdict::Fail, "redo").await;
    tokio::time::sleep(Duration::from_secs(12)).await;
    assert_eq!(w.states(&run).await.last().unwrap(), "preparing");
    tokio::time::sleep(Duration::from_secs(121)).await;
    let events = w.events(&run, Duration::ZERO).await;
    assert_eq!(last(&events, "run.failed")["reason"], "tracker_unavailable");
}
