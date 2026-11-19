//! The addon `addons/context-budget` of this repository: its params reach claude's settings and hooks, and its hook
//! measures a session's context from the transcript, tells the agent to wrap up past `soft` and stops it past `hard`,
//! on fake transcripts and hook inputs of claude (addons/context-budget/README.md).

use crate::common;
use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path, process::Stdio};

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

/// A hook test bed: a `br` tracker with one bead, a stand-in `loop-manager` and the addon's data directory, with a
/// budget of 1000 soft, 7000 hard and a note every 500.
struct Bed {
    dir: tempfile::TempDir,
    bead: String,
    /// What the hooks printed, to look for the hard limit in.
    said: std::cell::RefCell<String>,
    /// Whether the run has a bead: `LOOP_MANAGER_BEAD` is set for the hook.
    with_bead: std::cell::Cell<bool>,
}

impl Bed {
    fn new(cut: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let br = |args: &[&str]| br(d, args);
        br(&["init", "-q"]);
        let created: Value = serde_json::from_str(&br(&["create", "task", "--json"])).unwrap();
        fs::create_dir_all(d.join("bin")).unwrap();
        fs::create_dir_all(d.join("data")).unwrap();
        fs::create_dir_all(d.join("work")).unwrap();
        git(&d.join("work"), &["init", "-q"]);
        commit(&d.join("work"), "start");
        fs::write(d.join("work/notes.md"), "half done").unwrap();
        let fake = "#!/bin/sh\necho \"$*\" >>\"$(dirname \"$0\")/finished\"\necho accepted\n";
        fs::write(d.join("bin/loop-manager"), fake).unwrap();
        fs::set_permissions(d.join("bin/loop-manager"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(d.join("cut"), cut).unwrap();
        Bed {
            dir,
            bead: created["id"].as_str().unwrap().to_owned(),
            said: Default::default(),
            with_bead: std::cell::Cell::new(true),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Writes the transcript `name` whose last answer of the model holds `tokens` of context, after a tool call.
    fn transcript(&self, name: &str, tokens: u64) -> String {
        let usage = |input: u64| {
            json!({ "input_tokens": input / 2, "cache_creation_input_tokens": 10, "cache_read_input_tokens":
                input - input / 2 - 30, "output_tokens": 20 })
        };
        let lines = [
            json!({ "type": "user", "message": { "role": "user", "content": "the usage of the parser" } }),
            json!({ "type": "assistant", "message": { "model": "m", "usage": usage(100), "content": [
                { "type": "text", "text": "I read the parser first." },
                { "type": "tool_use", "name": "Bash", "input": { "command": "cargo test -p parser" } }] } }),
            json!({ "type": "assistant", "message": { "model": "m", "usage": usage(tokens), "content": [
                { "type": "tool_use", "name": "Edit", "input": { "file_path": "src/parser.rs" } }] } }),
            json!({ "type": "assistant", "message": { "model": "<synthetic>", "usage": usage(99999),
                "content": [{ "type": "text", "text": "No response requested." }] } }),
        ];
        let path = self.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        fs::write(&path, text).unwrap();
        path.display().to_string()
    }

    /// The hook on `input` (with `cwd` and `transcript_path` of the session `s1` added), its stdout parsed, or null
    /// when it printed nothing; the hook succeeded.
    fn hook(&self, mut input: Value) -> Value {
        input["session_id"] = json!("s1");
        input["cwd"] = json!(self.path().join("work"));
        if input.get("transcript_path").is_none() {
            input["transcript_path"] = json!(self.path().join("s1.jsonl"));
        }
        let d = self.path();
        let cut = fs::read_to_string(d.join("cut")).unwrap();
        let data = d.join("data");
        let mut command = common::program(addon().join("hooks/budget"));
        command
            .args([data.to_str().unwrap(), "1000", "7000", "500", &cut])
            .env("BEADS_DIR", d.join(".beads"))
            .env("LOOP_MANAGER_RUN", "r1")
            .env("RUST_LOG", "error")
            .env(
                "PATH",
                format!("{}:{}", d.join("bin").display(), std::env::var("PATH").unwrap()),
            );
        if self.with_bead.get() {
            command.env("LOOP_MANAGER_BEAD", &self.bead);
        }
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "budget on {input}");
        let text = String::from_utf8(out.stdout).unwrap();
        self.said.borrow_mut().push_str(&text);
        if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap()
        }
    }

    fn tool(&self, event: &str, tool: &str) -> Value {
        self.hook(json!({ "hook_event_name": event, "tool_name": tool }))
    }

    fn comments(&self) -> Vec<String> {
        let out = br(self.path(), &["comments", "list", &self.bead, "--json"]);
        let list: Vec<Value> = serde_json::from_str(&out).unwrap();
        list.iter().map(|c| c["text"].as_str().unwrap().to_owned()).collect()
    }

    fn finished(&self) -> String {
        fs::read_to_string(self.path().join("bin/finished")).unwrap_or_default()
    }

    /// Nothing the hooks said holds the hard limit.
    fn kept_hard_quiet(&self) {
        let said = self.said.borrow();
        assert!(!said.contains("7k") && !said.contains("7000"), "{said}");
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = common::program("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
}

fn commit(dir: &Path, message: &str) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    git(
        dir,
        &[&id[..], &["commit", "-q", "--allow-empty", "-m", message]].concat(),
    );
}

fn br(dir: &Path, args: &[&str]) -> String {
    let out = common::program("br")
        .args(args)
        .current_dir(dir)
        .env("BEADS_DIR", dir.join(".beads"))
        .env("RUST_LOG", "error")
        .output()
        .unwrap();
    assert!(out.status.success(), "br {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn note(answer: &Value) -> &str {
    answer["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
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
    let denied = bed.tool("PreToolUse", "Agent");
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
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
