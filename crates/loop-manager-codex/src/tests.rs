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

/// A fake app-server that answers `notLoaded` to every unsubscribe and refuses a resume as a closing thread: the
/// first one only, or every one when `always`. Each resume adds a line to the file `resumes` in `dir`.
fn refusing_codex(dir: &std::path::Path, always: bool) -> Codex {
    use std::os::unix::fs::PermissionsExt;
    let (script, resumes, closed) = (dir.join("codex"), dir.join("resumes"), dir.join("closed"));
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
      else {close}echo "{{\"id\":$id,\"error\":"'{refusal}'}}; fi ;;
    '{{"id":'*) echo "{{\"id\":$id,\"result\":{{}}}}" ;;
  esac
done
"#,
            resumes = resumes.display(),
            closed = closed.display(),
            close = if always {
                String::new()
            } else {
                format!("touch {}; ", closed.display())
            },
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    Codex::new(script, dir.join("state"), None, 8)
}

/// Resumes thread `t1` on a fresh instance of `driver`, then stops the instance.
async fn resume(driver: &Codex, dir: &std::path::Path) -> Result<(), Failure> {
    let plan = SessionPlan::New {
        workdir: dir.into(),
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
    let resumed = driver.resume_session(&instance, &session, &split.session).await;
    driver.stop_instance(&instance, Duration::from_secs(2)).await;
    resumed
}

/// app-server may answer `notLoaded` to the unsubscribe while the thread is still closing and refuse the resume that
/// follows with "retry thread/resume after the thread is closed": the resume is retried and the thread loads.
#[tokio::test]
async fn a_resume_refused_while_the_thread_closes_is_retried() {
    let dir = tempfile::tempdir().unwrap();
    let driver = refusing_codex(dir.path(), false);
    resume(&driver, dir.path()).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("resumes")).unwrap(),
        "resume\nresume\n"
    );
}

/// A thread that never finishes closing does not hold the resume forever: once the unload wait is over, the refusal
/// is the answer.
#[tokio::test]
async fn a_resume_refused_past_the_unload_wait_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = refusing_codex(dir.path(), true);
    driver.unload_wait = Duration::from_millis(300);
    let failure = tokio::time::timeout(Duration::from_secs(5), resume(&driver, dir.path()))
        .await
        .expect("the resume retried past the unload wait")
        .unwrap_err();
    assert!(failure.raw.contains("retry thread/resume"), "{}", failure.raw);
    let resumes = std::fs::read_to_string(dir.path().join("resumes")).unwrap();
    assert!(resumes.lines().count() > 1, "{resumes}");
}
