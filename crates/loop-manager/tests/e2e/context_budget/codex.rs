//! The addon on codex: its `hard` reaches codex's auto-compaction limit, and its hook measures a thread's context
//! from codex's rollout, answers only what codex's strict hook outputs take, and stops a session at the compaction
//! codex starts at `hard` (addons/context-budget/README.md).

use super::*;

/// Writes the codex rollout `name` whose last count of the model holds `tokens` of context, after two tool calls; it
/// holds a larger total of the whole thread, a tool output naming `token_count` and a count without usage.
fn rollout(bed: &Bed, name: &str, tokens: u64) -> String {
    let item = |payload: Value| json!({ "type": "response_item", "payload": payload });
    let count = |total: u64, last: Value| {
        json!({ "type": "event_msg", "payload": { "type": "token_count", "info": {
            "total_token_usage": { "total_tokens": total }, "last_token_usage": last,
            "model_context_window": 258400 } } })
    };
    let lines = [
        json!({ "timestamp": "2026-11-19T04:53:02.707Z", "type": "session_meta",
            "payload": { "id": "t1", "cli_version": "0.170.0", "source": "vscode" } }),
        item(json!({ "type": "message", "role": "user",
            "content": [{ "type": "input_text", "text": "the usage of the parser" }] })),
        item(json!({ "type": "message", "role": "assistant",
            "content": [{ "type": "output_text", "text": "I read the parser first." }] })),
        item(json!({ "type": "function_call", "name": "exec_command",
            "arguments": "{\"cmd\":\"cargo test -p parser\"}", "call_id": "c1" })),
        item(
            json!({ "type": "function_call_output", "call_id": "c1", "output": "a \"token_count\" in a tool output" }),
        ),
        count(
            100,
            json!({ "input_tokens": 80, "cached_input_tokens": 40, "output_tokens": 20, "total_tokens": 100 }),
        ),
        item(json!({ "type": "custom_tool_call", "name": "apply_patch",
            "input": "*** Begin Patch\n*** Update File: src/parser.rs\n", "call_id": "c2" })),
        count(99999, json!({ "total_tokens": tokens })),
        json!({ "type": "event_msg", "payload": { "type": "token_count", "info": null,
            "rate_limits": { "limit_id": "codex" } } }),
    ];
    let path = bed.path().join(name);
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::write(&path, text).unwrap();
    path.display().to_string()
}

/// The hook on the codex input `input` of the thread whose rollout is `name`: codex names the turn in every input but
/// that of `SessionStart`.
fn codex(bed: &Bed, name: &str, mut input: Value) -> Value {
    if input["hook_event_name"] != "SessionStart" {
        input["turn_id"] = json!("u1");
    }
    input["model"] = json!("gpt-5.5");
    input["transcript_path"] = json!(bed.path().join(name));
    bed.hook(input)
}

fn event(event: &str, tool: &str) -> Value {
    json!({ "hook_event_name": event, "tool_name": tool })
}

const STOPPED: &str = "the context budget of this session is spent; it is stopped";

/// The only answer of `PreToolUse` codex takes to keep a call from running: a denial, with no key to end the turn.
fn denial(reason: &str) -> Value {
    json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny",
        "permissionDecisionReason": reason } })
}

#[test]
fn the_hard_limit_of_codex_is_its_auto_compaction_limit_and_the_params_reach_its_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let files = [
        (
            "config.toml".to_owned(),
            format!("projects = [\"shop\"]\n[[addon]]\npath = \"{}\"\n", addon().display()),
        ),
        ("shop/loop-manager.toml".into(), "packs = [\"../kit\"]\n".into()),
        ("kit/pack.toml".into(), "name = \"kit\"\nversion = \"0.1.0\"\n".into()),
        (
            "kit/agents/codex/agent.toml".into(),
            "harness = \"codex\"\naddons = [{ name = \"context-budget\", params = { hard = 240000 } }]\n".into(),
        ),
    ];
    for (path, text) in &files {
        fs::create_dir_all(d.join(path).parent().unwrap()).unwrap();
        fs::write(d.join(path), text).unwrap();
    }
    let text = explain(d, &d.join("config.toml"), "shop", "kit.codex");
    let limit = "native.codex = {\"model_auto_compact_token_limit\":240000}  # addon context-budget";
    assert!(text.contains(limit), "{text}");
    assert!(!text.contains("native.claude"), "{text}");
    let data = d.join("state/loop-manager/addons/context-budget");
    let args = format!("hooks/budget' '{}' 300000 240000 30000 'fail'", data.display());
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

#[test]
fn past_soft_a_codex_session_is_told_to_wrap_up_by_the_last_count_and_may_spawn_no_agent() {
    let bed = Bed::new("fail");
    let start = codex(
        &bed,
        "r1.jsonl",
        json!({ "hook_event_name": "SessionStart", "source": "startup" }),
    );
    assert!(note(&start).contains("Your context budget is 1k tokens"), "{start}");

    // Neither the total of the thread nor a later count without usage is the context.
    rollout(&bed, "r1.jsonl", 900);
    assert_eq!(codex(&bed, "r1.jsonl", event("PostToolUse", "Bash")), Value::Null);
    assert_eq!(codex(&bed, "r1.jsonl", event("PreToolUse", "spawn_agent")), Value::Null);

    rollout(&bed, "r1.jsonl", 1200);
    let text = note(&codex(&bed, "r1.jsonl", event("PostToolUse", "Bash"))).to_owned();
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
    let denied = codex(&bed, "r1.jsonl", event("PreToolUse", "spawn_agent"));
    let reason = "Your context is 1k tokens, past your budget of 1k: call no subagents, wrap up.";
    assert_eq!(denied, denial(reason));
    assert_eq!(codex(&bed, "r1.jsonl", event("PreToolUse", "Bash")), Value::Null);

    rollout(&bed, "r1.jsonl", 1600);
    assert_eq!(codex(&bed, "r1.jsonl", event("PostToolUse", "Bash")), Value::Null);
    rollout(&bed, "r1.jsonl", 1700);
    assert!(note(&codex(&bed, "r1.jsonl", event("PostToolUse", "Bash"))).starts_with("Your context is 1k tokens"));
    assert_eq!(bed.finished(), "");
    bed.kept_hard_quiet();
}

#[test]
fn the_compaction_of_codex_or_a_count_past_hard_leaves_the_state_on_the_bead_and_stops_the_session() {
    // codex compacts at `hard` by its own count, which the rollout may not show yet: `PreCompact` stops the session.
    for (cut, name, tokens) in [
        ("fail", "PreCompact", 6900),
        ("pass", "PostToolUse", 7100),
        ("fail", "PreToolUse", 7100),
    ] {
        let bed = Bed::new(cut);
        codex(
            &bed,
            "r1.jsonl",
            json!({ "hook_event_name": "SessionStart", "source": "startup" }),
        );
        commit(&bed.path().join("work"), "parser half");
        rollout(&bed, "r1.jsonl", tokens);
        let stop = codex(&bed, "r1.jsonl", event(name, "Bash"));
        // codex rejects an answer with a key its event does not take, and then runs the call or the compaction.
        if name == "PreToolUse" {
            assert_eq!(stop, denial(STOPPED));
        } else {
            assert_eq!(stop, json!({ "continue": false, "stopReason": STOPPED }), "{name}");
        }
        let summary = "context budget: the session was stopped; its state is on the bead";
        assert_eq!(bed.finished(), format!("finish {cut} --summary {summary}\n"), "{name}");
        let comments = bed.comments();
        assert_eq!(comments.len(), 1, "{comments:?}");
        for part in [
            "# context-budget: the session was stopped",
            "## git status --short",
            "parser half",
            "?? notes.md",
            "exec_command: {\"cmd\":\"cargo test -p parser\"}",
            "apply_patch: \"*** Begin Patch",
            "I read the parser first.",
        ] {
            assert!(comments[0].contains(part), "{part}: {}", comments[0]);
        }
        // Every tool is denied from now on, once the context is small again too; the outcome is declared once.
        rollout(&bed, "r1.jsonl", 100);
        assert_eq!(codex(&bed, "r1.jsonl", event("PreToolUse", "Read")), denial(STOPPED));
        let again = codex(
            &bed,
            "r1.jsonl",
            json!({ "hook_event_name": "PreCompact", "trigger": "auto" }),
        );
        assert_eq!(again, json!({ "continue": false, "stopReason": STOPPED }));
        assert_eq!(bed.finished().lines().count(), 1);
        assert_eq!(bed.comments().len(), 1);
        bed.kept_hard_quiet();
    }
}

#[test]
fn a_codex_subagent_is_measured_on_its_own_rollout_and_its_compaction_stops_it_alone() {
    let bed = Bed::new("fail");
    let sub = |name: &str, tool: &str| {
        let mut input = event(name, tool);
        input["agent_id"] = json!("t2");
        input["agent_type"] = json!("helper");
        codex(&bed, "t2.jsonl", input)
    };
    assert!(note(&sub("SubagentStart", "")).contains("return your summary"));
    rollout(&bed, "r1.jsonl", 6900);
    rollout(&bed, "t2.jsonl", 1200);
    let text = note(&sub("PostToolUse", "Bash")).to_owned();
    assert!(text.starts_with("Your context is 1k tokens"), "{text}");
    assert!(text.contains("return your summary to your caller"), "{text}");
    assert!(!text.contains("loop-manager finish"), "{text}");

    assert_eq!(
        sub("PreCompact", ""),
        json!({ "continue": false, "stopReason": STOPPED })
    );
    assert_eq!(sub("PreToolUse", "Bash"), denial(STOPPED));
    assert_eq!(bed.finished(), "", "a subagent declares no outcome");
    let comments = bed.comments();
    assert!(
        comments[0].contains("The subagent t2 of the session s1"),
        "{comments:?}"
    );
    // The session itself goes on, under its own context.
    let root = note(&codex(&bed, "r1.jsonl", event("PostToolUse", "Bash"))).to_owned();
    assert!(root.starts_with("Your context is 6k tokens"), "{root}");
    assert_eq!(codex(&bed, "r1.jsonl", event("PreToolUse", "Bash")), Value::Null);
    bed.kept_hard_quiet();
}
