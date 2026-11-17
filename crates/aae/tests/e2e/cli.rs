//! The CLI against a real `aae daemon` (docs/api/frontends.md): a thin client with JSON output.

use crate::common::Daemon;
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
};

impl Daemon {
    /// `aae <args>` on this daemon's socket and its config directory.
    fn cli(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aae"));
        command
            .args(args)
            .env("AAE_SOCKET", self.socket())
            .env("HOME", self.dir.path())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
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
    let again = ["run", "start", "shop", "web", "check", "--key", "k1", "--input", "n=2"];
    assert_eq!(d.json(&again)["run"], run.as_str());
    // An API error under `--json` is the API's JSON error object, on stderr.
    let out = d.cli(&["run", "start", "shop", "web", "check", "--key", "k1", "--json"], &[]);
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(
        (out.status.code(), &error["error"]["code"]),
        (Some(1), &"idempotency_key_reused".into())
    );
    let out = d.cli(&["run", "start", "shop", "web", "check", "--input", "n"], &[]);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.code() == Some(2) && text.contains("`n` is not NAME=VALUE"),
        "{text}"
    );
    // The rig is optional: with none and no task input, the run works in the project root.
    let rootless = d.json(&["run", "start", "shop", "check"])["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let view = d.json(&["run", "show", &rootless]);
    assert_eq!(view["rig"], "root", "{view}");
    assert!(view["workdir"].as_str().unwrap().ends_with("/shop"), "{view}");
    d.json(&["run", "cancel", &rootless, "--reason", "by hand"]);
    let view = d.json(&["run", "show", &run]);
    assert_eq!((&view["state"], &view["inputs"]["n"]), (&"queued".into(), &2.into()));
    assert_eq!(
        (&view["gate"]["key"], &view["gate"]["reason"]),
        (&"project:shop".into(), &"pause".into())
    );
    let out = d.cli(&["gate", "open", "project:shop"], &[]);
    assert!(
        out.status.success() && out.stdout.is_empty(),
        "nothing to print, not `null`"
    );
    d.until_state(&run, "passed").await;
    assert_eq!(d.json(&["run", "show", &run])["reason"], "pass");
    let slow = d.json(&["run", "start", "shop", "web", "slow"])["run"]
        .as_str()
        .unwrap()
        .to_owned();
    d.json(&["run", "cancel", &slow, "--reason", "by hand"]);
    assert_eq!(d.until_state(&slow, "cancelled").await["reason"], "by hand");
    assert_eq!(d.json(&["run", "list", "--state", "passed"])[0]["id"], run.as_str());
    let out = d.cli(&["run", "list", "--state", "bogus"], &[]);
    assert!(
        !out.status.success() && out.stdout.is_empty(),
        "an unknown state is an error, not an empty list"
    );
    // The live view of a run that ended replays its events and ends too.
    let out = d.cli(&["run", "watch", &run], &[]);
    let watched = String::from_utf8(out.stdout).unwrap();
    assert!(watched.trim_end().ends_with(&format!("run.passed  {run}")), "{watched}");
    assert!(
        watched.contains(&format!("run.state_changed  {run}  preparing -> running")),
        "{watched}"
    );
    assert_eq!(d.json(&["status"])["version"], env!("CARGO_PKG_VERSION"));
    // Without `--json` a reply is `key: value` lines, nulls left out.
    let status = String::from_utf8(d.cli(&["status"], &[]).stdout).unwrap();
    let version = format!("version: {}\n", env!("CARGO_PKG_VERSION"));
    assert!(
        status.contains(&version) && !status.contains("null") && !status.contains('{'),
        "{status}"
    );
}

#[tokio::test]
async fn the_finisher_reports_the_answer_and_needs_the_session_environment() {
    let d = Daemon::start(|_| {}).await;
    let out = d.cli(&["finish", "pass", "--summary", "done"], &[]);
    assert!(!out.status.success());
    // A local error under `--json` is a JSON error object too.
    let out = d.cli(&["--json", "finish", "pass", "--summary", "done"], &[]);
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!((out.status.code(), &error["error"]["code"]), (Some(1), &"usage".into()));
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
    let out = d.cli(&["status", "--json"], &[("AAE_SOCKET", "/nonexistent/aae.sock")]);
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(
        (out.status.code(), &error["error"]["code"]),
        (Some(1), &"no_daemon".into())
    );
    // The checks ask the daemon too; without one, a config file can be checked by name.
    let explain = ["config", "explain", "--project", "shop", "--agent", "dev"];
    for args in [&["doctor"][..], &["config", "check"], &explain] {
        let out = d.cli(args, &[("AAE_SOCKET", "/nonexistent/aae.sock")]);
        let text = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1));
        assert!(text.contains("`--config <file>`"), "{text}");
    }
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
    let files = dir.path().join("config/aae");
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
    let aae = |args: &[&str]| {
        (Command::new(env!("CARGO_BIN_EXE_aae")).args(args))
            .env("AAE_SOCKET", "/nonexistent/aae.sock")
            .env("HOME", dir.path())
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .output()
            .unwrap()
    };
    let config = files.join("config.toml").display().to_string();
    let local = |args: &[&str]| aae(&[args, &["--config", &config]].concat());
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
    let out = aae(&["config", "check", "--config", broken]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("broken.toml"));
    let out = aae(&[
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
    let out = aae(&["config", "check", "--config", "/nonexistent/config.toml"]);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && text.contains("/nonexistent/config.toml"),
        "{text}"
    );
    let out = (Command::new(env!("CARGO_BIN_EXE_aae")))
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
    // A reader that went away (`aae config explain | head -1`): a quiet SIGPIPE exit, not a panic.
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = (Command::new(env!("CARGO_BIN_EXE_aae")))
        .args(["config", "explain", "--project", "shop", "--agent", "dev"])
        .args(["--config", &config])
        .stdout(writer)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(141), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The config by hand (docs/api/frontends.md): lists, the effective config of an agent with the layer of every value, a
/// check and a reload that a broken file does not pass, and the triggers of the reloaded config with their next firing.
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
            "harness = \"claude\"\neffort = \"high\"\nsubagents = [\"qa\"]\n",
        )
        .unwrap();
        fs::create_dir_all(aae.join("shop/agents/qa")).unwrap();
        let qa = "harness = \"claude\"\ndescription = \"Checks a change.\"\n";
        fs::write(aae.join("shop/agents/qa/agent.toml"), qa).unwrap();
        let inner = "name = \"inner\"\ninternal = true\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n";
        fs::write(aae.join("shop/formulas/inner.toml"), inner).unwrap();
    })
    .await;
    assert_eq!(d.json(&["project", "list"])[0]["rigs"][0]["name"], "web");
    let agent = &d.json(&["agent", "list", "--project", "shop"])[0];
    assert_eq!(
        (&agent["agent"], &agent["harness"], agent.get("provider")),
        (&"shop.dev".into(), &"claude".into(), None)
    );
    // The call graph, the agent itself left out of it.
    let agents = d.json(&["agent", "list"]);
    let graph = |a: &Value| (a["agent"].clone(), a["subagents"].clone(), a["called_by"].clone());
    assert_eq!(
        agents.as_array().unwrap().iter().map(graph).collect::<Vec<_>>(),
        [
            (json!("shop.dev"), json!(["shop.qa"]), json!([])),
            (json!("shop.qa"), json!([]), json!(["shop.dev"]))
        ]
    );
    let out = d.cli(&["agent", "list"], &[]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("agent: shop.dev\nharness: claude\nproject: shop\nsubagents: shop.qa\n"),
        "{text}"
    );
    assert!(
        text.contains("agent: shop.qa\ncalled_by: shop.dev\nharness: claude\n"),
        "{text}"
    );
    // An internal formula (docs/design/config.md): marked in the API, hidden from `aae formula list` unless `--all`.
    let internal = |args: &[&str]| -> Vec<Value> {
        let formulas = d.json(args);
        let inner = formulas.as_array().unwrap().iter().filter(|f| f["formula"] == "inner");
        inner.map(|f| f["internal"].clone()).collect()
    };
    assert_eq!(internal(&["formula", "list"]), Vec::<Value>::new());
    assert_eq!(internal(&["formula", "list", "--all"]), vec![Value::Bool(true)]);
    assert_eq!(d.json(&["formula", "list"]).as_array().unwrap().len(), 2);
    assert_eq!(d.json(&["pack", "list"])[0]["name"], "shop");
    let plugins = d.json(&["plugin", "list"]);
    assert_eq!(
        (&plugins[0]["project"], &plugins[0]["name"]),
        (&Value::Null, &"default-policy".into())
    );
    let shop = (plugins.as_array().unwrap().iter()).any(|p| p["project"] == "shop" && p["name"] == "default-policy");
    assert!(shop, "every project's plugins without --project: {plugins}");
    assert!(!d.cli(&["agent", "list", "--project", "nope"], &[]).status.success());
    // Asked from another environment, whose own config file is not the daemon's, the daemon answers by its config.
    let other = [("XDG_CONFIG_HOME", "/nonexistent")];
    let out = d.cli(&["config", "explain", "--project", "shop", "--agent", "dev"], &other);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("effort = \"high\"  # agent\n"), "{text}");
    assert!(text.contains("env.LANG = \"C\"  # orchestrator\n"), "{text}");
    assert!(
        text.ends_with("host: inherited by the harness, its content is not visible to aae\n"),
        "{text}"
    );

    let file = d.dir.path().join("config/aae/config.toml");
    fs::write(&file, "projects = [\"shop\"\n").unwrap();
    let out = d.cli(&["config", "check"], &other);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("config.toml"));
    let out = d.cli(&["config", "reload"], &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    // The errors of the refused file, not only that it has some.
    assert!(
        stderr.contains("config_error)\n") && stderr.contains("config.toml"),
        "{stderr}"
    );
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
