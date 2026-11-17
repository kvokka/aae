//! The bundled `worktree` addon on the run machine, with real git (docs/design/config.md).

use crate::common;

use common::{World, hollow_turn, world_on};
use loop_manager_core::run::{Create, ExecHooks, Msg};
use loop_manager_testkit::{ScriptedDriver, Step};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// `git -C <dir> <args>` without the user's git config; its trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The formula `build` with `worktree = true`.
const BUILD: (&str, &str) = (
    "shop/formulas/build.toml",
    "name = \"build\"\nworktree = true\n[[step]]\nkind = \"ai\"\nagent = \"dev\"\n",
);

/// The world with `files` over the common ones and the bundled addons, its rig a plain directory.
fn plain(files: &[(&str, &str)]) -> World {
    let hooks = ExecHooks {
        socket: None,
        bundled: Some(loop_manager_addons::run),
    };
    world_on(files, Arc::new(hooks), ScriptedDriver::new(2))
}

/// The world with `files` over the common ones, its rig a git repository.
fn world(files: &[(&str, &str)]) -> World {
    let w = plain(files);
    let rig = w.dir.path().join("shop/web");
    git(&rig, &["init", "-q", "-b", "main"]);
    git(&rig, &["add", "-A"]);
    git(&rig, &["commit", "-q", "-m", "base"]);
    w
}

/// Waits for the worktree `tree` to go.
async fn gone(tree: &Path) {
    let started = Instant::now();
    while tree.exists() {
        assert!(started.elapsed() < Duration::from_secs(10), "{tree:?} stays");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn two_runs_of_one_rig_work_at_once_each_in_its_worktree() {
    let w = world(&[BUILD]);
    let rig = w.dir.path().join("shop/web");
    // Each turn holds until both runs have started, so the first still runs when the second starts.
    let held = [Arc::new(Notify::new()), Arc::new(Notify::new())];
    for turn in &held {
        let mut steps = hollow_turn();
        steps.insert(1, Step::Await(turn.clone()));
        w.driver.script(steps);
    }
    let (one, two) = (w.create().await, w.create().await);
    w.until(Some(&two), "run.started", 0).await;
    held.iter().for_each(|turn| turn.notify_one());
    let (failed_one, failed_two) = (
        w.until(Some(&one), "run.failed", 0).await,
        w.until(Some(&two), "run.failed", 0).await,
    );
    // The second run started while the first still ran: the rig did not hold it.
    assert!(w.seq_of(&two, "run.started", 0).await < w.seq_of(&one, "run.failed", 0).await);
    for (run, failed) in [(&one, &failed_one), (&two, &failed_two)] {
        let tree = PathBuf::from(failed["run"]["workdir"].as_str().unwrap());
        assert!(tree.ends_with(format!("loop-manager/worktrees/{run}")), "{tree:?}");
        gone(&tree).await;
        // No commit on the branch: it is merged, so it goes with the worktree.
        assert_eq!(git(&rig, &["branch", "--list", &format!("loop-manager/{run}")]), "");
    }
}

#[tokio::test]
async fn a_run_continuing_a_session_works_in_the_worktree_of_that_session() {
    let w = world(&[BUILD]);
    w.driver.script(hollow_turn());
    let first = w.create().await;
    let failed = w.until(Some(&first), "run.failed", 0).await;
    let tree = PathBuf::from(failed["run"]["workdir"].as_str().unwrap());
    assert!(tree.ends_with(format!("loop-manager/worktrees/{first}")), "{tree:?}");
    gone(&tree).await;

    w.driver.script(hollow_turn());
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "build".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: Some(first.clone()),
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let second = reply["run"].as_str().unwrap();
    let started = w.until(Some(second), "run.started", 0).await;
    assert_eq!(
        started["run"]["workdir"],
        tree.to_string_lossy().as_ref(),
        "the session's cwd again"
    );
    w.until(Some(second), "run.failed", 0).await;
    gone(&tree).await;
}

#[tokio::test]
async fn a_formula_without_the_key_runs_in_the_rig() {
    let w = world(&[]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    let failed = w.until(Some(&run), "run.failed", 0).await;
    assert_eq!(
        Path::new(failed["run"]["workdir"].as_str().unwrap()),
        w.dir.path().join("shop/web")
    );
    assert!(!w.dir.path().join("shop/web/.git/loop-manager/worktrees").exists());
}

#[tokio::test]
async fn a_worktree_in_a_directory_without_git_fails_that_run() {
    let check = (
        "shop/formulas/check.toml",
        "name = \"check\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n",
    );
    let w = plain(&[BUILD, check]);
    let rig = w.dir.path().join("shop/web");
    let run = w.create().await;
    let failed = w.until(Some(&run), "run.failed", 0).await;
    assert_eq!(failed["class"], "check");
    assert_eq!(
        failed["reason"],
        format!(
            "worktree: `{}` is not a git repository; the formula has `worktree = true`",
            rig.display()
        )
    );

    // A formula without the key runs in that same directory.
    let create = Create {
        project: "shop".into(),
        rig: Some("web".into()),
        formula: "check".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
        token: None,
        continue_from: None,
    };
    let reply = w.core.send(None, Msg::Create(create)).await.unwrap();
    let passed = w.until(reply["run"].as_str(), "run.passed", 0).await;
    assert_eq!(Path::new(passed["run"]["workdir"].as_str().unwrap()), rig);
}
