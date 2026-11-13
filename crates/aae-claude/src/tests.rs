use super::*;

fn agent<T>(value: T) -> aae_core::config::Sourced<T> {
    aae_core::config::Sourced {
        value,
        from: aae_core::config::Layer::Agent,
    }
}

fn process(resume: bool, inherit: bool) -> Value {
    json!({ "session_id": "s1", "resume": resume, "inherit_host": inherit, "model": "claude-haiku-5-5",
        "effort": null, "system_prompt": "be brief", "permission_mode": null })
}

#[test]
fn every_flag_goes_again_on_a_resume_and_isolation_cuts_the_host() {
    let new = part::args(&process(false, true));
    let resumed = part::args(&process(true, false));
    let pair = |args: &[String], flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].clone());
    assert_eq!(pair(&new, "--session-id").as_deref(), Some("s1"));
    assert_eq!(pair(&resumed, "--resume").as_deref(), Some("s1"));
    for args in [&new, &resumed] {
        assert_eq!(pair(args, "--model").as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(pair(args, "--append-system-prompt").as_deref(), Some("be brief"));
    }
    assert_eq!(pair(&new, "--setting-sources"), None, "the host is inherited");
    assert_eq!(pair(&resumed, "--setting-sources").as_deref(), Some("project,local"));
}

#[test]
fn the_transcript_tells_where_the_prompt_stands() {
    let dir = tempfile::tempdir().unwrap();
    let session = json!({ "session_id": "s1", "config_dir": dir.path(), "workdir": "/w/my rig" });
    let key = PromptKey::new("r", 1, 1);
    assert_eq!(transcript(&session, key), Transcript::Missing);
    let file = dir.path().join("projects/-w-my-rig/s1.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let user = json!({ "type": "user", "uuid": key.0.to_string() });
    for (lines, prompt) in [
        (vec![json!({ "type": "queue-operation" })], PromptState::Absent),
        (vec![user.clone()], PromptState::Unanswered),
        (vec![user, json!({ "type": "assistant" })], PromptState::Answered),
    ] {
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(&file, text.join("\n")).unwrap();
        assert_eq!(transcript(&session, key), Transcript::Present { prompt });
    }
}

#[test]
fn a_config_the_adapter_cannot_deliver_is_a_config_error() {
    let mut config = EffectiveConfig {
        harness: "claude".into(),
        model: None,
        effort: None,
        system_prompt: None,
        permission_mode: None,
        inherit_host: None,
        env: Default::default(),
        mcp: Default::default(),
        hooks: Default::default(),
        skills: Default::default(),
        plugins: Vec::new(),
        native: None,
    };
    let driver = Claude::new("claude".into(), "/state".into(), None);
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
    };
    let one = driver.split(&config, &plan).unwrap();
    assert_eq!(
        one,
        driver.split(&config, &plan).unwrap(),
        "the session id is derived from the attempt"
    );
    config.native = Some(agent(json!("permissions")));
    assert_eq!(driver.split(&config, &plan).unwrap_err().kind, FailureKind::ConfigError);
    config.native = None;
    let shared = aae_core::config::Mcp {
        command: Some(vec!["srv".into()]),
        env: Default::default(),
        url: None,
        headers: Default::default(),
        scope: Some(aae_core::config::McpScope::Project),
    };
    config.mcp.insert("shared".into(), agent(shared));
    let split = driver.split(&config, &plan).unwrap();
    assert_eq!(
        split.process.0["mcp"]["mcpServers"]["shared"]["type"], "stdio",
        "no bridge: per session"
    );
}

#[test]
fn hooks_join_the_native_settings_and_mcp_servers_take_claude_form() {
    use aae_core::config::{Hook, Mcp};
    let hook = |event: &str, enabled| Hook {
        enabled,
        event: Some(event.into()),
        matcher: None,
        command: Some("true".into()),
        timeout: Some("90s".into()),
    };
    let mut config = EffectiveConfig {
        harness: "claude".into(),
        model: None,
        effort: None,
        system_prompt: None,
        permission_mode: None,
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
        plugins: Vec::new(),
        native: Some(agent(
            json!({ "hooks": { "Stop": [{ "hooks": [] }] }, "env": { "X": "1" } }),
        )),
    };
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
    };
    let p = part::process(&config, &plan, Path::new("/state")).unwrap();
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
    };
    let next = part::process(&config, &next, Path::new("/state")).unwrap();
    assert_ne!(next["session_id"], p["session_id"]);
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
        let env = part::env(p, "i1", None, host.path());
        env.into_iter()
            .find(|(k, _)| k == "CLAUDE_CODE_OAUTH_TOKEN")
            .map(|(_, v)| v)
    };
    let mut p = json!({ "config_dir": "/state/claude/s1/config", "env": {} });
    assert_eq!(token(&p).as_deref(), Some("sk-ant-oat-host"));
    let isolated = part::account(&p);
    p["config_dir"] = Value::Null;
    assert_eq!(token(&p), None, "an inheriting session reads the login itself");
    assert_eq!(part::account(&p), isolated, "either way it is the host's account");
    p["config_dir"] = json!("/state/claude/s1/config");
    p["env"]["ANTHROPIC_API_KEY"] = json!("k");
    assert_eq!(token(&p), None, "its own credential wins");
    assert_ne!(part::account(&p), isolated);
    p["run"] = json!("run_7");
    p["attempt"] = json!(2);
    let env = part::env(&p, "i1", None, host.path());
    assert!(
        env.contains(&("BD_ACTOR".to_owned(), "aae:run_7:2".to_owned())),
        "the attempt's tracker actor"
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
    let (sink, mut events) = tokio::sync::mpsc::unbounded_channel();
    let instance = driver.start("i1", &p, sink).await.unwrap();
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
