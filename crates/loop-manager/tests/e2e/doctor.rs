//! `loop-manager doctor` and the service files on fake harness, mise and br binaries (docs/design/config.md,
//! docs/guide/install.md).

use crate::common::FILES;
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
    fs::write(d.join("config/loop-manager/config.toml"), config).unwrap();
    script(&d.join("tools/claude"), "echo '2.1.322 (Claude Code)'");
    script(&d.join("tools/codex"), "echo 'codex-cli 0.167.2'");
    let codex = d.join("tools/codex").display().to_string();
    script(
        &d.join("bin/mise"),
        &format!("[ \"$1 $2\" = 'which codex' ] && echo {codex} && exit 0\nexit 1"),
    );
    dir
}

fn loop_manager(dir: &Path, args: &[&str]) -> Output {
    loop_manager_env(dir, args, &[])
}

fn loop_manager_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_loop-manager"))
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
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

/// `loop-manager doctor --json` on the config file of `dir`, with no daemon.
fn doctor(dir: &Path) -> (bool, Vec<(String, String, String)>) {
    doctor_env(dir, &[])
}

fn doctor_env(dir: &Path, env: &[(&str, &str)]) -> (bool, Vec<(String, String, String)>) {
    let config = dir.join("config/loop-manager/config.toml").display().to_string();
    findings(&loop_manager_env(dir, &["doctor", "--json", "--config", &config], env))
}

/// The output of `loop-manager doctor --json`: whether it passed, and the level and detail of each check by name.
fn findings(out: &Output) -> (bool, Vec<(String, String, String)>) {
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

    // The provider logs codex in without `auth.json`: an unset key is named, a provider without one is not checked.
    let provider = |rest: &str| {
        let text = format!("model_provider = \"proxy\"\n[model_providers.proxy]\n{rest}");
        fs::write(dir.path().join(".codex/config.toml"), text).unwrap();
        let found = doctor(dir.path()).1;
        let (level, detail) = level(&found, "codex login");
        (level.to_owned(), detail.to_owned())
    };
    let (unset, detail) = provider("env_key = \"LOOP_MANAGER_TEST_NO_SUCH_KEY\"\n");
    assert!(
        unset == "warning" && detail.starts_with("LOOP_MANAGER_TEST_NO_SUCH_KEY, the `env_key`"),
        "{detail}"
    );
    let (keyless, detail) = provider("base_url = \"http://127.0.0.1:9\"\n");
    assert!(keyless == "warning" && detail.starts_with("not checked: "), "{detail}");
}

#[test]
fn a_login_counts_only_when_a_session_the_daemon_starts_gets_it() {
    let dir = tree("");
    // The daemon's own keys reach no session (docs/design/config.md).
    let keys = [("ANTHROPIC_API_KEY", "k"), ("OPENAI_API_KEY", "k")];
    let found = doctor_env(dir.path(), &keys).1;
    assert_eq!(level(&found, "claude login").0, "warning", "{found:?}");
    assert_eq!(level(&found, "codex login").0, "warning", "{found:?}");

    let agents = dir.path().join("config/loop-manager/shop/agents");
    for (name, harness, key) in [("dev", "claude", "ANTHROPIC_API_KEY"), ("ops", "codex", "")] {
        fs::create_dir_all(agents.join(name)).unwrap();
        let env = if key.is_empty() {
            String::new()
        } else {
            format!("[env]\n{key} = \"k\"\n")
        };
        fs::write(
            agents.join(name).join("agent.toml"),
            format!("harness = \"{harness}\"\n{env}"),
        )
        .unwrap();
    }
    let found = doctor_env(dir.path(), &keys).1;
    let claude = level(&found, "claude login");
    assert_eq!(claude, ("ok", "every claude agent has a key in `env`"), "{found:?}");
    let (codex, why) = level(&found, "codex login");
    assert!(
        codex == "warning" && why.ends_with("these agents a key in `env`: shop/root@shop.ops, shop/web@shop.ops"),
        "{found:?}"
    );
}

#[test]
fn a_harness_with_no_adapter_is_a_config_error_naming_the_known_ones() {
    let dir = tree("[harness.opencode]\n");
    let (passed, found) = doctor(dir.path());
    let (config, why) = level(&found, "config");
    assert!(!passed && config == "error", "{found:?}");
    assert!(
        why.ends_with("`harness.opencode`: loop-manager has no adapter for harness `opencode`; known: claude, codex")
    );
    let config = dir.path().join("config/loop-manager/config.toml");
    let out = loop_manager(dir.path(), &["config", "check", "--config", config.to_str().unwrap()]);
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("known: claude, codex"));
}

#[test]
fn a_tracker_project_gets_the_br_version_and_its_license_warning() {
    let dir = tree("");
    fs::create_dir_all(dir.path().join("config/loop-manager/shop/.beads")).unwrap();
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
fn without_a_runtime_directory_the_state_directory_is_checked_once() {
    let dir = tree("");
    let (_, found) = doctor_env(dir.path(), &[("XDG_RUNTIME_DIR", "")]);
    let dirs = found.iter().filter(|(name, ..)| name.starts_with("dir ")).count();
    assert_eq!(dirs, 1, "{found:?}");
}

#[test]
fn a_directory_others_can_read_and_a_broken_config_are_errors() {
    let dir = tree("");
    let state = dir.path().join("state/loop-manager");
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
    let config = dir.path().join("config/loop-manager/config.toml");
    let config = config.to_str().unwrap();
    let exe = env!("CARGO_BIN_EXE_loop-manager");
    let systemd = loop_manager(dir.path(), &["daemon", "--config", config, "--service", "systemd"]);
    let unit = String::from_utf8(systemd.stdout).unwrap();
    assert!(systemd.status.success());
    assert!(
        unit.contains(&format!("ExecStart=\"{exe}\" \"daemon\" \"--config\" \"{config}\"")),
        "{unit}"
    );
    assert!(unit.contains("Environment=\"PATH="), "{unit}");
    let launchd = loop_manager(dir.path(), &["daemon", "--service", "launchd"]);
    let plist = String::from_utf8(launchd.stdout).unwrap();
    assert!(
        plist.contains(&format!("<string>{exe}</string>\n    <string>daemon</string>")),
        "{plist}"
    );
    assert!(
        !dir.path().join("run/loop-manager/loop-manager.sock").exists(),
        "no daemon starts"
    );
}

#[test]
fn stdio_mcp_servers_of_the_host_are_warned_about() {
    let dir = tree(
        "[settings.mcp.lint]\ncommand = [\"lint\"]\n[settings.mcp.solo]\ncommand = [\"solo\"]\nscope = \"session\"\n",
    );
    let agent = dir.path().join("config/loop-manager/shop/agents/dev/agent.toml");
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
    assert_eq!(level(&doctor(dir.path()).1, "codex sandbox"), ("ok", "no codex agents"));
    let shop = dir.path().join("config/loop-manager/shop");
    fs::create_dir_all(shop.join("api")).unwrap();
    let rigs = "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n\
                [rigs.api.settings.native.codex]\nsandbox_mode = \"workspace-write\"\n";
    fs::write(shop.join("loop-manager.toml"), rigs).unwrap();
    let agent = shop.join("agents/fix/agent.toml");
    fs::create_dir_all(agent.parent().unwrap()).unwrap();
    fs::write(&agent, "harness = \"codex\"\n").unwrap();
    let (_, found) = doctor(dir.path());
    let (sandbox, why) = level(&found, "codex sandbox");
    assert!(
        sandbox == "warning"
            && why.contains("danger-full-access")
            && why.ends_with(": shop/root@shop.fix, shop/web@shop.fix"),
        "only the places without their own sandbox_mode are named: {why}"
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

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_answers_the_same_checks_over_the_api() {
    let d = crate::common::Daemon::start(|_| {}).await;
    let names = |found: &Value| -> Vec<String> {
        let found = found.as_array().unwrap().iter();
        found.map(|f| f["check"].as_str().unwrap().to_owned()).collect()
    };
    let api = d.get("/v1/doctor").await;
    let config = d
        .dir
        .path()
        .join("config/loop-manager/config.toml")
        .display()
        .to_string();
    let local = loop_manager(d.dir.path(), &["doctor", "--json", "--config", &config]);
    let local: Value = serde_json::from_slice(&local.stdout).unwrap();
    assert_eq!(names(&api), names(&local), "{api}");
    // Without `--config` the CLI asks the daemon, whatever config file its own environment names.
    let other = [("XDG_CONFIG_HOME", "/nonexistent")];
    let asked = loop_manager_env(d.dir.path(), &["doctor", "--json"], &other);
    let asked: Value = serde_json::from_slice(&asked.stdout).unwrap();
    assert_eq!(names(&api), names(&asked), "{asked}");
    let table = loop_manager_env(d.dir.path(), &["doctor"], &other);
    let table = String::from_utf8_lossy(&table.stdout);
    assert!(
        table.starts_with(&format!("ok       config           {config}\n")),
        "{table}"
    );
    let state = api.as_array().unwrap().iter().find(|f| f["check"] == "state").unwrap();
    assert_eq!(state["level"], "ok", "the state the daemon holds open: {state}");
    assert!(state["detail"].as_str().unwrap().starts_with("outbox: "), "{state}");
}

/// The daemon with no config file at the default path runs on the defaults and says so.
#[tokio::test(flavor = "multi_thread")]
async fn with_no_config_file_the_defaults_are_named() {
    let d =
        crate::common::Daemon::start(|dir| fs::remove_file(dir.join("config/loop-manager/config.toml")).unwrap()).await;
    let (_, found) = findings(&loop_manager(d.dir.path(), &["doctor", "--json"]));
    let (config, why) = level(&found, "config");
    assert!(
        config == "ok" && why.starts_with("no config file at ") && why.ends_with(", the defaults"),
        "{found:?}"
    );
}

#[test]
fn a_rig_with_the_prefix_of_the_br_database_is_warned_about() {
    let dir = tree("");
    let shop = dir.path().join("config/loop-manager/shop");
    let init = Command::new("br")
        .args(["init", "--prefix", "w", "-q"])
        .env("BEADS_DIR", shop.join(".beads"))
        .env("RUST_LOG", "error")
        .current_dir(&shop)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    // The pinned br on the test's PATH, after the fake mise.
    let path = format!(
        "{}:{}",
        dir.path().join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    // Rig `web` takes its acronym `w`, the database's prefix.
    let (_, found) = doctor_env(dir.path(), &[("PATH", &path)]);
    let (level, why) = self::level(&found, "br prefix");
    assert!(level == "warning" && why.contains("`web`") && why.contains("`shop`") && why.contains("`w`"));
    fs::write(
        shop.join("loop-manager.toml"),
        "[rigs.web]\npath = \"web\"\nprefix = \"wb\"\n",
    )
    .unwrap();
    let (_, found) = doctor_env(dir.path(), &[("PATH", &path)]);
    assert!(found.iter().all(|f| f.0 != "br prefix"), "{found:?}");
}
