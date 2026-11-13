//! The claude adapter driving the real `claude` against the fake Anthropic API (spec §7.8): no model, no account.

use aae_claude::Claude;
use aae_core::{
    config::{EffectiveConfig, Hook, Layer, Mcp, Skill, Sourced},
    driver::{Driver, DriverEvent, Normalized, PromptKey, PromptState, SessionPlan, StopReason, Transcript},
};
use aae_testkit::anthropic::{Provider, Reply};
use serde_json::Value;
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

/// One turn; `edit` changes the config first. The events, the transcript, the raw messages of the turn and the
/// bodies of the model requests.
async fn turn(
    plan: Vec<Reply>,
    state: &Path,
    edit: impl FnOnce(&mut EffectiveConfig),
) -> (Vec<Normalized>, Transcript, Vec<Value>, Vec<Value>) {
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
    let mut config = config(&provider);
    edit(&mut config);
    let split = driver.split(&config, &plan).unwrap();
    let (sink, mut events) = mpsc::unbounded_channel();
    let instance = driver.start_instance("i1", &split.process, sink).await.unwrap();
    let session = driver.start_session(&instance, &split.session).await.unwrap();
    let mut live = driver.live(&session).unwrap();
    let key = PromptKey::new("r1", 1, 1);
    driver
        .send_prompt(&instance, &session, "Reply with exactly: OK", key)
        .await
        .unwrap();
    let seen = until(&mut events, |e| matches!(e, Normalized::TurnEnded { .. })).await;
    let transcript = driver.transcript(&instance, &session, key).await.unwrap();
    let raw = std::iter::from_fn(|| live.try_recv().ok()).collect();
    driver.stop_instance(&instance, Duration::from_secs(4)).await;
    until(&mut events, |e| matches!(e, Normalized::Exited { .. })).await;
    let requests = provider.requests().into_iter().map(|r| r.body).collect();
    (seen, transcript, raw, requests)
}

#[tokio::test]
async fn a_reply_is_acked_and_completed_and_is_in_the_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let (seen, transcript, ..) = turn(vec![Reply::Text("OK".into())], dir.path(), |_| {}).await;
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
async fn hooks_skills_and_mcp_servers_reach_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let skill = dir.path().join("skills/mail");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: mail\ndescription: Sends mail.\n---\n\nSay mail.\n",
    )
    .unwrap();
    let marker = dir.path().join("hook-fired");
    let (_, _, raw, _) = turn(vec![Reply::Text("OK".into())], dir.path(), |c| {
        let hook = Hook {
            enabled: true,
            event: Some("UserPromptSubmit".into()),
            matcher: None,
            command: Some(format!("touch {}", marker.display())),
            timeout: Some("10s".into()),
        };
        c.hooks.insert("mark".into(), sourced(hook));
        let path = Some(skill.clone());
        c.skills.insert("mail".into(), sourced(Skill { enabled: true, path }));
        let server = Mcp {
            command: Some(vec!["true".into()]),
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
            scope: None,
        };
        c.mcp.insert("mine".into(), sourced(server));
    })
    .await;
    assert!(marker.exists(), "the hook fired");
    let init = raw
        .iter()
        .find(|m| m["type"] == "system" && m["subtype"] == "init")
        .expect("an init message");
    let skills = init["skills"].to_string();
    assert!(skills.contains("\"aae:mail\""), "{skills}");
    let servers = init["mcp_servers"].to_string();
    assert!(servers.contains("\"mine\""), "{servers}");
}

fn sourced<T>(value: T) -> Sourced<T> {
    Sourced {
        value,
        from: Layer::Agent,
    }
}

/// Host inheritance off: the rig's `CLAUDE.md` reaches the model, the host's personal one does not (spec §6.4).
#[tokio::test]
async fn an_isolated_session_reads_the_rig_but_not_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "HOST-MARKER-7\n").unwrap();
    std::fs::create_dir_all(dir.path().join("rig")).unwrap();
    std::fs::write(dir.path().join("rig/CLAUDE.md"), "RIG-MARKER-7\n").unwrap();
    let (.., requests) = turn(vec![Reply::Text("OK".into())], dir.path(), |c| {
        let home = home.to_string_lossy().into_owned();
        c.env.insert("HOME".into(), sourced(home));
    })
    .await;
    let sent = Value::from(requests).to_string();
    assert!(sent.contains("RIG-MARKER-7"), "the rig's CLAUDE.md is read");
    assert!(!sent.contains("HOST-MARKER-7"), "the host's is not");
}
