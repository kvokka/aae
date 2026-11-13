//! The codex driver on the real `codex app-server` (pinned in `mise.toml`) against the fake Responses API: no model.

use aae_codex::Codex;
use aae_core::{
    config::{EffectiveConfig, Layer, Skill, Sourced},
    driver::{
        Driver, DriverEvent, FailureKind, Normalized, PromptKey, PromptState, SessionPlan, StopReason, Transcript,
    },
};
use aae_testkit::openai::{Provider, Reply};
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;

fn agent<T>(value: T) -> Sourced<T> {
    Sourced {
        value,
        from: Layer::Agent,
    }
}

fn config(provider: &Provider) -> EffectiveConfig {
    EffectiveConfig {
        harness: "codex".into(),
        model: Some(agent("gpt-fake".into())),
        effort: None,
        system_prompt: None,
        permission_mode: Some(agent("never".into())),
        inherit_host: Some(agent(false)),
        env: Default::default(),
        mcp: Default::default(),
        hooks: Default::default(),
        skills: Default::default(),
        plugins: Vec::new(),
        // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
        native: Some(agent(
            json!({ "model_provider": "fake", "sandbox_mode": "danger-full-access",
            "model_providers": { "fake": { "name": "fake", "base_url": provider.base_url(), "wire_api": "responses" } } }),
        )),
    }
}

/// Events until one matches, at most 30 s.
async fn until(rx: &mut mpsc::UnboundedReceiver<DriverEvent>, done: impl Fn(&Normalized) -> bool) -> Vec<Normalized> {
    let mut seen = Vec::new();
    while let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
        let stop = done(&e.event);
        seen.push(e.event);
        if stop {
            return seen;
        }
    }
    panic!("no matching event in {seen:?}")
}

#[tokio::test]
async fn a_prompt_runs_a_command_in_the_thread_and_the_turn_completes() {
    let provider = Provider::start(vec![
        Reply::Command("echo \"$AAE_RUN\" > marker".into()),
        Reply::Text("DONE".into()),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let driver = Codex::new("codex".into(), dir.path().join("state"), None, 8);
    let plan = SessionPlan::New {
        workdir: work.clone(),
        run: "run-1".into(),
        attempt: 1,
        token: "tok".into(),
    };
    // A skill reaches the thread through the process's own `CODEX_HOME/skills`.
    let skill = dir.path().join("skills/mail");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: mail\ndescription: Sends the zq7-mail marker.\n---\nSend it.\n",
    )
    .unwrap();
    let mut config = config(&provider);
    let path = Some(skill.clone());
    config
        .skills
        .insert("mail".into(), agent(Skill { enabled: true, path }));
    let split = driver.split(&config, &plan).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let instance = driver.start_instance("i1", &split.process, tx).await.unwrap();
    let session = driver.start_session(&instance, &split.session).await.unwrap();
    let key = PromptKey::new("run-1", 1, 1);
    driver.send_prompt(&instance, &session, "run it", key).await.unwrap();
    let events = until(&mut rx, |e| matches!(e, Normalized::TurnEnded { .. })).await;
    assert!(events.contains(&Normalized::PromptAcked { key }), "{events:?}");
    assert!(events.contains(&Normalized::TurnStarted));
    assert!(
        events.contains(&Normalized::Activity { tool_running: true }),
        "{events:?}"
    );
    assert_eq!(
        events.last(),
        Some(&Normalized::TurnEnded {
            stop_reason: StopReason::Completed
        })
    );
    // The attempt environment reached the command through the thread's shell policy.
    assert_eq!(std::fs::read_to_string(work.join("marker")).unwrap().trim(), "run-1");
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body["model"], "gpt-fake");
    assert!(
        requests[0].body.to_string().contains("zq7-mail"),
        "the skill is listed to the model"
    );
    let answered = driver.transcript(&instance, &session, key).await.unwrap();
    assert_eq!(
        answered,
        Transcript::Present {
            prompt: PromptState::Answered
        }
    );
    let other = driver
        .transcript(&instance, &session, PromptKey::new("run-1", 1, 2))
        .await
        .unwrap();
    assert_eq!(
        other,
        Transcript::Present {
            prompt: PromptState::Absent
        }
    );
    driver.stop_instance(&instance, Duration::from_secs(4)).await;
    until(&mut rx, |e| matches!(e, Normalized::Exited { .. })).await;
}

#[tokio::test]
async fn a_usage_limit_ends_the_turn_with_a_quota_and_its_reset() {
    let provider = Provider::start(vec![Reply::UsageLimit(3600)]).await;
    let dir = tempfile::tempdir().unwrap();
    let driver = Codex::new("codex".into(), dir.path().join("state"), None, 8);
    let plan = SessionPlan::New {
        workdir: dir.path().into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
    };
    let split = driver.split(&config(&provider), &plan).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let instance = driver.start_instance("i1", &split.process, tx).await.unwrap();
    let session = driver.start_session(&instance, &split.session).await.unwrap();
    driver
        .send_prompt(&instance, &session, "hi", PromptKey::new("r", 1, 1))
        .await
        .unwrap();
    let events = until(&mut rx, |e| matches!(e, Normalized::TurnEnded { .. })).await;
    let failure = events.iter().find_map(|e| match e {
        Normalized::Failure(f) => Some(f),
        _ => None,
    });
    let failure = failure.unwrap_or_else(|| panic!("{events:?}"));
    assert_eq!(failure.kind, FailureKind::QuotaExhausted, "{}", failure.raw);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let at = failure.retry_at_ms.expect("the reset of the exhausted window");
    assert!((now + 3_500_000..now + 3_700_000).contains(&at), "{at} vs {now}");
    assert_eq!(
        events.last(),
        Some(&Normalized::TurnEnded {
            stop_reason: StopReason::Failed
        })
    );
    driver.stop_instance(&instance, Duration::from_secs(4)).await;
}
