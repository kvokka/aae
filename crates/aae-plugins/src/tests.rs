use super::{check, run};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

/// `run.attempt_failed` at 12:00:00 for a failure of `kind`, after `attempts` of `(kind, failure_kind, seconds)`.
fn failed(kind: &str, failure: Value, attempts: &[(&str, &str, i64)], gates: Value) -> Value {
    let attempts: Vec<Value> = attempts
        .iter()
        .enumerate()
        .map(|(n, (k, f, secs))| {
            json!({ "n": n + 1, "kind": k, "failure_kind": f, "started_at": "2026-11-13T11:00:00.000Z",
                "ended_at": format!("2026-11-13T11:{:02}:{:02}.000Z", secs / 60, secs % 60) })
        })
        .collect();
    let mut failure = failure;
    failure["kind"] = json!(kind);
    json!({ "type": "run.attempt_failed", "ts": "2026-11-13T12:00:00.000Z", "data": {
        "run": { "id": "r1" }, "failure": failure, "attempts": attempts, "gates": gates,
        "keys": ["orchestrator", "project:shop", "rig:shop/web", "agent:shop.dev", "account:claude/a1", "model:anthropic/haiku"],
    } })
}

fn policy(event: &Value, params: Value) -> Value {
    run("default-policy", event, &params).unwrap().unwrap()
}

#[test]
fn default_policy_follows_contracts_10() {
    let none = json!({});
    let crash = |n| vec![("resume", "harness_crash", 600); n];
    let cases = [
        (
            failed("harness_crash", none.clone(), &crash(1), json!([])),
            json!({"decision": "resume", "after": "0s"}),
        ),
        (
            failed("harness_crash", none.clone(), &crash(3), json!([])),
            json!({"decision": "new_session"}),
        ),
        (
            failed(
                "harness_crash",
                none.clone(),
                &[("new_session", "harness_crash", 5); 5],
                json!([]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "agent:shop.dev",
                "until": "2026-11-13T12:05:00.000Z", "reason": "harness_crash"}}),
        ),
        (
            failed(
                "quota_exhausted",
                json!({"retry_at": "2026-11-13T15:00:00.000Z"}),
                &[],
                json!([]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "account:claude/a1",
                "until": "2026-11-13T15:01:00.000Z", "reason": "quota_exhausted"}}),
        ),
        (
            failed(
                "rate_limited",
                none.clone(),
                &[("resume", "rate_limited", 1); 2],
                json!([]),
            ),
            json!({"decision": "resume", "after": "20s"}),
        ),
        (
            failed("overloaded", json!({"retried_by_harness": true}), &[], json!([])),
            json!({"decision": "park", "then": "resume", "gate": {"key": "model:anthropic/haiku",
                "until": "2026-11-13T12:10:00.000Z", "reason": "overloaded"}}),
        ),
        (
            failed("config_error", none.clone(), &[], json!([])),
            json!({"decision": "fail", "reason": "config_error", "gate": {"key": "agent:shop.dev", "until": null,
                "reason": "config_error"}}),
        ),
        (
            failed("stall", none.clone(), &[("resume", "stall", 9); 2], json!([])),
            json!({"decision": "new_session"}),
        ),
        (
            failed("unknown", none.clone(), &[("resume", "unknown", 9); 4], json!([])),
            json!({"decision": "fail", "reason": "unknown"}),
        ),
        // A gate that closed while the attempt ran answers its failure: park on it, no new closure.
        (
            failed(
                "rate_limited",
                none.clone(),
                &[("resume", "rate_limited", 9)],
                json!([{"key": "model:anthropic/haiku", "until": "2026-11-13T12:30:00.000Z", "reason": "x",
                    "closed_at": "2026-11-13T11:00:05.000Z"}]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "model:anthropic/haiku",
                "until": "2026-11-13T12:30:00.000Z", "reason": "x"}}),
        ),
    ];
    for (event, want) in cases {
        assert_eq!(policy(&event, json!({})), want, "{event}");
    }
    // Thresholds come from the config.
    let event = failed("unknown", none, &[("resume", "unknown", 9)], json!([]));
    assert_eq!(policy(&event, json!({"unknown_after_s": 1}))["after"], "1s");
    assert!(check("default-policy", &json!({"no_such": 1})).is_err());
}

#[test]
fn default_policy_reminds_once() {
    let turn = |remarks| json!({ "type": "run.turn_ended_without_outcome", "data": { "remarks": remarks } });
    assert_eq!(policy(&turn(0), json!({}))["decision"], "continue");
    assert_eq!(policy(&turn(1), json!({}))["decision"], "fail");
    let other = json!({ "type": "run.finishing", "data": {} });
    assert_eq!(run("default-policy", &other, &json!({})), Ok(None));
}

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
    let event = |kind: &str, id: &str, workdir: &Path, agent: &str| {
        let run = json!({ "id": id, "agent": agent, "formula": "shop.fix", "workdir": workdir });
        json!({ "type": kind, "data": { "run": run } })
    };
    let params = json!({ "agents": ["dev"] });
    assert_eq!(
        run("worktree", &event("run.preparing", "r0", &repo, "shop.other"), &params),
        Ok(None)
    );

    // Created on the run's branch, and the same again when the hook runs twice.
    let decision = run("worktree", &event("run.preparing", "r1", &repo, "shop.dev"), &params)
        .unwrap()
        .unwrap();
    assert_eq!(decision["decision"], "proceed");
    let tree = Path::new(decision["workdir"].as_str().unwrap()).to_owned();
    assert_eq!(git(&tree, &["branch", "--show-current"]), "aae/r1");
    let again = run("worktree", &event("run.preparing", "r1", &repo, "shop.dev"), &params);
    assert_eq!(again, Ok(Some(decision)));

    // Uncommitted work keeps the worktree.
    std::fs::write(tree.join("work.txt"), "x").unwrap();
    assert!(run("worktree", &event("run.failed", "r1", &tree, "shop.dev"), &params).is_err());
    assert!(tree.exists());

    // Committed work: the worktree goes, the unmerged branch with the commit stays.
    git(&tree, &["add", "work.txt"]);
    git(&tree, &["commit", "-q", "-m", "work"]);
    assert_eq!(
        run("worktree", &event("run.passed", "r1", &tree, "shop.dev"), &params),
        Ok(None)
    );
    assert!(!tree.exists());
    assert_eq!(git(&repo, &["log", "-1", "--format=%s", "aae/r1"]), "work");

    // Prepared again with the branch left: the branch is checked out as it is, its commit kept.
    let decision = run("worktree", &event("run.preparing", "r1", &repo, "shop.dev"), &params)
        .unwrap()
        .unwrap();
    let tree = Path::new(decision["workdir"].as_str().unwrap()).to_owned();
    assert_eq!(git(&tree, &["log", "-1", "--format=%s"]), "work");
    std::fs::remove_dir_all(&tree).unwrap();

    // A detached HEAD whose commit no ref reaches is left in place.
    let decision = run("worktree", &event("run.preparing", "r2", &repo, "shop.dev"), &params)
        .unwrap()
        .unwrap();
    let tree = Path::new(decision["workdir"].as_str().unwrap()).to_owned();
    git(&tree, &["checkout", "-q", "--detach"]);
    git(&tree, &["commit", "-q", "--allow-empty", "-m", "lost"]);
    git(&repo, &["branch", "-D", "aae/r2"]);
    assert!(run("worktree", &event("run.passed", "r2", &tree, "shop.dev"), &params).is_err());
    assert!(tree.exists());
}
