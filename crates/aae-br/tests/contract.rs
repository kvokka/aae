//! Contract tests of the br adapter on the pinned br (spec §10.2): claim under concurrency, conditional release and
//! close, the ready set with epics and rig labels, the writes as changes, the watcher.

use aae_br::Br;
use aae_core::tracker::{Tracker, TrackerError};
use std::{path::Path, process::Command, sync::mpsc, time::Duration};

/// A fresh tracker in a temporary directory.
fn tracker() -> (tempfile::TempDir, Br) {
    let dir = tempfile::tempdir().unwrap();
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status();
    assert!(git.unwrap().success());
    br(dir.path(), &["init", "--prefix", "t", "-q"]);
    let beads = dir.path().join(".beads");
    (dir, Br::new("br".into(), beads))
}

/// Runs br by hand, as a human (`alice`) would; its trimmed stdout.
fn br(root: &Path, args: &[&str]) -> String {
    let out = Command::new("br")
        .args(args)
        .args(["--actor", "alice"])
        .env("BEADS_DIR", root.join(".beads"))
        .env("RUST_LOG", "error")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "br {args:?}: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn create(root: &Path, title: &str, extra: &[&str]) -> String {
    let args: Vec<&str> = ["create", title, "--silent"]
        .into_iter()
        .chain(extra.iter().copied())
        .collect();
    br(root, &args)
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_claims_have_one_winner_and_the_rest_see_it_as_the_holder() {
    let (dir, t) = tracker();
    let t = std::sync::Arc::new(t);
    let id = create(dir.path(), "One", &[]);
    let owners: Vec<String> = (0..12).map(|i| format!("aae:run_{i}")).collect();
    let claims: Vec<_> = owners
        .iter()
        .map(|o| {
            let (t, o, id) = (t.clone(), o.clone(), id.clone());
            tokio::spawn(async move { t.claim(&o, &id).await })
        })
        .collect();
    let mut results = Vec::new();
    for claim in claims {
        results.push(claim.await.unwrap());
    }
    let winners: Vec<&String> = owners
        .iter()
        .zip(&results)
        .filter(|(_, r)| r.is_ok())
        .map(|(o, _)| o)
        .collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    let winner = winners[0].clone();
    for r in &results {
        match r {
            Ok(bead) => assert_eq!(
                (bead.assignee.as_ref(), bead.status.as_str()),
                (Some(&winner), "in_progress")
            ),
            Err(e) => assert_eq!(e, &TrackerError::Claimed { holder: winner.clone() }),
        }
    }
    assert!(t.claim(&winner, &id).await.is_ok(), "the holder claims again");
    assert_eq!(t.claim("aae:run_x", "t-nope").await, Err(TrackerError::NotFound));
}

#[tokio::test(flavor = "multi_thread")]
async fn release_and_close_change_only_a_bead_their_owner_holds() {
    let (dir, t) = tracker();
    let id = create(dir.path(), "Two", &[]);
    t.claim("aae:run_1", &id).await.unwrap();
    let other = t.release("aae:run_2", &id).await;
    assert_eq!(
        other,
        Err(TrackerError::OwnerMismatch {
            holder: Some("aae:run_1".into())
        })
    );
    assert_eq!(t.close("aae:run_2", &id, "x").await, other);
    t.release("aae:run_1", &id).await.unwrap();
    let ready = t.ready().await.unwrap();
    assert_eq!(
        (ready.len(), ready[0].assignee.as_ref(), ready[0].status.as_str()),
        (1, None, "open")
    );
    assert_eq!(
        t.release("aae:run_1", &id).await,
        Err(TrackerError::OwnerMismatch { holder: None }),
        "a released bead is nobody's"
    );
    t.claim("aae:run_3", &id).await.unwrap();
    t.close("aae:run_3", &id, "passed: done").await.unwrap();
    assert!(t.ready().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ready_set_carries_the_epic_rig_labels_and_description() {
    let (dir, t) = tracker();
    let epic = create(dir.path(), "Epic", &["-t", "epic"]);
    let child = create(
        dir.path(),
        "Child",
        &["--parent", &epic, "-l", "rig:web", "-d", "Do it"],
    );
    let lone = create(dir.path(), "Lone", &[]);
    let ready = t.ready().await.unwrap();
    let child = ready.iter().find(|b| b.id == child).unwrap();
    assert_eq!(child.epic.as_ref(), Some(&epic));
    assert_eq!(
        (child.labels.as_slice(), child.description.as_str()),
        (&["rig:web".to_owned()][..], "Do it")
    );
    assert_eq!(ready.iter().find(|b| b.id == lone).unwrap().epic, None);
    assert!(
        ready.iter().all(|b| b.id != epic),
        "an epic with open children is not ready"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_command_is_one_change_with_its_actor_and_the_watcher_signals_it() {
    let (dir, t) = tracker();
    let (tx, rx) = mpsc::channel();
    let _watch = aae_br::watch(&dir.path().join(".beads"), move || {
        let _ = tx.send(());
    })
    .unwrap();
    let id = create(dir.path(), "Three", &["-l", "rig:web"]);
    assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok(), "a write is signalled");
    t.claim("aae:run_1:1", &id).await.unwrap();
    t.close("aae:run_1:1", &id, "passed: done").await.unwrap();
    let changes = t.changes(0).await.unwrap();
    let seen: Vec<(&str, &str, Vec<String>)> = changes
        .iter()
        .map(|c| (c.kind.as_str(), c.actor.as_str(), c.changed.clone()))
        .collect();
    assert_eq!(
        seen,
        [
            ("created", "alice", vec![]),
            (
                "updated",
                "aae:run_1:1",
                vec!["status".to_owned(), "assignee".to_owned()]
            ),
            ("closed", "aae:run_1:1", vec![]),
        ]
    );
    assert!(changes.windows(2).all(|w| w[0].seq < w[1].seq));
    assert_eq!(changes[2].bead.status, "closed");
    let last = changes[2].seq;
    assert!(t.changes(last).await.unwrap().is_empty(), "nothing after the cursor");
}
