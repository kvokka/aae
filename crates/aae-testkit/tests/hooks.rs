//! Orchestrator hooks as commands (spec §11.3, docs/design/contracts.md 3): real processes, so real time.

mod common;

use aae_core::{
    Journal,
    driver::{Normalized, RequestKind},
    run::{ExecHooks, Msg, Outcome, Verdict},
};
use aae_testkit::Step;
use common::{World, hollow_turn, world_with};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// A world whose plugin `policy` has the hooks `(event, script)`, each run as `sh <script>` in the plugin directory.
fn policy(hooks: &[(&str, &str)]) -> World {
    let mut toml = "name = \"policy\"\nversion = \"0.1.0\"\n".to_owned();
    let mut files = vec![(
        "config.toml".to_owned(),
        "projects = [\"shop\"]\n[harness.scripted]\n[[plugin]]\npath = \"policy\"\n".to_owned(),
    )];
    for (n, (on, script)) in hooks.iter().enumerate() {
        toml += &format!("[[hook]]\non = \"{on}\"\ncommand = [\"sh\", \"{n}.sh\"]\ntimeout = \"1s\"\n");
        files.push((format!("policy/{n}.sh"), (*script).to_owned()));
    }
    files.push(("policy/plugin.toml".to_owned(), toml));
    let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    world_with(&files, Arc::new(ExecHooks))
}

impl World {
    /// The `n`-th event `kind` of `run` (`None`: of any run), waiting for it up to 10 s.
    async fn until(&self, run: Option<&str>, kind: &str, n: usize) -> Value {
        let started = Instant::now();
        loop {
            let all = Journal::new(&self.journal).read_from(1, 100_000).unwrap_or_default();
            let found = all
                .into_iter()
                .filter(|e| e["type"] == kind && run.is_none_or(|r| e["run"] == r))
                .nth(n);
            if let Some(event) = found {
                return event["data"].clone();
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no {kind} #{n}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn declare(&self, run: &str) -> Value {
        let started = Instant::now();
        let token = loop {
            if let Some(token) = self.driver.token(run, 1) {
                break token;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no token");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let outcome = Outcome {
            result: Verdict::Pass,
            summary: "done".into(),
            data: None,
        };
        let msg = Msg::Finish {
            run: run.into(),
            token,
            outcome,
        };
        self.core.send(None, msg).await.unwrap()["answer"].clone()
    }
}

/// A turn that ends a second after the prompt: time to declare an outcome.
fn slow_turn() -> Vec<Step> {
    let mut steps = hollow_turn();
    steps.insert(1, Step::Wait(Duration::from_secs(1)));
    steps
}

#[tokio::test]
async fn preparing_hooks_chain_their_changes() {
    let one = r#"echo '{"decision":"proceed","workdir":"/tmp","env":{"X":"1"},"prompt_append":"one"}'"#;
    let two = r#"read -r e
case "$e" in *'"prompt_append":"one"'*) ;; *) exit 1 ;; esac
case "$e" in *'"X"'*) ;; *) exit 1 ;; esac
case "$e" in *'"workdir":"/tmp"'*) ;; *) exit 1 ;; esac
echo '{"decision":"proceed","prompt_append":"two"}'"#;
    let w = policy(&[("run.preparing", one), ("run.preparing", two)]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    let failed = w.until(Some(&run), "run.failed", 0).await;
    assert_eq!(
        (&failed["class"], &failed["run"]["workdir"]),
        (&json!("hollow_done"), &json!("/tmp"))
    );
}

#[tokio::test]
async fn a_remark_brings_a_second_pass_in_the_same_attempt() {
    let check = r#"read -r e
case "$e" in *'"remarks":0'*) echo '{"decision":"remark","message":"again"}' ;; esac"#;
    let w = policy(&[("run.finishing", check)]);
    w.driver.script(slow_turn());
    w.driver.script(slow_turn());
    let run = w.create().await;
    assert_eq!(w.declare(&run).await, "accepted");
    w.until(Some(&run), "run.finishing", 0).await;
    let changes = async |n| {
        let mut running = 0;
        for i in 0.. {
            if w.until(Some(&run), "run.state_changed", i).await["to"] == "running" {
                running += 1;
                if running == n {
                    return;
                }
            }
        }
    };
    changes(2).await;
    assert_eq!(w.declare(&run).await, "accepted");
    let passed = w.until(Some(&run), "run.passed", 0).await;
    assert_eq!(passed["run"]["attempt"], 1);
}

#[tokio::test]
async fn exit_2_on_finishing_is_a_veto_with_the_stderr_reason() {
    let w = policy(&[("run.finishing", "echo 'tests are red' >&2; exit 2")]);
    w.driver.script(slow_turn());
    let run = w.create().await;
    assert_eq!(w.declare(&run).await, "accepted");
    let failed = w.until(Some(&run), "run.failed", 0).await;
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&json!("check"), &json!("tests are red"))
    );
}

#[tokio::test]
async fn a_failing_hook_is_reported_and_counts_as_no_decision() {
    let w = policy(&[("run.attempt_failed", "echo broken >&2; exit 7")]);
    w.driver.script(vec![Step::Exit {
        code: Some(1),
        signal: None,
    }]);
    let started = Instant::now();
    let run = w.create().await;
    let failed = w.until(Some(&run), "run.failed", 0).await;
    assert_eq!(failed["reason"], "no_decision");
    assert!(started.elapsed() < Duration::from_secs(5), "not at the deadline");
    let report = w.until(Some(&run), "plugin.hook_failed", 0).await;
    assert_eq!(
        report,
        json!({ "plugin": "policy", "hook_type": "run.attempt_failed", "event_seq": report["event_seq"],
            "error": "exit", "exit_code": 7, "stderr_tail": "broken" })
    );
}

#[tokio::test]
async fn a_veto_hook_that_times_out_blocks_the_start() {
    let w = policy(&[("run.preparing", "sleep 10")]);
    let run = w.create().await;
    let failed = w.until(Some(&run), "run.failed", 0).await;
    assert_eq!(
        (&failed["class"], &failed["reason"]),
        (&json!("check"), &json!("hook_failed:policy"))
    );
    assert_eq!(w.until(Some(&run), "plugin.hook_failed", 0).await["error"], "timeout");
}

#[tokio::test]
async fn a_hook_answers_a_permission_request() {
    let w = policy(&[("run.requires_action", r#"echo '{"decision":"allow"}'"#)]);
    let request = Normalized::PermissionRequested {
        request_id: "req-1".into(),
        kind: RequestKind::Permission,
        tool: "Bash".into(),
        input: json!({ "command": "ls" }),
        text: String::new(),
    };
    w.driver.script(vec![Step::Emit(request)]);
    w.driver.script(hollow_turn());
    let run = w.create().await;
    assert_eq!(w.until(Some(&run), "run.failed", 0).await["class"], "hollow_done");
    let mut states = Vec::new();
    for n in 0.. {
        let to = w.until(Some(&run), "run.state_changed", n).await["to"].clone();
        states.push(to.as_str().unwrap().to_owned());
        if to == "failed" {
            break;
        }
    }
    assert!(
        states.ends_with(&["requires_action".into(), "running".into(), "failed".into()]),
        "{states:?}"
    );
}
