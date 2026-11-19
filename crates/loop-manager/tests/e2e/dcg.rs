//! The addon `addons/dcg` of this repository: its hook reaches claude and codex agents, hands a shell command to `dcg`
//! (pinned in `mise.toml`), which denies a destructive one, and refuses every command when `dcg` is missing
//! (addons/dcg/README.md).

use crate::common;
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, process::Stdio};

fn addon() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../addons/dcg")
}

#[test]
fn the_hook_reaches_a_claude_and_a_codex_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    let files = [
        (
            "config.toml".to_owned(),
            format!(
                "projects = [\"shop\"]\n[[addon]]\npath = \"{}\"\n[settings]\naddons = [{{ name = \"dcg\" }}]\n",
                addon().display()
            ),
        ),
        ("shop/loop-manager.toml".into(), "packs = [\"../kit\"]\n".into()),
        ("kit/pack.toml".into(), "name = \"kit\"\nversion = \"0.1.0\"\n".into()),
        ("kit/agents/claude/agent.toml".into(), "harness = \"claude\"\n".into()),
        ("kit/agents/codex/agent.toml".into(), "harness = \"codex\"\n".into()),
    ];
    for (path, text) in &files {
        fs::create_dir_all(d.join(path).parent().unwrap()).unwrap();
        fs::write(d.join(path), text).unwrap();
    }
    for agent in ["kit.claude", "kit.codex"] {
        let out = common::loop_manager()
            .args(["--json", "config", "explain", "--config"])
            .arg(d.join("config.toml"))
            .args(["--project", "shop", "--agent", agent])
            .env("HOME", d)
            .env("XDG_CACHE_HOME", d.join("cache"))
            .env("XDG_STATE_HOME", d.join("state"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
        let text = serde_json::from_str::<Value>(&text).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            text.contains("\"event\":\"PreToolUse\",\"matcher\":\"Bash\",\"command\":\"'")
                && text.contains("hooks/guard'\""),
            "{agent}: {text}"
        );
    }
}

/// The hook of the addon on `input`, in `dir` (no repository there, so no project config of `dcg`), with `dir` for a
/// home that has no user config of `dcg`, and `path` for `PATH`: its exit code, stdout and stderr.
fn guard(dir: &Path, path: &std::ffi::OsStr, input: &Value) -> (Option<i32>, String, String) {
    let mut child = common::program(addon().join("hooks/guard"))
        .current_dir(dir)
        .env("PATH", path)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env_remove("DCG_CONFIG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Without `dcg` the hook exits before it reads its input, so the write may meet a closed pipe.
    let _ = child.stdin.take().unwrap().write_all(input.to_string().as_bytes());
    let out = child.wait_with_output().unwrap();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (out.status.code(), text(&out.stdout), text(&out.stderr))
}

/// A `PreToolUse` input of `harness` for the shell command `command`: claude's, or codex's (`turn_id`, `model`).
fn input(harness: &str, command: &str) -> Value {
    let mut input = json!({
        "session_id": "s1",
        "cwd": "/tmp",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "tool_use_id": "t1",
    });
    if harness == "codex" {
        input["turn_id"] = json!("u1");
        input["model"] = json!("gpt-5.5");
    } else {
        input["transcript_path"] = json!("/tmp/s1.jsonl");
        input["permission_mode"] = json!("bypassPermissions");
    }
    input
}

#[test]
fn a_destructive_command_is_denied_and_an_ordinary_one_passes_on_both_harnesses() {
    let tmp = tempfile::tempdir().unwrap();
    let path = std::env::var_os("PATH").unwrap();
    for harness in ["claude", "codex"] {
        let (code, out, err) = guard(tmp.path(), &path, &input(harness, "git reset --hard"));
        assert_eq!(code, Some(0), "{harness}: {err}");
        let answer: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{harness}: {e}: {out}{err}"));
        let decision = &answer["hookSpecificOutput"];
        assert_eq!(decision["permissionDecision"], "deny", "{harness}: {out}");
        let reason = decision["permissionDecisionReason"].as_str().unwrap();
        assert!(reason.contains("git reset --hard"), "{harness}: {reason}");

        let (code, out, err) = guard(tmp.path(), &path, &input(harness, "git status"));
        assert_eq!(code, Some(0), "{harness}: {err}");
        assert!(!out.contains("deny"), "{harness}: {out}");
    }
}

#[test]
fn without_dcg_on_the_path_every_command_is_refused_and_the_agent_is_told_why() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = tmp.path().join("bin");
    fs::create_dir(&empty).unwrap();
    let (code, out, err) = guard(tmp.path(), empty.as_os_str(), &input("claude", "git status"));
    assert_eq!(code, Some(2), "{out}{err}");
    assert!(
        err.contains("`dcg` (destructive_command_guard) is not on the PATH"),
        "{err}"
    );
}
