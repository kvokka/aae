//! The 15 contract scenarios of spec §7.8 for codex: the real `codex app-server` against the fake Responses API,
//! no model and no account. Each scenario pins what the adapter reports for one harness behaviour; rerun them on
//! every codex update (docs/harness-updates.md).

use aae_codex::Codex;
use aae_core::{
    config::{EffectiveConfig, Layer, Sourced},
    driver::{
        Answer, Driver, DriverEvent, Failure, FailureKind, InstanceHandle, Normalized, PromptKey, SessionPlan,
        SessionRef, SessionState, StopReason,
    },
};
use aae_testkit::openai::{Provider, Reply};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

fn sourced<T>(value: T) -> Sourced<T> {
    Sourced {
        value,
        from: Layer::Agent,
    }
}

/// One codex thread on the fake with one prompt sent; `retries` is the provider's `request_max_retries`,
/// `approval` codex's approval policy.
struct Live {
    provider: Arc<Provider>,
    driver: Codex,
    instance: InstanceHandle,
    session: SessionRef,
    events: mpsc::UnboundedReceiver<DriverEvent>,
    seen: Vec<Normalized>,
    rig: PathBuf,
}

impl Live {
    async fn start(plan: Vec<Reply>, dir: &Path, retries: u32, approval: &str) -> Self {
        let provider = Arc::new(Provider::start(plan).await);
        Self::open(provider, dir, retries, approval, None).await.unwrap()
    }

    /// `resume` binds the thread of another live app-server: a second writer.
    /// The same provider and state make the same `CODEX_HOME`, where the thread is.
    async fn open(
        provider: Arc<Provider>,
        dir: &Path,
        retries: u32,
        approval: &str,
        resume: Option<SessionRef>,
    ) -> Result<Self, Failure> {
        let id = if resume.is_some() { "i2" } else { "i1" };
        let driver = Codex::new("codex".into(), dir.join("state"), None, 8);
        let rig = dir.join("rig");
        std::fs::create_dir_all(&rig).unwrap();
        let (run, attempt, token) = ("r1".to_owned(), 1, "t".to_owned());
        let plan = match resume.clone() {
            None => SessionPlan::New {
                workdir: rig.clone(),
                run,
                attempt,
                token,
            },
            Some(session) => SessionPlan::Resume {
                session,
                workdir: rig.clone(),
                run,
                attempt,
                token,
            },
        };
        let provider_config = json!({ "name": "fake", "base_url": provider.base_url(), "wire_api": "responses",
            "request_max_retries": retries, "stream_max_retries": retries });
        let config = EffectiveConfig {
            harness: "codex".into(),
            model: Some(sourced("gpt-fake".to_owned())),
            effort: None,
            system_prompt: None,
            permission_mode: Some(sourced(approval.to_owned())),
            inherit_host: Some(sourced(false)),
            env: Default::default(),
            mcp: Default::default(),
            hooks: Default::default(),
            skills: Default::default(),
            plugins: Vec::new(),
            // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
            native: Some(sourced(
                json!({ "model_provider": "fake", "sandbox_mode": "danger-full-access",
                "model_providers": { "fake": provider_config } }),
            )),
        };
        let split = driver.split(&config, &plan).unwrap();
        let (sink, events) = mpsc::unbounded_channel();
        let instance = driver.start_instance(id, &split.process, sink).await.unwrap();
        let session = match resume {
            None => driver.start_session(&instance, &split.session).await.unwrap(),
            Some(session) => {
                if let Err(refused) = driver.resume_session(&instance, &session, &split.session).await {
                    driver.stop_instance(&instance, Duration::from_secs(4)).await;
                    return Err(refused);
                }
                session
            }
        };
        let prompt = if id == "i2" { 2 } else { 1 };
        let key = PromptKey::new("r1", 1, prompt);
        driver
            .send_prompt(&instance, &session, "Do the task.", key)
            .await
            .unwrap();
        Ok(Self {
            provider,
            driver,
            instance,
            session,
            events,
            seen: Vec::new(),
            rig,
        })
    }

    /// Events until `until` holds for one, or `wait` passes; the matching event, if any.
    async fn until(&mut self, wait: Duration, until: impl Fn(&Normalized) -> bool) -> Option<Normalized> {
        let deadline = tokio::time::Instant::now() + wait;
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, self.events.recv()).await {
            self.seen.push(event.event.clone());
            if until(&event.event) {
                return Some(event.event);
            }
        }
        None
    }

    async fn ended(&mut self) -> StopReason {
        match self
            .until(Duration::from_secs(60), |e| matches!(e, Normalized::TurnEnded { .. }))
            .await
        {
            Some(Normalized::TurnEnded { stop_reason }) => stop_reason,
            _ => panic!("no turn end in {:?}", self.seen),
        }
    }

    fn failures(&self) -> Vec<&Failure> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                Normalized::Failure(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    /// The first failure of a failed turn.
    async fn failed(&mut self) -> Failure {
        assert_eq!(self.ended().await, StopReason::Failed, "{:?}", self.seen);
        let failures = self.failures();
        assert!(!failures.is_empty(), "{:?}", self.seen);
        failures[0].clone()
    }

    async fn stop(mut self) {
        self.driver.stop_instance(&self.instance, Duration::from_secs(4)).await;
        self.until(Duration::from_secs(10), |e| matches!(e, Normalized::Exited { .. }))
            .await;
    }
}

async fn failure_of(reply: Reply, retries: u32) -> (Failure, Vec<Normalized>) {
    let dir = tempfile::tempdir().unwrap();
    let mut live = Live::start(vec![reply], dir.path(), retries, "never").await;
    let failure = live.failed().await;
    let seen = live.seen.clone();
    live.stop().await;
    (failure, seen)
}

#[tokio::test]
async fn s01_rate_limits_then_an_answer_complete_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::RateLimited, Reply::RateLimited, Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 3, "never").await;
    assert_eq!(live.ended().await, StopReason::Completed, "{:?}", live.seen);
    assert!(live.failures().is_empty(), "{:?}", live.seen);
    assert_eq!(live.provider.requests().len(), 3);
    live.stop().await;
}

#[tokio::test]
async fn s02_endless_rate_limits_are_rate_limited() {
    let (failure, seen) = failure_of(Reply::RateLimited, 1).await;
    assert_eq!(failure.kind, FailureKind::RateLimited, "{seen:?}");
}

#[tokio::test]
async fn s03_a_usage_limit_is_a_quota_with_its_reset_time() {
    let (failure, seen) = failure_of(Reply::UsageLimit(3600), 0).await;
    assert_eq!(failure.kind, FailureKind::QuotaExhausted, "{seen:?}");
    assert!(failure.retry_at_ms.is_some(), "{}", failure.raw);
}

#[tokio::test]
async fn s04_a_quota_in_unknown_words_is_a_rate_limit_by_its_status() {
    let (failure, seen) = failure_of(Reply::StrangeQuota, 0).await;
    assert_eq!(failure.kind, FailureKind::RateLimited, "{seen:?}");
}

#[tokio::test]
async fn s05_an_overload_is_overloaded() {
    let (failure, seen) = failure_of(Reply::Overloaded, 0).await;
    assert_eq!(failure.kind, FailureKind::Overloaded, "{seen:?}");
}

#[tokio::test]
async fn s06_a_401_is_auth_required() {
    let (failure, seen) = failure_of(Reply::Unauthorized, 0).await;
    assert_eq!(failure.kind, FailureKind::AuthRequired, "{seen:?}");
}

#[tokio::test]
async fn s07_insufficient_quota_is_billing() {
    let (failure, seen) = failure_of(Reply::InsufficientQuota, 0).await;
    assert_eq!(failure.kind, FailureKind::Billing, "{seen:?}");
}

#[tokio::test]
async fn s08_a_context_too_long_is_a_context_overflow() {
    let (failure, seen) = failure_of(Reply::ContextOverflow, 0).await;
    assert_eq!(failure.kind, FailureKind::ContextOverflow, "{seen:?}");
}

#[tokio::test]
async fn s09_a_missing_model_is_a_config_error() {
    let (failure, seen) = failure_of(Reply::NoModel, 0).await;
    assert_eq!(failure.kind, FailureKind::ConfigError, "{seen:?}");
}

/// Criterion 9 at the adapter: the crash of the app-server is typed within a second of the kill.
#[tokio::test]
async fn s10_kill_9_in_a_command_is_a_harness_crash_within_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::Command("sleep 20".into()), Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 0, "never").await;
    let running = live.until(Duration::from_secs(30), |e| {
        *e == Normalized::Activity { tool_running: true }
    });
    assert!(running.await.is_some(), "{:?}", live.seen);
    let killed = Instant::now();
    let pid = rustix::process::Pid::from_raw(live.instance.pid).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::KILL).unwrap();
    let exited = live
        .until(Duration::from_secs(5), |e| matches!(e, Normalized::Exited { .. }))
        .await;
    assert!(killed.elapsed() <= Duration::from_secs(1), "{:?}", killed.elapsed());
    assert_eq!(
        exited,
        Some(Normalized::Exited {
            code: None,
            signal: Some(9)
        })
    );
    let kinds: Vec<FailureKind> = live.failures().iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FailureKind::HarnessCrash], "{:?}", live.seen);
}

/// EOF on stdin in a turn: the app-server exits at once with code 0, the turn left open; the adapter closes the
/// "exit 0 without a turn end" trap (spec §7.2) as a crash.
#[tokio::test]
async fn s11_eof_on_stdin_in_a_turn_is_a_harness_crash() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::Command("sleep 20".into()), Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 0, "never").await;
    let running = live.until(Duration::from_secs(30), |e| {
        *e == Normalized::Activity { tool_running: true }
    });
    assert!(running.await.is_some(), "{:?}", live.seen);
    let instance = live.instance.clone();
    let started = Instant::now();
    // A long grace: only stdin closes; TERM would come after 15 s.
    live.driver.stop_instance(&instance, Duration::from_secs(30)).await;
    assert!(started.elapsed() < Duration::from_secs(15), "exited by itself");
    live.until(Duration::from_secs(5), |e| matches!(e, Normalized::Exited { .. }))
        .await;
    assert!(
        live.seen.contains(&Normalized::Exited {
            code: Some(0),
            signal: None
        }),
        "{:?}",
        live.seen
    );
    let kinds: Vec<FailureKind> = live.failures().iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FailureKind::HarnessCrash], "{:?}", live.seen);
}

/// A silent provider gives no turn end and no failure: the core's watchdog owns the stall (spec §5.5); an
/// interrupt ends the turn as cancelled.
#[tokio::test]
async fn s12_a_silent_provider_ends_nothing_until_an_interrupt_cancels_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut live = Live::start(vec![Reply::Silent], dir.path(), 0, "never").await;
    let ended = live
        .until(Duration::from_secs(5), |e| matches!(e, Normalized::TurnEnded { .. }))
        .await;
    assert_eq!(ended, None);
    assert!(live.failures().is_empty(), "{:?}", live.seen);
    live.driver.cancel_turn(&live.instance, &live.session).await.unwrap();
    assert_eq!(live.ended().await, StopReason::Cancelled, "{:?}", live.seen);
    assert!(live.failures().is_empty(), "{:?}", live.seen);
    live.stop().await;
}

#[tokio::test]
async fn s13_a_permission_request_is_answered_and_the_command_runs() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::Command("touch allowed".into()), Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 0, "untrusted").await;
    let asked = live
        .until(Duration::from_secs(30), |e| {
            matches!(e, Normalized::PermissionRequested { .. })
        })
        .await;
    let Some(Normalized::PermissionRequested { request_id, .. }) = asked else {
        panic!("no permission request in {:?}", live.seen)
    };
    let answer = Answer::Allow { updated_input: None };
    live.driver
        .respond(&live.instance, &live.session, &request_id, &answer)
        .await
        .unwrap();
    assert_eq!(live.ended().await, StopReason::Completed, "{:?}", live.seen);
    assert!(live.rig.join("allowed").exists());
    live.stop().await;
}

/// codex has no background tasks a session waits for: the turn ends on `turn/completed` with the session idle
/// without background, and no turn follows on its own.
#[tokio::test]
async fn s14_a_command_sent_to_the_background_ends_nothing_later() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![
        Reply::Command("(sleep 3 && touch bg) > /dev/null 2>&1 &".into()),
        Reply::Text("Started.".into()),
    ];
    let mut live = Live::start(plan, dir.path(), 0, "never").await;
    assert_eq!(live.ended().await, StopReason::Completed, "{:?}", live.seen);
    let idle = Normalized::State {
        state: SessionState::Idle,
        background: false,
    };
    let after = live.until(Duration::from_secs(5), |e| *e == idle).await;
    assert_eq!(after, Some(idle), "{:?}", live.seen);
    let more = live
        .until(Duration::from_secs(5), |e| *e == Normalized::TurnStarted)
        .await;
    assert_eq!(more, None, "{:?}", live.seen);
    assert_eq!(live.provider.requests().len(), 2);
    live.stop().await;
}

/// codex refuses a second app-server on a live thread: the resume is a transcript divergence (contracts 4.6).
#[tokio::test]
async fn s15_a_second_writer_is_refused_as_a_divergence() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = Live::start(vec![Reply::Hang], dir.path(), 0, "never").await;
    let started = first
        .until(Duration::from_secs(30), |e| *e == Normalized::TurnStarted)
        .await;
    assert!(started.is_some(), "{:?}", first.seen);
    let provider = first.provider.clone();
    let second = Live::open(provider, dir.path(), 0, "never", Some(first.session.clone())).await;
    let refused = second.err().expect("the second writer is refused");
    assert_eq!(refused.kind, FailureKind::TranscriptDivergence, "{}", refused.raw);
    assert!(refused.raw.contains("active writer"), "{}", refused.raw);
    first.stop().await;
}
