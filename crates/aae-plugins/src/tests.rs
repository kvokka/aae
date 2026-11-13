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
        // Postmortem 0001, spec §2.1 item 12: a wait on the account, longer with every failure in a row.
        (
            failed(
                "auth_required",
                none.clone(),
                &[("resume", "auth_required", 9); 3],
                json!([]),
            ),
            json!({"decision": "park", "then": "resume", "gate": {"key": "account:claude/a1",
                "until": "2026-11-13T12:02:00.000Z", "reason": "auth_required"}}),
        ),
        (
            failed("billing", none.clone(), &[("resume", "billing", 9)], json!([])),
            json!({"decision": "park", "then": "resume", "gate": {"key": "account:claude/a1",
                "until": "2026-11-13T12:00:30.000Z", "reason": "billing"}}),
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

/// Every threshold of `[builtin.default-policy]` moves its decision away from the default one.
#[test]
fn every_policy_threshold_comes_from_the_config() {
    let none = json!({});
    let until = |v: &Value| v["gate"]["until"].clone();
    let quota = |retry: Value| failed("quota_exhausted", retry, &[], json!([]));
    let reset = quota(json!({"retry_at": "2026-11-13T15:00:00.000Z"}));
    assert_eq!(
        until(&policy(&reset, json!({"quota_margin_s": 0}))),
        "2026-11-13T15:00:00.000Z"
    );
    let no_reset = quota(none.clone());
    assert_eq!(
        until(&policy(&no_reset, json!({"quota_wait_s": 60}))),
        "2026-11-13T12:01:00.000Z"
    );
    let overloaded = failed("overloaded", json!({"retried_by_harness": true}), &[], json!([]));
    assert_eq!(
        until(&policy(&overloaded, json!({"model_pause_s": 60}))),
        "2026-11-13T12:01:00.000Z"
    );

    let limited = failed(
        "rate_limited",
        none.clone(),
        &[("resume", "rate_limited", 1); 2],
        json!([]),
    );
    assert_eq!(policy(&limited, json!({"backoff_base_s": 1}))["after"], "2s");
    assert_eq!(policy(&limited, json!({"backoff_cap_s": 5}))["after"], "5s");
    assert_eq!(policy(&limited, json!({"backoff_park_from": 2}))["decision"], "park");

    let rapid = failed(
        "harness_crash",
        none.clone(),
        &[("new_session", "harness_crash", 5); 2],
        json!([]),
    );
    assert_eq!(policy(&rapid, json!({}))["decision"], "resume");
    let looped = policy(&rapid, json!({"crash_loop": 2, "crash_pause_s": 60}));
    assert_eq!(until(&looped), "2026-11-13T12:01:00.000Z");
    assert_eq!(
        policy(&rapid, json!({"crash_loop": 2, "rapid_s": 5}))["decision"],
        "resume"
    );
    let resumed = failed(
        "harness_crash",
        none.clone(),
        &[("resume", "harness_crash", 600)],
        json!([]),
    );
    assert_eq!(
        policy(&resumed, json!({"resume_failures": 1}))["decision"],
        "new_session"
    );

    // The streak of `auth_required` began an hour ago, when its first attempt started.
    let auth = failed(
        "auth_required",
        none.clone(),
        &[("resume", "auth_required", 9); 2],
        json!([]),
    );
    assert_eq!(until(&policy(&auth, json!({}))), "2026-11-13T12:01:00.000Z");
    let based = policy(&auth, json!({"auth_backoff_base_s": 1}));
    assert_eq!(until(&based), "2026-11-13T12:00:02.000Z");
    let capped = policy(&auth, json!({"auth_backoff_cap_s": 5}));
    assert_eq!(until(&capped), "2026-11-13T12:00:05.000Z");
    assert!(check("default-policy", &json!({"auth_escalate_s": 3600})).is_err());
    // However long it lasts, the wait grows to an hour and never goes to an operator.
    let long = failed(
        "auth_required",
        none.clone(),
        &[("resume", "auth_required", 9); 40],
        json!([]),
    );
    assert_eq!(until(&policy(&long, json!({}))), "2026-11-13T13:00:00.000Z");

    let unknown = failed("unknown", none, &[("resume", "unknown", 9); 2], json!([]));
    assert_eq!(policy(&unknown, json!({}))["decision"], "resume");
    assert_eq!(policy(&unknown, json!({"unknown_retries": 1}))["decision"], "fail");
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

    // A run continuing r1's session got r1's workdir: the worktree comes back there on r1's branch, chosen or not,
    // and goes again when that run ends.
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
