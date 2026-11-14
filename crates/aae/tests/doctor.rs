//! `aae doctor` and the service files on fake harness, mise and br binaries (spec §6.5, §13).

mod common;

use common::FILES;
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

/// An executable script at `path`.
fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The XDG tree of the daemon tests with claude at an explicit path and codex where a fake mise says it is.
fn tree(extra: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for (path, text) in FILES {
        fs::create_dir_all(d.join(path).parent().unwrap()).unwrap();
        fs::write(d.join(path), text).unwrap();
    }
    let config = format!(
        "projects = [\"shop\"]\n[harness.claude]\nbinary = \"{}\"\n[harness.codex]\n{extra}",
        d.join("tools/claude").display()
    );
    fs::write(d.join("config/aae/config.toml"), config).unwrap();
    script(&d.join("tools/claude"), "echo '2.1.322 (Claude Code)'");
    script(&d.join("tools/codex"), "echo 'codex-cli 0.167.2'");
    let codex = d.join("tools/codex").display().to_string();
    script(
        &d.join("bin/mise"),
        &format!("[ \"$1 $2\" = 'which codex' ] && echo {codex} && exit 0\nexit 1"),
    );
    dir
}

fn aae(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aae"))
        .args(args)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("PATH", format!("{}:/usr/bin:/bin", dir.join("bin").display()))
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .output()
        .unwrap()
}

/// `aae doctor --json`: whether it passed, and the level and detail of each check by name.
fn doctor(dir: &Path) -> (bool, Vec<(String, String, String)>) {
    let out = aae(dir, &["doctor", "--json"]);
    let found: Value = serde_json::from_slice(&out.stdout).unwrap();
    let found = found.as_array().unwrap().iter().map(|f| {
        let s = |k: &str| f[k].as_str().unwrap().to_owned();
        (s("check"), s("level"), s("detail"))
    });
    (out.status.success(), found.collect())
}

fn level<'a>(found: &'a [(String, String, String)], check: &str) -> (&'a str, &'a str) {
    let f = found
        .iter()
        .find(|f| f.0 == check)
        .unwrap_or_else(|| panic!("no `{check}` in {found:?}"));
    (f.1.as_str(), f.2.as_str())
}

#[test]
fn harnesses_resolve_from_the_config_and_from_mise_and_a_missing_one_fails() {
    let dir = tree("");
    let (passed, found) = doctor(dir.path());
    assert!(passed, "{found:?}");
    let (claude, path) = level(&found, "claude");
    assert!(
        claude == "ok" && path.contains("tools/claude `") && path.ends_with("` (from config)"),
        "{found:?}"
    );
    let (codex, path) = level(&found, "codex");
    assert!(
        codex == "ok" && path.ends_with("tools/codex `codex-cli 0.167.2` (from mise)"),
        "{found:?}"
    );
    assert_eq!(level(&found, "claude login").0, "warning", "no login in the fake home");
    assert_eq!(level(&found, "codex login").0, "warning", "no login in the fake home");
    assert_eq!(level(&found, "state").0, "ok");

    script(&dir.path().join(".claude/.credentials.json"), "{}");
    // A host provider whose key is a variable the daemon has: `PATH` stands in for it.
    fs::create_dir_all(dir.path().join(".codex")).unwrap();
    fs::write(
        dir.path().join(".codex/config.toml"),
        "model_provider = \"proxy\"\n[model_providers.proxy]\nenv_key = \"PATH\"\n",
    )
    .unwrap();
    fs::remove_file(dir.path().join("tools/codex")).unwrap();
    let (passed, found) = doctor(dir.path());
    assert!(!passed, "a missing harness binary is an error");
    assert_eq!(level(&found, "codex"), ("error", level(&found, "codex").1));
    assert!(level(&found, "codex").1.ends_with("not found"), "{found:?}");
    assert_eq!(level(&found, "claude login").0, "ok");
    assert_eq!(level(&found, "codex login"), ("ok", "PATH is set"));
}

#[test]
fn a_tracker_project_gets_the_br_version_and_its_license_warning() {
    let dir = tree("");
    fs::create_dir_all(dir.path().join("config/aae/shop/.beads")).unwrap();
    script(&dir.path().join("bin/br"), "echo 'br 0.7.5'");
    let (passed, found) = doctor(dir.path());
    assert!(passed, "warnings do not fail: {found:?}");
    let (br, why) = level(&found, "br");
    assert!(br == "warning" && why.contains("tested: 0.7.6"), "{found:?}");
    assert!(
        why.starts_with(&dir.path().join("bin/br").display().to_string()),
        "found on PATH: {why}"
    );
    assert!(level(&found, "br license").1.contains("rider"));
}

#[test]
fn a_directory_others_can_read_and_a_broken_config_are_errors() {
    let dir = tree("");
    let state = dir.path().join("state/aae");
    fs::create_dir_all(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).unwrap();
    let (passed, found) = doctor(dir.path());
    assert!(!passed);
    let (level, why) = level(&found, &format!("dir {}", state.display()));
    assert!(level == "error" && why.contains("chmod 700"), "{found:?}");

    let dir = tree("modle = \"x\"\n");
    let (passed, found) = doctor(dir.path());
    assert!(
        !passed && found.iter().any(|f| f.0 == "config" && f.1 == "error"),
        "{found:?}"
    );
}

#[test]
fn service_files_run_this_binary_with_the_config() {
    let dir = tree("");
    let config = dir.path().join("config/aae/config.toml");
    let config = config.to_str().unwrap();
    let exe = env!("CARGO_BIN_EXE_aae");
    let systemd = aae(dir.path(), &["daemon", "--config", config, "--service", "systemd"]);
    let unit = String::from_utf8(systemd.stdout).unwrap();
    assert!(systemd.status.success());
    assert!(
        unit.contains(&format!("ExecStart=\"{exe}\" \"daemon\" \"--config\" \"{config}\"")),
        "{unit}"
    );
    assert!(unit.contains("Environment=\"PATH="), "{unit}");
    let launchd = aae(dir.path(), &["daemon", "--service", "launchd"]);
    let plist = String::from_utf8(launchd.stdout).unwrap();
    assert!(
        plist.contains(&format!("<string>{exe}</string>\n    <string>daemon</string>")),
        "{plist}"
    );
    assert!(!dir.path().join("run/aae/aae.sock").exists(), "no daemon starts");
}

#[test]
fn stdio_mcp_servers_of_the_host_are_warned_about() {
    let dir = tree(
        "[settings.mcp.lint]\ncommand = [\"lint\"]\n[settings.mcp.solo]\ncommand = [\"solo\"]\nscope = \"session\"\n",
    );
    let agent = dir.path().join("config/aae/shop/agents/dev/agent.toml");
    fs::create_dir_all(agent.parent().unwrap()).unwrap();
    fs::write(agent, "harness = \"claude\"\n").unwrap();
    let (passed, found) = doctor(dir.path());
    assert!(passed, "{found:?}");
    assert_eq!(level(&found, "claude host mcp").0, "ok");
    assert_eq!(level(&found, "codex host mcp").0, "ok");
    assert_eq!(
        level(&found, "codex sandbox").0,
        "ok",
        "a claude agent runs in no codex sandbox"
    );

    let host = r#"{"mcpServers": {"deja": {"command": "deja"}, "web": {"type": "http", "url": "http://x"}},
        "projects": {"/p": {"mcpServers": {"local": {"type": "stdio", "command": "l"}}}}}"#;
    fs::write(dir.path().join(".claude.json"), host).unwrap();
    fs::create_dir_all(dir.path().join(".codex")).unwrap();
    fs::write(
        dir.path().join(".codex/config.toml"),
        "[mcp_servers.notes]\ncommand = \"n\"\n",
    )
    .unwrap();
    let (passed, found) = doctor(dir.path());
    assert!(passed, "warnings do not fail: {found:?}");
    let (claude, why) = level(&found, "claude host mcp");
    assert!(claude == "warning" && why.contains("deja, local;"), "{found:?}");
    let (codex, why) = level(&found, "codex host mcp");
    assert!(codex == "warning" && why.contains(": notes;"), "{found:?}");
}

#[test]
fn the_sandbox_warning_names_each_codex_agent_that_sets_no_sandbox_mode() {
    let dir = tree("");
    let agent = dir.path().join("config/aae/shop/agents/fix/agent.toml");
    fs::create_dir_all(agent.parent().unwrap()).unwrap();
    fs::write(&agent, "harness = \"codex\"\n").unwrap();
    let (_, found) = doctor(dir.path());
    let (sandbox, why) = level(&found, "codex sandbox");
    assert!(
        sandbox == "warning" && why.contains("danger-full-access") && why.ends_with("shop/web@shop.fix"),
        "{why}"
    );
    fs::create_dir_all(dir.path().join(".codex")).unwrap();
    fs::write(
        dir.path().join(".codex/config.toml"),
        "sandbox_mode = \"workspace-write\"\n",
    )
    .unwrap();
    assert_eq!(
        level(&doctor(dir.path()).1, "codex sandbox").0,
        "ok",
        "the inherited host config sets it"
    );
    fs::remove_file(dir.path().join(".codex/config.toml")).unwrap();
    let native = "harness = \"codex\"\n[native.codex]\nsandbox_mode = \"workspace-write\"\n";
    fs::write(&agent, native).unwrap();
    assert_eq!(
        level(&doctor(dir.path()).1, "codex sandbox").0,
        "ok",
        "the native block sets it"
    );
}
