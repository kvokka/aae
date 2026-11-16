//! Contract tests of the br adapter on the pinned br (spec §10.2): claim under concurrency, conditional release and
//! close, the ready set with epics and labels, the prefix of a rig's beads, the writes as changes, the watcher.

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
async fn the_ready_set_carries_the_epic_labels_and_description() {
    let (dir, t) = tracker();
    let epic = create(dir.path(), "Epic", &["-t", "epic"]);
    let child = create(
        dir.path(),
        "Child",
        &["--parent", &epic, "-l", "agent:coder", "-d", "Do it"],
    );
    let lone = create(dir.path(), "Lone", &[]);
    let ready = t.ready().await.unwrap();
    let child = ready.iter().find(|b| b.id == child).unwrap();
    assert_eq!(child.epic.as_ref(), Some(&epic));
    assert_eq!(
        (child.labels.as_slice(), child.description.as_str()),
        (&["agent:coder".to_owned()][..], "Do it")
    );
    assert_eq!(ready.iter().find(|b| b.id == lone).unwrap().epic, None);
    assert!(
        ready.iter().all(|b| b.id != epic),
        "an epic with open children is not ready"
    );
}

/// A rig's bead filed under a root with `BD_ISSUE_PREFIX` keeps the rig's prefix through `--deps parent-child`, and
/// `--parent` loses it (spec §10.2): the planner's prompt files rig beads the first way.
#[tokio::test(flavor = "multi_thread")]
async fn a_bead_filed_with_a_rig_prefix_under_a_root_keeps_it_and_parent_loses_it() {
    let (dir, t) = tracker();
    let root = create(dir.path(), "Root", &["-t", "epic"]);
    let on_rig = |link: &str| {
        let out = Command::new("br")
            .args(["create", "For the rig", "--silent", link, "--actor", "alice"])
            .env("BEADS_DIR", dir.path().join(".beads"))
            .env("BD_ISSUE_PREFIX", "api")
            .env("RUST_LOG", "error")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let kept = on_rig(&format!("--deps=parent-child:{root}"));
    assert!(kept.starts_with("api-"), "{kept}");
    let ready = t.ready().await.unwrap();
    assert_eq!(ready.iter().find(|b| b.id == kept).unwrap().epic.as_ref(), Some(&root));
    assert_eq!(t.under(&root).await.unwrap(), [kept]);
    assert_eq!(on_rig(&format!("--parent={root}")), format!("{root}.1"));
    assert!(root.starts_with("t-"), "a root bead has the database's prefix: {root}");
}

#[tokio::test(flavor = "multi_thread")]
async fn under_gives_the_ready_beads_at_any_depth_and_an_epic_once_its_children_close() {
    let (dir, t) = tracker();
    let root = create(dir.path(), "Root", &["-t", "epic"]);
    let epic = create(dir.path(), "Epic", &["-t", "epic", "--parent", &root]);
    let kids = [1, 2].map(|_| create(dir.path(), "Grandchild", &["--parent", &epic]));
    let lone = create(dir.path(), "Lone", &[]);
    assert_eq!(t.under(&root).await.unwrap(), kids);
    assert!(t.ready().await.unwrap().iter().any(|b| b.id == lone));
    br(dir.path(), &["close", &kids[0], &kids[1]]);
    assert_eq!(t.under(&root).await.unwrap(), [epic]);
    assert_eq!(t.under("t-none").await, Err(TrackerError::NotFound));
}

#[tokio::test(flavor = "multi_thread")]
async fn each_command_is_one_change_with_its_actor_and_the_watcher_signals_it() {
    let (dir, t) = tracker();
    let (tx, rx) = mpsc::channel();
    let _watch = aae_br::watch(&dir.path().join(".beads"), move || {
        let _ = tx.send(());
    })
    .unwrap();
    let id = create(dir.path(), "Three", &["-l", "agent:coder"]);
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

#[tokio::test(flavor = "multi_thread")]
async fn a_key_creates_one_epic_whose_children_run_and_which_is_released_while_they_are_open() {
    let (dir, t) = tracker();
    let root = t
        .create("aae:run_1", "aae-k1", "Ship it", "Ship it\nwhole text")
        .await
        .unwrap();
    let again = t.create("aae:run_1", "aae-k1", "Other", "other").await.unwrap();
    assert_eq!(again.id, root.id, "a repeated key gives the first bead");
    assert_eq!(
        (
            root.title.as_str(),
            root.description.as_str(),
            root.labels.len(),
            root.epic.as_ref()
        ),
        ("Ship it", "Ship it\nwhole text", 0, None)
    );
    let listed = br(dir.path(), &["list", "-a", "--limit", "0", "--json"]);
    let listed: serde_json::Value = serde_json::from_str(&listed).unwrap();
    let issues = listed["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 1, "one bead per key");
    assert_eq!(issues[0]["issue_type"], "epic");
    assert_ne!(t.create("aae:run_2", "aae-k2", "Two", "").await.unwrap().id, root.id);

    let child = create(dir.path(), "Child", &["--parent", &root.id]);
    t.claim("aae:run_1", &root.id).await.unwrap();
    let ready = t.ready().await.unwrap();
    assert!(
        ready.iter().any(|b| b.id == child),
        "the children of a claimed root are ready"
    );
    t.close("aae:run_1", &root.id, "passed: done").await.unwrap();
    let shown = t.ready().await.unwrap();
    assert!(
        shown.iter().all(|b| b.id != root.id),
        "an epic with open children is not ready"
    );
    let shown: serde_json::Value = serde_json::from_str(&br(dir.path(), &["show", &root.id, "--json"])).unwrap();
    assert_eq!(
        (shown[0]["status"].as_str(), shown[0]["assignee"].as_str()),
        (Some("open"), None),
        "a parent with an open child is released, not closed"
    );
}

/// A task that names a bead gives that bead and files none; a text files a root that keeps the database's prefix
/// even where a rig's `BD_ISSUE_PREFIX` is set, as in a daemon started from a rig's shell (spec §10.2, §10.3).
#[tokio::test(flavor = "multi_thread")]
async fn a_task_naming_a_bead_gives_it_and_a_text_files_a_root_of_the_database_prefix() {
    const NAME: &str = "a_task_naming_a_bead_gives_it_and_a_text_files_a_root_of_the_database_prefix";
    if std::env::var_os("BD_ISSUE_PREFIX").is_none() {
        // The variable is set for a run of this test alone: the test binary runs it again with it.
        let me = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--quiet"])
            .env("BD_ISSUE_PREFIX", "api")
            .status();
        assert!(me.unwrap().success());
        return;
    }
    let (dir, t) = tracker();
    let named = create(dir.path(), "For the rig", &[]);
    assert!(named.starts_with("api-"), "{named}");
    let taken = t.create("aae:run_1", "aae-k1", &named, &named).await.unwrap();
    assert_eq!(taken.id, named);
    let root = t.create("aae:run_2", "aae-k2", "Ship it", "Ship it").await.unwrap();
    assert!(root.id.starts_with("t-"), "{}", root.id);
    let listed = br(dir.path(), &["list", "-a", "--limit", "0", "--json"]);
    let listed: serde_json::Value = serde_json::from_str(&listed).unwrap();
    assert_eq!(listed["issues"].as_array().unwrap().len(), 2, "one bead filed");
}
