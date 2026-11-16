//! The bundled `worktree` plugin on real git.

use crate::run;
use serde_json::json;
use std::{path::Path, process::Command, time::Duration};

fn git(dir: &Path, args: &[&str]) -> String {
    // Away from the user's git config: its hooks would run on the commits here.
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-C")
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

#[test]
fn worktree_on_real_git() {
    let repo = tempfile::tempdir().unwrap();
    let repo = repo.path().canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for config in [["user.email", "t@example.com"], ["user.name", "t"]] {
        git(&repo, &["config", config[0], config[1]]);
    }
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let event = |kind: &str, id: &str, workdir: &Path, formula: &str| {
        let run = json!({ "id": id, "agent": "shop.dev", "formula": formula, "workdir": workdir });
        json!({ "type": kind, "data": { "run": run } })
    };
    let params = json!({ "formulas": ["shop.fix"] });
    assert_eq!(
        run("worktree", &event("run.preparing", "r0", &repo, "shop.other"), &params),
        Ok(None)
    );

    // Created on the run's branch, and the same again when the hook runs twice.
    let decision = run("worktree", &event("run.preparing", "r1", &repo, "shop.fix"), &params)
        .unwrap()
        .unwrap();
    assert_eq!(decision["decision"], "proceed");
    let tree = Path::new(decision["workdir"].as_str().unwrap()).to_owned();
    assert_eq!(git(&tree, &["branch", "--show-current"]), "aae/r1");
    let again = run("worktree", &event("run.preparing", "r1", &repo, "shop.fix"), &params);
    assert_eq!(again, Ok(Some(decision)));

    // Uncommitted work keeps the worktree.
    std::fs::write(tree.join("work.txt"), "x").unwrap();
    assert!(run("worktree", &event("run.failed", "r1", &tree, "shop.fix"), &params).is_err());
    assert!(tree.exists());

    // Committed work: the worktree goes, the unmerged branch with the commit stays.
    git(&tree, &["add", "work.txt"]);
    git(&tree, &["commit", "-q", "-m", "work"]);
    assert_eq!(
        run("worktree", &event("run.passed", "r1", &tree, "shop.fix"), &params),
        Ok(None)
    );
    assert!(!tree.exists());
    assert_eq!(git(&repo, &["log", "-1", "--format=%s", "aae/r1"]), "work");

    // A run continuing r1's session got r1's workdir: the worktree comes back there on r1's branch, whatever its
    // formula, and goes again when that run ends.
    let decision = run("worktree", &event("run.preparing", "r3", &tree, "shop.other"), &params)
        .unwrap()
        .unwrap();
    assert_eq!(decision["workdir"], json!(tree));
    assert_eq!(git(&tree, &["log", "-1", "--format=%s"]), "work");
    assert_eq!(
        run("worktree", &event("run.passed", "r3", &tree, "shop.other"), &params),
        Ok(None)
    );
    assert!(!tree.exists());

    // Prepared again with the branch left: the branch is checked out as it is, its commit kept.
    let decision = run("worktree", &event("run.preparing", "r1", &repo, "shop.fix"), &params)
        .unwrap()
        .unwrap();
    let tree = Path::new(decision["workdir"].as_str().unwrap()).to_owned();
    assert_eq!(git(&tree, &["log", "-1", "--format=%s"]), "work");
    std::fs::remove_dir_all(&tree).unwrap();

    // A detached HEAD whose commit no ref reaches is left in place.
    let decision = run("worktree", &event("run.preparing", "r2", &repo, "shop.fix"), &params)
        .unwrap()
        .unwrap();
    let tree = Path::new(decision["workdir"].as_str().unwrap()).to_owned();
    git(&tree, &["checkout", "-q", "--detach"]);
    git(&tree, &["commit", "-q", "--allow-empty", "-m", "lost"]);
    git(&repo, &["branch", "-D", "aae/r2"]);
    assert!(run("worktree", &event("run.passed", "r2", &tree, "shop.fix"), &params).is_err());
    assert!(tree.exists());
}

#[test]
fn a_git_past_the_hook_timeout_is_killed() {
    let repo = tempfile::tempdir().unwrap();
    let repo = repo.path().canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(
        &repo,
        &[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "b",
        ],
    );
    // A hook path of the user's own config would skip the repository's hooks.
    let hooks = repo.join(".git/hooks");
    git(&repo, &["config", "core.hooksPath", &hooks.to_string_lossy()]);
    let hook = hooks.join("post-checkout");
    std::fs::write(&hook, "#!/bin/sh\nexec sleep 20\n").unwrap();
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let run_ = json!({ "id": "r1", "agent": "shop.dev", "formula": "shop.fix", "workdir": repo });
    let event = json!({ "type": "run.preparing", "data": { "run": run_ } });
    let started = std::time::Instant::now();
    let said = run("worktree", &event, &json!({ "formulas": ["shop.fix"] }));
    assert!(started.elapsed() < Duration::from_secs(10), "{said:?}");
    assert!(said.unwrap_err().contains("killed past the hook timeout"));
}

#[test]
fn runs_of_one_repository_prepared_at_once_each_get_a_worktree() {
    let repo = tempfile::tempdir().unwrap();
    let repo = repo.path().canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    // The first worktree added then writes the repository's config.
    git(&repo, &["config", "worktree.useRelativePaths", "true"]);
    git(
        &repo,
        &[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "b",
        ],
    );
    let params = json!({ "formulas": ["shop.fix"] });
    let said: Vec<_> = std::thread::scope(|s| {
        let runs: Vec<_> = (0..8)
            .map(|i| {
                let run_ =
                    json!({ "id": format!("r{i}"), "agent": "shop.dev", "formula": "shop.fix", "workdir": repo });
                let event = json!({ "type": "run.preparing", "data": { "run": run_ } });
                let params = &params;
                s.spawn(move || run("worktree", &event, params))
            })
            .collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    for (i, said) in said.iter().enumerate() {
        let tree = repo.join(format!(".git/aae/worktrees/r{i}"));
        assert_eq!(said, &Ok(Some(json!({ "decision": "proceed", "workdir": tree }))));
    }
}
