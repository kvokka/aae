//! The plugin `plugins/context-budget` of this repository: its params reach the native compaction thresholds of
//! claude and codex, and its hook counts the compactions of a session, keeps their label on the bead and fails the
//! run past the budget, on the hook inputs of both harnesses (plugins/context-budget/README.md).

use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
};

fn plugin() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/context-budget")
}

/// `aae config explain --json` of the agent `kit.<agent>` of the project `shop`, its value without the layers.
fn explain(dir: &Path, agent: &str) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_aae"))
        .args(["--json", "config", "explain", "--config"])
        .arg(dir.join("config.toml"))
        .args(["--project", "shop", "--agent", &format!("kit.{agent}")])
        .env("HOME", dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_params_reach_the_compaction_threshold_of_each_harness_and_the_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let files = [
        (
            "config.toml".to_owned(),
            format!(
                "projects = [\"shop\"]\n[[plugin]]\npath = \"{}\"\n[settings]\nplugins = [{{ name = \"context-budget\", \
                 params = {{ max_compactions = 1 }} }}]\n",
                plugin().display()
            ),
        ),
        ("shop/aae.toml".into(), "packs = [\"../kit\"]\n".into()),
        ("kit/pack.toml".into(), "name = \"kit\"\nversion = \"0.1.0\"\n".into()),
        (
            "kit/agents/claude/agent.toml".into(),
            "harness = \"claude\"\nplugins = [{ name = \"context-budget\", params = { window = 150000 } }]\n".into(),
        ),
        ("kit/agents/codex/agent.toml".into(), "harness = \"codex\"\n".into()),
    ];
    for (path, text) in &files {
        fs::create_dir_all(d.join(path).parent().unwrap()).unwrap();
        fs::write(d.join(path), text).unwrap();
    }
    let (claude, codex) = (explain(d, "claude"), explain(d, "codex"));
    let (claude, codex) = (claude["text"].as_str().unwrap(), codex["text"].as_str().unwrap());
    // The integer itself, not a string: the harnesses take a number.
    assert!(
        claude.contains("native.claude = {\"autoCompactWindow\":150000}  # plugin context-budget"),
        "{claude}"
    );
    let limit = "native.codex = {\"model_auto_compact_token_limit\":200000}  # plugin context-budget";
    assert!(codex.contains(limit), "{codex}");
    let data = d.join("state/aae/plugins/context-budget");
    let count = format!("hooks/budget' compacted '{}' 1", data.display());
    for text in [claude, codex] {
        assert!(
            text.contains(&count) && text.contains("\"event\":\"PostCompact\""),
            "{text}"
        );
        assert!(text.contains("\"event\":\"PreToolUse\""), "{text}");
    }
}

/// The hook `budget` with `args` and the hook input `input` in `dir`, with the stand-in `aae` of `dir/bin`; its
/// stdout, the hook succeeded.
fn hook(dir: &Path, bead: Option<&str>, args: &[&str], input: &Value) -> String {
    let out = run_hook(dir, bead, args, input);
    assert!(out.status.success(), "budget {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn run_hook(dir: &Path, bead: Option<&str>, args: &[&str], input: &Value) -> std::process::Output {
    let mut command = Command::new(plugin().join("hooks/budget"));
    command
        .args(args)
        .current_dir(dir)
        .env("BEADS_DIR", dir.join(".beads"))
        .env("RUST_LOG", "error");
    let path = format!("{}:{}", dir.join("bin").display(), std::env::var("PATH").unwrap());
    command.env("PATH", path).env_remove("AAE_BEAD");
    if let Some(bead) = bead {
        command.env("AAE_BEAD", bead);
    }
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// The stand-in `aae` of `dir/bin`: it logs its arguments and answers `answer`, failing unless it is `accepted`.
fn finisher(dir: &Path, answer: &str) {
    fs::create_dir_all(dir.join("bin")).unwrap();
    let fake =
        format!("#!/bin/sh\necho \"$*\" >>\"$(dirname \"$0\")/finished\"\necho {answer}\n[ {answer} = accepted ]\n");
    fs::write(dir.join("bin/aae"), fake).unwrap();
    fs::set_permissions(dir.join("bin/aae"), fs::Permissions::from_mode(0o755)).unwrap();
}

fn labels(dir: &Path, bead: &str) -> Value {
    let out = Command::new("br")
        .args(["label", "list", bead, "--json"])
        .current_dir(dir)
        .env("BEADS_DIR", dir.join(".beads"))
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn past_the_budget_the_hook_fails_the_run_and_stops_the_session_on_the_input_of_either_harness() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let br = |args: &[&str]| {
        let out = Command::new("br")
            .args(args)
            .current_dir(d)
            .env("RUST_LOG", "error")
            .output()
            .unwrap();
        assert!(out.status.success(), "br {args:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    br(&["init", "-q"]);
    finisher(d, "accepted");
    let data = d.join("data");
    fs::create_dir_all(&data).unwrap();
    let data = data.to_str().unwrap();
    // The inputs of PostCompact as each harness gives them (docs/design/experiments.md, experiment 9).
    let claude = json!({ "session_id": "s-claude", "prompt_id": "p", "hook_event_name": "PostCompact",
        "trigger": "auto", "custom_instructions": null, "compact_summary": "so far" });
    let codex = json!({ "session_id": "s-codex", "turn_id": "t", "transcript_path": "/x", "cwd": "/x",
        "hook_event_name": "PostCompact", "model": "m", "trigger": "auto" });
    for input in [claude, codex] {
        let session = input["session_id"].as_str().unwrap();
        let created: Value = serde_json::from_str(&br(&["create", "task", "--json"])).unwrap();
        let bead = created["id"].as_str().unwrap();
        br(&["label", "add", bead, "-l", "compactions:7,keep"]);
        let tool = json!({ "session_id": session, "hook_event_name": "PreToolUse", "tool_name": "Bash" });
        let mut child = input.clone();
        child["agent_id"] = json!("a1");
        // A subagent's or a child thread's compaction does not count.
        assert_eq!(hook(d, Some(bead), &["compacted", data, "1"], &child), "");
        assert_eq!(hook(d, Some(bead), &["compacted", data, "1"], &input), "");
        assert_eq!(labels(d, bead), json!(["compactions:1", "keep"]), "{session}");
        assert_eq!(hook(d, Some(bead), &["tool", data], &tool), "");

        let stop: Value = serde_json::from_str(&hook(d, Some(bead), &["compacted", data, "1"], &input)).unwrap();
        let reason = "context budget: 2 compactions";
        assert_eq!(stop, json!({ "continue": false, "stopReason": reason }));
        assert_eq!(labels(d, bead), json!(["compactions:2", "keep"]), "{session}");
        let denied: Value = serde_json::from_str(&hook(d, Some(bead), &["tool", data], &tool)).unwrap();
        assert_eq!(denied["continue"], false);
        assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(denied["hookSpecificOutput"]["permissionDecisionReason"], reason);
    }
    let finished = fs::read_to_string(d.join("bin/finished")).unwrap();
    assert_eq!(
        finished,
        "finish fail --summary context budget: 2 compactions\n".repeat(2)
    );
    // Without a bead only the count: the third compaction of a session past a budget of three goes on.
    let input = json!({ "session_id": "s-none", "hook_event_name": "PostCompact" });
    for _ in 0..3 {
        assert_eq!(hook(d, None, &["compacted", data, "3"], &input), "");
    }
    assert_eq!(fs::read_to_string(d.join("data/s-none.count")).unwrap(), "3\n");
    // An outcome not accepted (a remark, a daemon out of reach) fails the hook and leaves the session going.
    finisher(d, "remark");
    let input = json!({ "session_id": "s-remark", "hook_event_name": "PostCompact" });
    assert!(!run_hook(d, None, &["compacted", data, "0"], &input).status.success());
    let tool = json!({ "session_id": "s-remark", "hook_event_name": "PreToolUse", "tool_name": "Bash" });
    assert_eq!(hook(d, None, &["tool", data], &tool), "");
}
