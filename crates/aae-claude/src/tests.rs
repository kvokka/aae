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

/// A process that dies on its own in a turn: its stderr is in the failure and the driver forgets it.
#[tokio::test]
async fn a_process_that_dies_in_a_turn_reports_its_stderr_and_is_forgotten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("claude");
    let init = r#"{"type":"system","subtype":"init"}"#;
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho '{init}'\necho 'Error: boom' >&2\nexit 1\n"),
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
