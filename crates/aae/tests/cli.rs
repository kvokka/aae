//! The CLI against a real `aae daemon` (spec §12): a thin client with JSON output.

mod common;

use common::Daemon;
use serde_json::Value;
use std::{
    fs,
    process::{Command, Output},
};

impl Daemon {
    /// `aae <args>` on this daemon's socket.
    fn cli(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aae"));
        command
            .args(args)
            .env("AAE_SOCKET", self.socket())
            .envs(env.iter().copied());
        command.output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.cli(&[args, &["--json"]].concat(), &[]);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

#[tokio::test]
async fn runs_and_gates_by_hand_from_the_cli() {
    let d = Daemon::start(|_| {}).await;
    d.json(&["gate", "close", "project:shop", "--reason", "pause", "--key", "g1"]);
    d.json(&["gate", "close", "project:shop", "--reason", "pause", "--key", "g1"]);
    assert_eq!(d.json(&["gate", "list"])[0]["key"], "project:shop");
    let out = d.cli(
        &["run", "start", "shop", "web", "check", "--key", "k1", "--input", "n=2"],
        &[],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let run = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert_eq!(
        d.json(&["run", "start", "shop", "web", "check", "--key", "k1"])["run"],
        run.as_str()
    );
    let view = d.json(&["run", "show", &run]);
    assert_eq!((&view["state"], &view["inputs"]["n"]), (&"queued".into(), &2.into()));
    d.json(&["gate", "open", "project:shop"]);
    d.until_state(&run, "passed").await;
    assert_eq!(d.json(&["run", "list", "--state", "passed"])[0]["id"], run.as_str());
    // The live view of a run that ended replays its events and ends too.
    let out = d.cli(&["run", "watch", &run], &[]);
    let watched = String::from_utf8(out.stdout).unwrap();
    assert!(watched.trim_end().ends_with(&format!("run.passed  {run}")), "{watched}");
    assert_eq!(d.json(&["status"])["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn the_finisher_reports_the_answer_and_needs_the_session_environment() {
    let d = Daemon::start(|_| {}).await;
    let out = d.cli(&["finish", "pass", "--summary", "done"], &[]);
    assert!(!out.status.success());
    let env = [("AAE_RUN", "nope"), ("AAE_ATTEMPT_TOKEN", "bad")];
    let out = d.cli(&["finish", "pass", "--summary", "done", "--data", "{\"n\":1}"], &env);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "invalid");
    assert_eq!(out.status.code(), Some(1));
}

#[tokio::test]
async fn without_a_daemon_a_client_command_fails_and_names_the_daemon() {
    let d = Daemon::start(|_| {}).await;
    let out = d.cli(&["status"], &[("AAE_SOCKET", "/nonexistent/aae.sock")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("aae daemon"));
}

#[tokio::test]
async fn the_finisher_gives_up_on_an_unreachable_daemon_with_tempfail() {
    let d = Daemon::start(|_| {}).await;
    let env = [
        ("AAE_SOCKET", "/nonexistent/aae.sock"),
        ("AAE_RUN", "r1"),
        ("AAE_ATTEMPT_TOKEN", "t"),
        ("AAE_FINISH_RETRY", "1"),
    ];
    let out = d.cli(&["finish", "pass", "--summary", "done"], &env);
    assert_eq!(out.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unreachable for 1 s"));
}

/// The config by hand (spec §12): lists, the effective config of an agent with the layer of every value, a check
/// and a reload that a broken file does not pass, and the triggers of the reloaded config with their next firing.
#[tokio::test]
async fn the_config_from_the_cli_lists_explains_checks_and_reloads() {
    const CONFIG: &str =
        "projects = [\"shop\"]\n[harness.claude]\nbinary = \"true\"\n[settings]\nenv = { LANG = \"C\" }\n";
    let d = Daemon::start(|dir| {
        let aae = dir.join("config/aae");
        fs::write(aae.join("config.toml"), CONFIG).unwrap();
        fs::create_dir_all(aae.join("shop/agents/dev")).unwrap();
        fs::write(
            aae.join("shop/agents/dev/agent.toml"),
            "harness = \"claude\"\neffort = \"high\"\n",
        )
        .unwrap();
    })
    .await;
    assert_eq!(d.json(&["project", "list"])[0]["rigs"][0]["name"], "web");
    assert_eq!(d.json(&["agent", "list", "--project", "shop"])[0]["agent"], "shop.dev");
    assert_eq!(d.json(&["formula", "list"]).as_array().unwrap().len(), 2);
    assert_eq!(d.json(&["pack", "list"])[0]["name"], "shop");
    assert_eq!(d.json(&["plugin", "list"])[0]["name"], "default-policy");
    assert!(!d.cli(&["agent", "list", "--project", "nope"], &[]).status.success());
    let out = d.cli(&["config", "explain", "--project", "shop", "--agent", "dev"], &[]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("effort = \"high\"  # agent\n"), "{text}");
    assert!(text.contains("env.LANG = \"C\"  # orchestrator\n"), "{text}");
    assert!(
        text.ends_with("host: inherited by the harness, its content is not visible to aae\n"),
        "{text}"
    );

    let file = d.dir.path().join("config/aae/config.toml");
    fs::write(&file, "projects = [\"shop\"\n").unwrap();
    let out = d.cli(&["config", "check"], &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("config.toml"));
    assert!(!d.cli(&["config", "reload"], &[]).status.success());
    assert_eq!(
        d.json(&["agent", "list"])[0]["agent"],
        "shop.dev",
        "the daemon keeps its config"
    );
    // The adapter checks its own settings: claude takes none, codex its sessions and idle time (contracts 4.5).
    let claude = CONFIG.replace("binary = \"true\"\n", "binary = \"true\"\nmax_sessions = 3\n");
    let codex = |extra: &str| format!("{CONFIG}[harness.codex]\n{extra}");
    for (text, error) in [
        (claude, Some("`harness.claude`: unknown key `max_sessions`")),
        (
            codex("max_sessions = 0\n"),
            Some("`harness.codex`: `max_sessions`: invalid value"),
        ),
        (codex("idle = 3\n"), Some("`harness.codex`")),
        (codex("max_sessions = 3\ninstance_idle_s = 0\n"), None),
    ] {
        fs::write(&file, &text).unwrap();
        let out = d.cli(&["config", "check"], &[]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        match error {
            Some(error) => assert!(!out.status.success() && stderr.contains(error), "{text}: {stderr}"),
            None => assert!(out.status.success(), "{text}: {stderr}"),
        }
    }

    fs::write(&file, CONFIG).unwrap();
    let trigger = "[[trigger]]\nname = \"tick\"\nevery = \"1h\"\nrun = \"check\"\nrig = \"web\"\n";
    let project = d.dir.path().join("config/aae/shop/aae.toml");
    fs::write(&project, format!("[rigs.web]\npath = \"web\"\n{trigger}")).unwrap();
    assert!(d.json(&["trigger", "list"]).as_array().unwrap().is_empty());
    assert_eq!(d.json(&["config", "reload"])["config_generation"], 2);
    let triggers = d.json(&["trigger", "list"]);
    assert_eq!(
        (&triggers[0]["trigger"], &triggers[0]["schedule"]),
        (&"shop.tick".into(), &"3600s".into())
    );
    assert!(triggers[0]["next_at"].is_string(), "{triggers}");
}
