use super::*;

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
    config.native = Some(aae_core::config::Sourced {
        value: json!({}),
        from: aae_core::config::Layer::Agent,
    });
    assert_eq!(driver.split(&config, &plan).unwrap_err().kind, FailureKind::ConfigError);
}
