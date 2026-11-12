//! The claude adapter driving the real `claude` against the fake Anthropic API (spec §7.8): no model, no account.

use aae_claude::Claude;
use aae_core::{
    config::{EffectiveConfig, Layer, Sourced},
    driver::{
        Driver, DriverEvent, Failure, FailureKind, Normalized, PromptKey, PromptState, SessionPlan, StopReason,
        Transcript,
    },
};
use aae_testkit::anthropic::{Provider, Reply};
use std::{collections::BTreeMap, path::Path, time::Duration};
use tokio::sync::mpsc;

fn config(provider: &Provider) -> EffectiveConfig {
    let sourced = |v: &str| Sourced {
        value: v.to_owned(),
        from: Layer::Agent,
    };
    let env = [
        ("ANTHROPIC_BASE_URL", provider.base_url()),
        ("ANTHROPIC_API_KEY", "fake".to_owned()),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".to_owned()),
        ("CLAUDE_CODE_MAX_RETRIES", "0".to_owned()),
    ];
    EffectiveConfig {
        harness: "claude".into(),
        model: Some(sourced("claude-haiku-5-5")),
        effort: None,
        system_prompt: None,
        permission_mode: None,
        inherit_host: Some(Sourced {
            value: false,
            from: Layer::Agent,
        }),
        env: env.into_iter().map(|(k, v)| (k.to_owned(), sourced(&v))).collect(),
        mcp: BTreeMap::new(),
        hooks: BTreeMap::new(),
        skills: BTreeMap::new(),
        plugins: Vec::new(),
        native: None,
    }
}

/// The events of the instance until `until` holds for one, or 30 s pass.
async fn until(
    events: &mut mpsc::UnboundedReceiver<DriverEvent>,
    until: impl Fn(&Normalized) -> bool,
) -> Vec<Normalized> {
    let mut seen = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(30), events.recv()).await {
        let done = until(&event.event);
        seen.push(event.event);
        if done {
            return seen;
        }
    }
    panic!("no expected event in {seen:?}");
}

async fn turn(plan: Vec<Reply>, state: &Path) -> (Vec<Normalized>, Transcript) {
    let provider = Provider::start(plan).await;
    let driver = Claude::new("claude".into(), state.join("state"), None);
    let workdir = state.join("rig");
    std::fs::create_dir_all(&workdir).unwrap();
    let plan = SessionPlan::New {
        workdir,
        run: "r1".into(),
        attempt: 1,
        token: "t".into(),
    };
    let split = driver.split(&config(&provider), &plan).unwrap();
    let (sink, mut events) = mpsc::unbounded_channel();
    let instance = driver.start_instance("i1", &split.process, sink).await.unwrap();
    let session = driver.start_session(&instance, &split.session).await.unwrap();
    let key = PromptKey::new("r1", 1, 1);
    driver
        .send_prompt(&instance, &session, "Reply with exactly: OK", key)
        .await
        .unwrap();
    let seen = until(&mut events, |e| matches!(e, Normalized::TurnEnded { .. })).await;
    let transcript = driver.transcript(&instance, &session, key).await.unwrap();
    driver.stop_instance(&instance, Duration::from_secs(4)).await;
    until(&mut events, |e| matches!(e, Normalized::Exited { .. })).await;
    (seen, transcript)
}

#[tokio::test]
async fn a_reply_is_acked_and_completed_and_is_in_the_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let (seen, transcript) = turn(vec![Reply::Text("OK".into())], dir.path()).await;
    let key = PromptKey::new("r1", 1, 1);
    assert!(seen.contains(&Normalized::PromptAcked { key }), "{seen:?}");
    assert!(seen.contains(&Normalized::TurnStarted));
    assert_eq!(
        seen.last(),
        Some(&Normalized::TurnEnded {
            stop_reason: StopReason::Completed
        })
    );
    assert_eq!(
        transcript,
        Transcript::Present {
            prompt: PromptState::Answered
        }
    );
}

#[tokio::test]
async fn an_api_error_is_a_classified_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (seen, _) = turn(vec![Reply::Billing], dir.path()).await;
    let failure: Vec<&Failure> = seen
        .iter()
        .filter_map(|e| match e {
            Normalized::Failure(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(failure.first().map(|f| f.kind), Some(FailureKind::Billing), "{seen:?}");
    assert_eq!(
        seen.last(),
        Some(&Normalized::TurnEnded {
            stop_reason: StopReason::Failed
        })
    );
}
