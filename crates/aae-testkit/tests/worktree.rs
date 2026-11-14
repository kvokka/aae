//! The bundled `worktree` plugin on the run machine, with real git (spec §11.5).

mod common;

use aae_core::run::{Create, ExecHooks, Msg};
use aae_testkit::ScriptedDriver;
use common::{World, hollow_turn, world_on};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

fn slow_turn() -> Vec<aae_testkit::Step> {
    let mut steps = hollow_turn();
    steps.insert(1, aae_testkit::Step::Wait(Duration::from_secs(1)));
    steps
}

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
    "name = \"build\"\nworktree = true\n[step]\nkind = \"prompt\"\nagent = \"dev\"\nprompt = \"build.md\"\n",
);

/// The world with `files` over the common ones, its rig a git repository.
fn world(files: &[(&str, &str)]) -> World {
    let hooks = ExecHooks {
        socket: None,
        bundled: Some(aae_plugins::run),
    };
    let w = world_on(files, Arc::new(hooks), ScriptedDriver::new(2));
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
    w.driver.script(slow_turn());
    w.driver.script(slow_turn());
    let (one, two) = (w.create().await, w.create().await);
    let (failed_one, failed_two) = (
        w.until(Some(&one), "run.failed", 0).await,
        w.until(Some(&two), "run.failed", 0).await,
    );
    // The second run started while the first still ran: the rig did not hold it.
    assert!(w.seq_of(&two, "run.started", 0).await < w.seq_of(&one, "run.failed", 0).await);
    for (run, failed) in [(&one, &failed_one), (&two, &failed_two)] {
        let tree = PathBuf::from(failed["run"]["workdir"].as_str().unwrap());
        assert!(tree.ends_with(format!("aae/worktrees/{run}")), "{tree:?}");
        gone(&tree).await;
        // No commit on the branch: it is merged, so it goes with the worktree.
        assert_eq!(git(&rig, &["branch", "--list", &format!("aae/{run}")]), "");
    }
}

#[tokio::test]
async fn a_run_continuing_a_session_works_in_the_worktree_of_that_session() {
    let w = world(&[BUILD]);
    w.driver.script(hollow_turn());
    let first = w.create().await;
    let failed = w.until(Some(&first), "run.failed", 0).await;
    let tree = PathBuf::from(failed["run"]["workdir"].as_str().unwrap());
    assert!(tree.ends_with(format!("aae/worktrees/{first}")), "{tree:?}");
    gone(&tree).await;

    w.driver.script(hollow_turn());
    let create = Create {
        project: "shop".into(),
        rig: "web".into(),
        formula: "build".into(),
        inputs: serde_json::Map::new(),
        correlation: None,
        depth: 0,
        parent_run: None,
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
    assert!(!w.dir.path().join("shop/web/.git/aae/worktrees").exists());
}
