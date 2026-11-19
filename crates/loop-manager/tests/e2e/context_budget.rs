//! The addon `addons/context-budget` of this repository: its params reach claude's settings and hooks, and its hook
//! measures a session's context from the transcript, tells the agent to wrap up past `soft` and stops it past `hard`,
//! on fake transcripts and hook inputs of claude (addons/context-budget/README.md); those of codex are in `codex`.

use crate::common;
use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path, process::Stdio};

use bed::{Bed, commit, note};

fn addon() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../addons/context-budget")
}

/// `loop-manager config explain --json` of `agent` of `project` in `config`, with `dir` for the home and state of
/// `loop-manager`: its text.
fn explain(dir: &Path, config: &Path, project: &str, agent: &str) -> String {
    let out = common::loop_manager()
        .args(["--json", "config", "explain", "--config"])
        .arg(config)
        .args(["--project", project, "--agent", agent])
        .env("HOME", dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
    let value: Value = serde_json::from_str(&text).unwrap();
    value["text"].as_str().unwrap().to_owned()
}

#[test]
fn the_params_reach_the_hooks_of_claude_and_its_compaction_window_stays_past_hard() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let files = [
        (
            "config.toml".to_owned(),
            format!(
                "projects = [\"shop\"]\n[[addon]]\npath = \"{}\"\n[settings]\naddons = [{{ name = \"context-budget\", \
                 params = {{ cut = \"pass\" }} }}]\n",
                addon().display()
            ),
        ),
        ("shop/loop-manager.toml".into(), "packs = [\"../kit\"]\n".into()),
        ("kit/pack.toml".into(), "name = \"kit\"\nversion = \"0.1.0\"\n".into()),
        (
            "kit/agents/claude/agent.toml".into(),
            "harness = \"claude\"\naddons = [{ name = \"context-budget\", params = { soft = 150000 } }]\n".into(),
        ),
    ];
    for (path, text) in &files {
        fs::create_dir_all(d.join(path).parent().unwrap()).unwrap();
        fs::write(d.join(path), text).unwrap();
    }
    let text = explain(d, &d.join("config.toml"), "shop", "kit.claude");
    assert!(
        text.contains("native.claude = {\"autoCompactWindow\":1000000}  # addon context-budget"),
        "{text}"
    );
    let data = d.join("state/loop-manager/addons/context-budget");
    let args = format!("hooks/budget' '{}' 150000 420000 30000 'pass'", data.display());
    assert!(text.contains(&args), "{text}");
    for event in [
        "SessionStart",
        "SubagentStart",
        "PreToolUse",
        "PostToolUse",
        "PreCompact",
    ] {
        assert!(text.contains(&format!("\"event\":\"{event}\"")), "{event}: {text}");
    }
}

/// Demo 4 attaches the addon to its claude agent alone, with a small budget (demos/4-context-budget/README.md).
#[test]
fn demo_4_gives_its_claude_agent_a_small_budget() {
    let tmp = tempfile::tempdir().unwrap();
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/4-context-budget/root/config.toml");
    let claude = explain(tmp.path(), &config, "library", "study.glossarist");
    for (key, value) in [("soft", 40000), ("hard", 150000), ("warn_step", 20000)] {
        let line = format!("addons.context-budget.params.{key} = {value}  # project [agents]");
        assert!(claude.contains(&line), "{claude}");
    }
    assert!(claude.contains("\"event\":\"PostToolUse\""), "{claude}");
    let codex = explain(tmp.path(), &config, "library", "study.indexer");
    assert!(
        !codex.contains("addons.context-budget") && !codex.contains("hooks/budget"),
        "{codex}"
    );
}

#[test]
fn past_soft_the_agent_is_told_to_wrap_up_again_every_warn_step_and_may_call_no_subagent() {
    let bed = Bed::new("fail");
    let start = bed.hook(json!({ "hook_event_name": "SessionStart", "source": "startup" }));
    assert_eq!(start["hookSpecificOutput"]["hookEventName"], "SessionStart");
    assert!(note(&start).contains("Your context budget is 1k tokens"), "{start}");
    let compact = bed.hook(json!({ "hook_event_name": "PreCompact", "trigger": "auto" }));
    assert_eq!(compact["decision"], "block");

    bed.transcript("s1.jsonl", 900);
    assert_eq!(bed.tool("PostToolUse", "Bash"), Value::Null);
    assert_eq!(bed.tool("PreToolUse", "Agent"), Value::Null);

    // The synthetic answer at the end does not count: the context is that of the model's last answer.
    bed.transcript("s1.jsonl", 1200);
    let first = bed.tool("PostToolUse", "Bash");
    assert_eq!(first["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let text = note(&first);
    assert!(
        text.starts_with("Your context is 1k tokens, past your budget of 1k."),
        "{text}"
    );
    for part in [
        "call no subagents",
        "br comments add \"$LOOP_MANAGER_BEAD\"",
        "loop-manager finish",
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    // The call is denied and the turn goes on: `continue: false` would end it without an outcome.
    let denied = bed.tool("PreToolUse", "Agent");
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(denied.get("continue"), None, "{denied}");
    assert_eq!(bed.tool("PreToolUse", "Bash"), Value::Null);

    // The next note comes `warn_step` tokens later.
    bed.transcript("s1.jsonl", 1600);
    assert_eq!(bed.tool("PostToolUse", "Bash"), Value::Null);
    bed.transcript("s1.jsonl", 1700);
    assert!(note(&bed.tool("PostToolUse", "Bash")).starts_with("Your context is 1k tokens"));
    bed.transcript("s1.jsonl", 6900);
    assert!(note(&bed.tool("PostToolUse", "Read")).starts_with("Your context is 6k tokens"));
    assert_eq!(bed.finished(), "");
    bed.kept_hard_quiet();
}

#[test]
fn past_hard_the_session_leaves_its_state_on_the_bead_declares_the_cut_and_stops() {
    // The new usage of a session is seen first by the next call's `PreToolUse`, else by `PostToolUse`.
    for (cut, event) in [("fail", "PreToolUse"), ("pass", "PostToolUse")] {
        let bed = Bed::new(cut);
        bed.hook(json!({ "hook_event_name": "SessionStart", "source": "startup" }));
        commit(&bed.path().join("work"), "parser half");
        bed.transcript("s1.jsonl", 7100);
        let stop = bed.tool(event, "Bash");
        let reason = "the context budget of this session is spent; it is stopped";
        assert_eq!(stop["continue"], false, "{event}: {stop}");
        assert_eq!(stop["stopReason"], reason, "{event}: {stop}");
        if event == "PreToolUse" {
            assert_eq!(stop["hookSpecificOutput"]["permissionDecision"], "deny");
        }
        let summary = "context budget: the session was stopped; its state is on the bead";
        assert_eq!(bed.finished(), format!("finish {cut} --summary {summary}\n"));
        let comments = bed.comments();
        assert_eq!(comments.len(), 1, "{comments:?}");
        for part in [
            "# context-budget: the session was stopped",
            "## git status --short",
            "..HEAD",
            "parser half",
            "?? notes.md",
            "Bash: {\"command\":\"cargo test -p parser\"}",
            "Edit: {\"file_path\":\"src/parser.rs\"}",
            "I read the parser first.",
        ] {
            assert!(comments[0].contains(part), "{part}: {}", comments[0]);
        }
        // The log is that of the run: from the commit its session started on.
        assert!(!comments[0].contains(" start\n"), "{}", comments[0]);
        // Every tool is denied from now on, once the context is small again too; the outcome is declared once.
        bed.transcript("s1.jsonl", 100);
        let denied = bed.tool("PreToolUse", "Read");
        assert_eq!(denied["continue"], false);
        assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(denied["hookSpecificOutput"]["permissionDecisionReason"], reason);
        bed.transcript("s1.jsonl", 7200);
        bed.tool("PreToolUse", "Bash");
        assert_eq!(bed.finished().lines().count(), 1);
        assert_eq!(bed.comments().len(), 1);
        bed.kept_hard_quiet();
    }
}

#[test]
fn a_subagent_is_measured_on_its_own_transcript_and_stops_alone_without_an_outcome() {
    let bed = Bed::new("fail");
    let sub = |event: &str, tool: &str| {
        bed.hook(json!({ "hook_event_name": event, "tool_name": tool, "agent_id": "a1", "agent_type": "helper" }))
    };
    let start = sub("SubagentStart", "");
    assert!(note(&start).contains("return your summary"), "{start}");
    bed.transcript("s1.jsonl", 6900);
    bed.transcript("s1/subagents/agent-a1.jsonl", 1200);
    let text = note(&sub("PostToolUse", "Bash")).to_owned();
    assert!(text.starts_with("Your context is 1k tokens"), "{text}");
    assert!(text.contains("return your summary to your caller"), "{text}");
    assert!(!text.contains("loop-manager finish"), "{text}");

    bed.transcript("s1/subagents/agent-a1.jsonl", 7300);
    assert_eq!(sub("PostToolUse", "Bash")["continue"], false);
    assert_eq!(
        sub("PreToolUse", "Bash")["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert_eq!(bed.finished(), "", "a subagent declares no outcome");
    let comments = bed.comments();
    assert!(
        comments[0].contains("The subagent a1 of the session s1"),
        "{comments:?}"
    );
    // The session itself goes on, under its own context.
    assert!(note(&bed.tool("PostToolUse", "Bash")).starts_with("Your context is 6k tokens"));
    assert_eq!(bed.tool("PreToolUse", "Bash"), Value::Null);
    bed.kept_hard_quiet();
}

#[test]
fn a_run_without_a_bead_is_told_to_put_its_summary_in_finish() {
    let bed = Bed::new("fail");
    bed.with_bead.set(false);
    bed.transcript("s1.jsonl", 1200);
    let text = note(&bed.tool("PostToolUse", "Bash")).to_owned();
    assert!(text.contains("your summary in `--summary`"), "{text}");
    assert!(!text.contains("br comments add"), "{text}");
}

#[test]
fn a_wrong_cut_costs_only_the_outcome() {
    let bed = Bed::new("Fail");
    bed.transcript("s1.jsonl", 1200);
    assert!(note(&bed.tool("PostToolUse", "Bash")).starts_with("Your context is 1k tokens"));
    bed.transcript("s1.jsonl", 7100);
    assert_eq!(bed.tool("PreToolUse", "Bash")["continue"], false);
    assert_eq!(bed.comments().len(), 1);
    assert_eq!(bed.finished(), "");
}

mod bed;
mod codex;
