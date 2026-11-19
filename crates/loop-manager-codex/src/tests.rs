use super::*;

pub(crate) fn agent<T>(value: T) -> loop_manager_contract::config::Sourced<T> {
    loop_manager_contract::config::Sourced {
        value,
        from: loop_manager_contract::config::Layer::Agent,
    }
}

pub(crate) fn config() -> EffectiveConfig {
    EffectiveConfig {
        services: Default::default(),
        harness: "codex".into(),
        harness_from: loop_manager_contract::config::Layer::Agent,
        scope: Default::default(),
        description: None,
        subagents: None,
        targets: Default::default(),
        model_given: None,
        model: Some(agent("gpt-fake".into())),
        effort: Some(agent("low".into())),
        system_prompt: None,
        inherit_host: Some(agent(false)),
        env: [("OPENAI_API_KEY".to_owned(), agent("sk-fake".to_owned()))].into(),
        mcp: Default::default(),
        hooks: Default::default(),
        skills: Default::default(),
        addons: Vec::new(),
        native: Some(agent(
            json!({ "sandbox_mode": "danger-full-access", "approval_policy": "never",
            "model_providers": { "fake": { "base_url": "http://127.0.0.1:1/v1", "wire_api": "responses" } } }),
        )),
    }
}

/// app-server may answer `notLoaded` to the unsubscribe while the thread is still closing and refuse the resume that
/// follows with "retry thread/resume after the thread is closed": the resume is retried and the thread loads.
#[tokio::test]
async fn a_resume_refused_while_the_thread_closes_is_retried() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (script, resumes, closed) = (
        dir.path().join("codex"),
        dir.path().join("resumes"),
        dir.path().join("closed"),
    );
    let refusal = r#"{"code":-32600,"message":"thread t1 is closing; retry thread/resume after the thread is closed"}"#;
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
while read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"thread/unsubscribe"'*) echo "{{\"id\":$id,\"result\":{{\"status\":\"notLoaded\"}}}}" ;;
    *'"method":"thread/resume"'*)
      echo resume >> {resumes}
      if [ -e {closed} ]; then echo "{{\"id\":$id,\"result\":{{}}}}"
      else touch {closed}; echo "{{\"id\":$id,\"error\":"'{refusal}'}}; fi ;;
    '{{"id":'*) echo "{{\"id\":$id,\"result\":{{}}}}" ;;
  esac
done
"#,
            resumes = resumes.display(),
            closed = closed.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let driver = Codex::new(script, dir.path().join("state"), None, 8);
    let plan = SessionPlan::New {
        workdir: dir.path().into(),
        run: "r".into(),
        attempt: 2,
        token: "tok".into(),
        actor: None,
        bead: None,
    };
    let split = driver.split(&config(), &plan).unwrap();
    let (sink, _events) = tokio::sync::mpsc::unbounded_channel();
    let instance = driver.start_instance("i1", &split.process, sink).await.unwrap();
    let session = rpc::session("t1", &split.process.0["home"]);
    driver
        .resume_session(&instance, &session, &split.session)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&resumes).unwrap(), "resume\nresume\n");
    driver.stop_instance(&instance, Duration::from_secs(2)).await;
}
