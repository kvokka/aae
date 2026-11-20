//! Local CLI commands (docs/api/frontends.md).

use crate::common;
use serde_json::Value;
use std::{fs, io::Write};

#[test]
fn format_reads_one_json_value_without_a_daemon() {
    let format = |args: &[&str], input: &str| {
        let (reader, mut writer) = std::io::pipe().unwrap();
        writer.write_all(input.as_bytes()).unwrap();
        drop(writer);
        common::loop_manager()
            .arg("format")
            .args(args)
            .env_remove("HOME")
            .env_remove("XDG_RUNTIME_DIR")
            .env_remove("LOOP_MANAGER_SOCKET")
            .stdin(reader)
            .output()
            .unwrap()
    };
    let input = r#"{"answer":["yes","no"]}"#;
    let toon = format(&[], input);
    assert!(toon.status.success());
    assert_eq!(toon.stdout, b"answer[2]: yes,no\n");
    let json = format(&["--json"], input);
    assert!(json.status.success());
    assert_eq!(json.stdout, format!("{input}\n").as_bytes());
    for invalid in ["", "{", "{} []"] {
        let out = format(&["--json"], invalid);
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        let error: Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(error["error"]["code"], "usage");
    }
}

/// `config check` and `config explain` of a file named with `--config` load it as the daemon would, so they need none
/// (docs/design/config.md).
#[test]
fn the_config_is_checked_and_explained_without_a_daemon() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in crate::common::FILES {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let files = dir.path().join("config/loop-manager");
    let claude = "projects = [\"shop\"]\n[harness.claude]\nbinary = \"true\"\n";
    fs::write(files.join("config.toml"), claude).unwrap();
    fs::create_dir_all(files.join("shop/agents/dev")).unwrap();
    fs::write(
        files.join("shop/agents/dev/agent.toml"),
        "harness = \"claude\"\neffort = \"high\"\n",
    )
    .unwrap();
    fs::create_dir_all(files.join("shop/formulas")).unwrap();
    let formula = "name = \"go\"\n[[step]]\nkind = \"ai\"\nagent = \"dev\"\neffort = \"low\"\n";
    fs::write(files.join("shop/formulas/go.toml"), formula).unwrap();
    fs::write(files.join("shop/agents/dev/prompt.template.md"), "Go.\n").unwrap();
    let broken = files.join("broken.toml");
    fs::write(&broken, "projects = [\"shop\"\n").unwrap();
    let broken = broken.to_str().unwrap();
    let loop_manager = |args: &[&str]| {
        common::loop_manager()
            .args(args)
            .env("LOOP_MANAGER_SOCKET", "/nonexistent/loop-manager.sock")
            .env("HOME", dir.path())
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .output()
            .unwrap()
    };
    let config = files.join("config.toml").display().to_string();
    let local = |args: &[&str]| loop_manager(&[args, &["--config", &config]].concat());
    let out = local(&["config", "check"]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"errors: []\nok: true\n");
    let out = local(&["config", "check", "--json"]);
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(out.status.success(), "{reply}");
    assert_eq!(reply, serde_json::json!({ "ok": true, "errors": [] }));
    let out = local(&["config", "explain", "--project", "shop", "--agent", "dev"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && text.contains("effort = \"high\"  # agent\n"),
        "{text}"
    );
    let out = local(&["config", "explain", "--project", "shop", "--agent", "dev", "--json"]);
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reply["inherits_host"], true, "{reply}");
    // With a formula the step's layer counts, over the agent's.
    let go = ["config", "explain", "--project", "shop", "--formula", "go"];
    let out = local(&[&go[..], &["--rig", "web", "--json"]].concat());
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    let text = reply["text"].as_str().unwrap_or_default();
    assert!(text.contains("effort = \"low\"  # formula shop.go"), "{reply}");
    assert!(
        text.contains("# prompt template: ") && text.contains("agents/dev/prompt.template.md\n"),
        "{reply}"
    );
    assert_eq!(reply["steps"][0]["agent"], "shop.dev", "{reply}");
    // With no rig, the formula is explained on the project root.
    let out = local(&[&go[..], &["--json"]].concat());
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    let text = reply["text"].as_str().unwrap_or_default();
    assert!(out.status.success() && text.contains("on `root`"), "{reply}");
    let out = local(&["--json", "config", "explain", "--project", "nope", "--agent", "dev"]);
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(
        (out.status.code(), &error["error"]["code"]),
        (Some(1), &"not_found".into())
    );
    let out = loop_manager(&["config", "check", "--config", broken]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("broken.toml"));
    let out = loop_manager(&[
        "config",
        "explain",
        "--config",
        broken,
        "--project",
        "shop",
        "--agent",
        "dev",
    ]);
    assert!(!out.status.success());
    // A named config must exist; a relative one resolves against the working directory, and its project paths are
    // absolute after that.
    let out = loop_manager(&["config", "check", "--config", "/nonexistent/config.toml"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success() && text.contains("/nonexistent/config.toml"),
        "{text}"
    );
    let out = common::loop_manager()
        .args([
            "config",
            "explain",
            "--config",
            "config.toml",
            "--project",
            "shop",
            "--agent",
            "dev",
        ])
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .current_dir(&files)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let beads = format!("env.BEADS_DIR = \"{}\"", files.join("shop/.beads").display());
    assert!(text.contains(&beads), "{text}");
    // A reader that went away (`loop-manager config explain | head -1`): a quiet SIGPIPE exit, not a panic.
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = common::loop_manager()
        .args(["config", "explain", "--project", "shop", "--agent", "dev"])
        .args(["--config", &config])
        .stdout(writer)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(141), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
}
