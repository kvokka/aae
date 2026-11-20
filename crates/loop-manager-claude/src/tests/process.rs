//! The process part: hooks, MCP servers, environment and login of a session.

use super::*;

#[test]
fn headless_defaults_yield_to_the_agents_env_and_native_settings() {
    let env = |p: &Value| {
        part::env(p, "i1", None, Path::new("/state"))
            .unwrap()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let mut p = json!({ "env": {} });
    let defaults = env(&p);
    for key in [
        "CLAUDE_CODE_DISABLE_TERMINAL_TITLE",
        "DISABLE_TELEMETRY",
        "DISABLE_ERROR_REPORTING",
        "DISABLE_FEEDBACK_COMMAND",
        "DISABLE_AUTOUPDATER",
        "CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY",
    ] {
        assert_eq!(defaults[key], "1", "{key}");
    }
    assert_eq!(defaults["CLAUDE_CODE_ENTRYPOINT"], "loop-manager");
    p["env"]["CLAUDE_CODE_ENTRYPOINT"] = json!("sdk-cli");
    p["env"]["DISABLE_TELEMETRY"] = json!("");
    p["settings"] = json!({ "env": { "DISABLE_AUTOUPDATER": "0" } });
    let configured = env(&p);
    assert_eq!(configured["CLAUDE_CODE_ENTRYPOINT"], "sdk-cli");
    assert_eq!(configured["DISABLE_TELEMETRY"], "");
    assert!(!configured.contains_key("DISABLE_AUTOUPDATER"));
}

#[test]
fn hooks_join_the_native_settings_and_mcp_servers_take_claude_form() {
    use loop_manager_contract::config::{Hook, Mcp};
    let hook = |event: &str, enabled| Hook {
        enabled,
        event: Some(event.into()),
        matcher: None,
        command: Some("true".into()),
        timeout: Some("90s".into()),
    };
    let mut config = EffectiveConfig {
        services: Default::default(),
        harness: "claude".into(),
        harness_from: loop_manager_contract::config::Layer::Agent,
        scope: Default::default(),
        description: None,
        subagents: None,
        targets: Default::default(),
        model_given: None,
        model: None,
        effort: None,
        system_prompt: None,
        inherit_host: None,
        env: Default::default(),
        mcp: [(
            "web".to_owned(),
            agent(Mcp {
                command: None,
                env: Default::default(),
                url: Some("http://127.0.0.1:1/mcp".into()),
                headers: [("x-k".to_owned(), "v".to_owned())].into(),
                scope: None,
            }),
        )]
        .into(),
        hooks: [
            ("a".to_owned(), agent(hook("Stop", true))),
            ("b".to_owned(), agent(hook("Stop", false))),
        ]
        .into(),
        skills: Default::default(),
        addons: Vec::new(),
        native: Some(agent(
            json!({ "hooks": { "Stop": [{ "hooks": [] }] }, "env": { "X": "1" } }),
        )),
    };
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
        actor: Some(("BD_ACTOR".into(), "loop-manager:r:1".into())),
        bead: Some("bd-1".into()),
    };
    let p = part::process(&config, &plan, Path::new("/state")).unwrap();
    let env = part::env(&p, "i1", None, Path::new("/state")).unwrap();
    assert!(
        env.contains(&("BD_ACTOR".to_owned(), "loop-manager:r:1".to_owned())),
        "the attempt's tracker actor, as the core planned it"
    );
    assert!(
        env.contains(&("LOOP_MANAGER_BEAD".to_owned(), "bd-1".to_owned())),
        "the run's bead"
    );
    assert!(
        env.contains(&("CLAUDE_CODE_STARTUP_FAILURE_RESULTS".to_owned(), "1".to_owned())),
        "a claude that cannot start says why in a result"
    );
    let stop = &p["settings"]["hooks"]["Stop"];
    assert_eq!(
        stop.as_array().map(Vec::len),
        Some(2),
        "the native hook and the enabled one: {stop}"
    );
    assert_eq!(
        stop[1],
        json!({ "matcher": "", "hooks": [{ "type": "command", "command": "true", "timeout": 90 }] })
    );
    assert_eq!(p["settings"]["env"]["X"], "1");
    assert_eq!(
        p["mcp"]["mcpServers"]["web"],
        json!({ "type": "http", "url": "http://127.0.0.1:1/mcp", "headers": { "x-k": "v" } })
    );
    let args = part::args(&p);
    let settings = format!("/state/claude/{}/settings.json", p["session_id"].as_str().unwrap());
    assert!(
        args.windows(2).any(|w| w == ["--settings", settings.as_str()]),
        "{args:?}"
    );
    assert!(args.iter().any(|a| a == "--mcp-config"));
    assert!(!args.iter().any(|a| a == "--plugin-dir"), "no skills, no plugin");
    // The session id is a function of run and attempt, and the core numbers every launch as a new attempt: a second
    // `New` never meets the id of the first, which `claude --session-id` would refuse as taken.
    let again = part::process(&config, &plan, Path::new("/state")).unwrap();
    assert_eq!(again["session_id"], p["session_id"]);
    let next = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 2,
        token: "t".into(),
        actor: None,
        bead: None,
    };
    let next = part::process(&config, &next, Path::new("/state")).unwrap();
    assert_ne!(next["session_id"], p["session_id"]);
    let env = part::env(&next, "i2", None, Path::new("/state")).unwrap();
    assert!(
        !env.iter().any(|(k, _)| k == "LOOP_MANAGER_BEAD"),
        "a run without a bead has no LOOP_MANAGER_BEAD"
    );
    // A native `hooks` of a foreign shape is refused, neither a panic in the core's writer nor a silent overwrite.
    for native in [json!({ "hooks": "x" }), json!({ "hooks": { "Stop": {} } })] {
        config.native = Some(agent(native));
        let err = part::process(&config, &plan, Path::new("/state")).unwrap_err();
        assert_eq!(err.kind, FailureKind::ConfigError, "{}", err.raw);
    }
}

#[test]
fn an_isolated_session_without_a_credential_gets_the_host_login() {
    let host = tempfile::tempdir().unwrap();
    let login = json!({ "claudeAiOauth": { "accessToken": "sk-ant-oat-host" } });
    std::fs::write(host.path().join(".credentials.json"), login.to_string()).unwrap();
    let token = |p: &Value| {
        let env = part::env(p, "i1", None, host.path()).unwrap();
        env.into_iter()
            .find(|(k, _)| k == "CLAUDE_CODE_OAUTH_TOKEN")
            .map(|(_, v)| v)
    };
    let mut p = json!({ "config_dir": "/state/claude/s1/config", "env": {} });
    assert_eq!(token(&p).as_deref(), Some("sk-ant-oat-host"));
    let isolated = part::account(&p);
    let vars = |p: &Value| part::daemon_vars(p).collect::<Vec<_>>();
    assert!(!vars(&p).contains(&"HTTPS_PROXY") && !vars(&p).contains(&"CLAUDE_CONFIG_DIR"));
    p["config_dir"] = Value::Null;
    for var in [
        "HTTPS_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "SSL_CERT_FILE",
        "CLAUDE_CONFIG_DIR",
    ] {
        assert!(vars(&p).contains(&var), "an inheriting session gets {var}");
    }
    assert_eq!(token(&p), None, "an inheriting session reads the login itself");
    assert_eq!(part::account(&p), isolated, "either way it is the host's account");
    p["config_dir"] = json!("/state/claude/s1/config");
    p["env"]["ANTHROPIC_API_KEY"] = json!("k");
    assert_eq!(token(&p), None, "its own credential wins");
    assert_ne!(part::account(&p), isolated);
    let env = part::env(&p, "i1", None, host.path()).unwrap();
    assert!(
        !env.iter().any(|(k, _)| k == "BD_ACTOR"),
        "a plan without a tracker actor sets none"
    );
}

/// A process that dies on its own in a turn: its stderr is in the failure and the driver forgets it.
#[tokio::test]
async fn a_process_that_dies_in_a_turn_reports_its_stderr_and_is_forgotten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("claude");
    let init = r#"{"type":"system","subtype":"init"}"#;
    std::fs::write(
        &script,
        // It dies only once started: dead before `start` reads its start time, it would fail the start instead.
        format!("#!/bin/sh\necho '{init}'\nwhile [ ! -e go ]; do sleep 0.01; done\necho 'Error: boom' >&2\nexit 1\n"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let driver = Claude::new(script, dir.path().join("state"), None);
    let mut p = process(false, true);
    p["workdir"] = json!(dir.path());
    p["files"] = json!(dir.path().join("state/claude/s1"));
    p["mcp"] = json!({ "mcpServers": { "svc": { "type": "http", "url": "http://x", "headers": { "a": "secret" } } } });
    let (sink, mut events) = tokio::sync::mpsc::unbounded_channel();
    let instance = driver.start("i1", &p, sink).await.unwrap();
    let mcp = dir.path().join("state/claude/s1/i1/mcp.json");
    assert!(mcp.is_file(), "written for the process");
    let session = driver
        .start_session(&instance, &SessionPart(Value::Null))
        .await
        .unwrap();
    std::fs::write(dir.path().join("go"), "").unwrap();
    let mut failure = None;
    while let Some(event) = events.recv().await {
        match event.event {
            Normalized::Failure(f) => failure = Some(f),
            Normalized::Exited { .. } => break,
            _ => {}
        }
    }
    let failure = failure.expect("a failure in the open turn");
    assert!(
        !mcp.parent().unwrap().exists(),
        "the generated files, secrets included, go with the process"
    );
    assert_eq!(failure.kind, FailureKind::HarnessCrash);
    assert!(
        failure.raw.ends_with("exited with code 1 in a turn: Error: boom"),
        "{}",
        failure.raw
    );
    assert!(driver.live(&session).is_none());
    assert!(
        driver
            .start_session(&instance, &SessionPart(Value::Null))
            .await
            .is_err()
    );
}

/// A request of claude's that nothing in loop-manager answers is refused at once rather than holding the turn.
#[tokio::test]
async fn a_control_request_gets_an_error_at_once() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("claude");
    let request = r#"{"type":"control_request","request_id":"r1","request":{"subtype":"elicitation"}}"#;
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho '{request}'\nread line\necho \"$line\" > answer\n"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let driver = Claude::new(script, dir.path().join("state"), None);
    let mut p = process(false, true);
    p["workdir"] = json!(dir.path());
    p["files"] = json!(dir.path().join("state/claude/s1"));
    let (sink, mut events) = tokio::sync::mpsc::unbounded_channel();
    driver.start("i1", &p, sink).await.unwrap();
    // Unanswered, the script waits in `read` for ever.
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), events.recv())
        .await
        .expect("the request got no answer")
        .unwrap();
    assert!(matches!(first.event, Normalized::Exited { .. }), "{:?}", first.event);
    let answer: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("answer")).unwrap()).unwrap();
    assert_eq!(answer["response"]["subtype"], "error");
    assert_eq!(answer["response"]["request_id"], "r1");
}

/// Recovery stops an instance a dead daemon started: there is no stdin to close, so no grace is spent waiting on it.
#[tokio::test]
async fn an_instance_of_a_dead_daemon_is_signalled_at_once() {
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = i32::try_from(child.id()).unwrap();
    let handle = InstanceHandle {
        id: "i-dead".into(),
        pid,
        start_time: orphans::start_time(pid).unwrap(),
        pgid: pid,
    };
    let reaper = std::thread::spawn(move || child.wait());
    let driver = Claude::new("claude".into(), "/state".into(), None);
    let at = std::time::Instant::now();
    driver.stop_instance(&handle, Duration::from_secs(10)).await;
    assert!(at.elapsed() < Duration::from_secs(2), "{:?}", at.elapsed());
    assert!(!reaper.join().unwrap().unwrap().success());
}

#[test]
fn the_process_part_keeps_the_reference_and_the_process_gets_the_value() {
    let mut config = config(None);
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
        actor: None,
        bead: None,
    };
    let key = |config: &mut EffectiveConfig, value: &str| {
        config.env.insert("ANTHROPIC_API_KEY".into(), agent(value.into()));
        part::process(config, &plan, Path::new("/state")).unwrap()
    };
    let p = key(&mut config, "${CARGO_PKG_NAME}");
    assert_eq!(
        p["env"]["ANTHROPIC_API_KEY"], "${CARGO_PKG_NAME}",
        "the fingerprint and the account too"
    );
    let env = part::env(&p, "i1", None, Path::new("/state")).unwrap();
    assert!(
        env.contains(&("ANTHROPIC_API_KEY".to_owned(), "loop-manager-claude".to_owned())),
        "{env:?}"
    );
    let p = key(&mut config, "${LOOP_MANAGER_TEST_NO_SUCH_VARIABLE}");
    let failure = part::env(&p, "i1", None, Path::new("/state")).unwrap_err();
    assert_eq!(failure.kind, FailureKind::ConfigError, "{}", failure.raw);
}
