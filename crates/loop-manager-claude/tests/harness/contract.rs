//! The 15 contract scenarios of docs/harness-updates.md for claude: the real `claude` against the fake Anthropic API,
//! no model and no account. Each scenario pins what the adapter reports for one harness behaviour; rerun them on every
//! claude update (docs/harness-updates.md).

use loop_manager_claude::Claude;
use loop_manager_contract::{
    config::{Layer, Sourced},
    driver::{
        Driver, DriverEvent, Failure, FailureKind, InstanceHandle, Normalized, PromptKey, SessionPlan, SessionRef,
        SessionState, StopReason,
    },
};
use loop_manager_testkit::anthropic::{Provider, Reply};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

fn sourced<T>(value: T) -> Sourced<T> {
    Sourced {
        value,
        from: Layer::Agent,
    }
}

/// One claude session on the fake with one prompt sent; `retries` is `CLAUDE_CODE_MAX_RETRIES`.
struct Live {
    provider: Provider,
    driver: Claude,
    instance: InstanceHandle,
    session: SessionRef,
    events: mpsc::UnboundedReceiver<DriverEvent>,
    seen: Vec<Normalized>,
    rig: PathBuf,
}

impl Live {
    async fn start(plan: Vec<Reply>, dir: &Path, retries: u32, permission: Option<&str>) -> Self {
        let provider = Provider::start(plan).await;
        Self::open(provider, dir, retries, permission, None).await
    }

    /// `resume` binds the session of another live process: a second writer.
    async fn open(
        provider: Provider,
        dir: &Path,
        retries: u32,
        permission: Option<&str>,
        resume: Option<SessionRef>,
    ) -> Self {
        let driver = Claude::new("claude".into(), dir.join("state"), None);
        let rig = dir.join("rig");
        std::fs::create_dir_all(&rig).unwrap();
        let (run, attempt, token) = ("r1".to_owned(), 1, "t".to_owned());
        let plan = match resume.clone() {
            None => SessionPlan::New {
                workdir: rig.clone(),
                run,
                attempt,
                token,
                actor: None,
                bead: None,
            },
            Some(session) => SessionPlan::Resume {
                session,
                workdir: rig.clone(),
                run,
                attempt,
                token,
                actor: None,
                bead: None,
            },
        };
        // The config of the claude tests with this many retries and this permission mode.
        let mut config = super::claude::config(&provider);
        config
            .env
            .insert("CLAUDE_CODE_MAX_RETRIES".into(), sourced(retries.to_string()));
        config.native = permission.map(|p| sourced(serde_json::json!({ "permission_mode": p })));
        let split = driver.split(&config, &plan).unwrap();
        let (sink, events) = mpsc::unbounded_channel();
        let id = if resume.is_some() { "i2" } else { "i1" };
        let instance = driver.start_instance(id, &split.process, sink).await.unwrap();
        let session = match resume {
            None => driver.start_session(&instance, &split.session).await.unwrap(),
            Some(session) => {
                driver
                    .resume_session(&instance, &session, &split.session)
                    .await
                    .unwrap();
                session
            }
        };
        // The second writer's prompt has a key of its own.
        let prompt = if instance.id == "i2" { 2 } else { 1 };
        let key = PromptKey::new("r1", 1, prompt);
        driver
            .send_prompt(&instance, &session, "Do the task.", key)
            .await
            .unwrap();
        Self {
            provider,
            driver,
            instance,
            session,
            events,
            seen: Vec::new(),
            rig,
        }
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

    /// The one failure of a failed turn.
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
    let mut live = Live::start(vec![reply], dir.path(), retries, None).await;
    let failure = live.failed().await;
    let seen = live.seen.clone();
    live.stop().await;
    (failure, seen)
}

fn retried(seen: &[Normalized]) -> usize {
    seen.iter().filter(|e| matches!(e, Normalized::Retrying { .. })).count()
}

#[tokio::test]
async fn s01_rate_limits_then_an_answer_complete_the_turn_after_claude_retries() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::RateLimited, Reply::RateLimited, Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 3, None).await;
    assert_eq!(live.ended().await, StopReason::Completed, "{:?}", live.seen);
    assert_eq!(retried(&live.seen), 2, "{:?}", live.seen);
    assert!(live.failures().is_empty(), "{:?}", live.seen);
    assert_eq!(live.provider.requests().len(), 3);
    live.stop().await;
}

#[tokio::test]
async fn s02_endless_rate_limits_are_rate_limited_and_already_retried() {
    let (failure, seen) = failure_of(Reply::RateLimited, 1).await;
    assert_eq!(failure.kind, FailureKind::RateLimited, "{seen:?}");
    assert!(failure.retried_by_harness);
    assert_eq!(retried(&seen), 1, "{seen:?}");
}

/// With an API key claude reports no reset time (docs/harness-updates.md): a quota looks like a plain rate limit.
#[tokio::test]
async fn s03_a_quota_with_a_reset_time_is_a_rate_limit_without_a_time() {
    let (failure, seen) = failure_of(Reply::Quota, 0).await;
    assert_eq!(failure.kind, FailureKind::RateLimited, "{seen:?}");
    assert_eq!(failure.retry_at_ms, None);
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
    assert!(failure.retried_by_harness);
}

#[tokio::test]
async fn s06_a_401_is_auth_required() {
    let (failure, seen) = failure_of(Reply::Unauthorized, 0).await;
    assert_eq!(failure.kind, FailureKind::AuthRequired, "{seen:?}");
}

/// Anthropic's `insufficient_quota` is the credit balance error.
#[tokio::test]
async fn s07_no_credit_is_billing() {
    let (failure, seen) = failure_of(Reply::Billing, 0).await;
    assert_eq!(failure.kind, FailureKind::Billing, "{seen:?}");
}

#[tokio::test]
async fn s08_a_prompt_too_long_is_a_context_overflow() {
    let (failure, seen) = failure_of(Reply::ContextOverflow, 0).await;
    assert_eq!(failure.kind, FailureKind::ContextOverflow, "{seen:?}");
}

#[tokio::test]
async fn s09_a_missing_model_is_a_config_error() {
    let (failure, seen) = failure_of(Reply::NoModel, 0).await;
    assert_eq!(failure.kind, FailureKind::ConfigError, "{seen:?}");
}

/// Criterion 9 at the adapter: the crash is typed within a second of the kill.
#[tokio::test]
async fn s10_kill_9_in_a_command_is_a_harness_crash_within_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::Bash("sleep 20".into()), Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 0, Some("bypassPermissions")).await;
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

/// EOF on stdin in a turn: claude finishes the turn without an observer, then exits on its own
/// (docs/harness-updates.md, scenario 11).
#[tokio::test]
async fn s11_eof_on_stdin_in_a_turn_lets_the_turn_finish_then_exit() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::Bash("sleep 2 && touch done".into()), Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 0, Some("bypassPermissions")).await;
    let running = live.until(Duration::from_secs(30), |e| {
        *e == Normalized::Activity { tool_running: true }
    });
    assert!(running.await.is_some(), "{:?}", live.seen);
    // A long grace: only stdin closes; TERM would come after 15 s.
    let (driver, instance) = (&live.driver, live.instance.clone());
    let stop = driver.stop_instance(&instance, Duration::from_secs(30));
    let started = Instant::now();
    stop.await;
    assert!(started.elapsed() < Duration::from_secs(15), "exited by itself");
    live.until(Duration::from_secs(5), |e| matches!(e, Normalized::Exited { .. }))
        .await;
    assert!(live.rig.join("done").exists(), "the command ran to its end");
    assert!(
        live.seen.contains(&Normalized::TurnEnded {
            stop_reason: StopReason::Completed
        }),
        "{:?}",
        live.seen
    );
    assert!(live.seen.contains(&Normalized::Exited {
        code: Some(0),
        signal: None
    }));
    assert!(live.failures().is_empty(), "{:?}", live.seen);
}

/// A silent provider gives no turn end and no failure: the core's watchdog owns the stall (docs/design/invariants.md);
/// an interrupt ends the turn as cancelled.
#[tokio::test]
async fn s12_a_silent_provider_ends_nothing_until_an_interrupt_cancels_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut live = Live::start(vec![Reply::Silent], dir.path(), 0, None).await;
    let ended = live
        .until(Duration::from_secs(2), |e| matches!(e, Normalized::TurnEnded { .. }))
        .await;
    assert_eq!(ended, None);
    assert!(live.failures().is_empty(), "{:?}", live.seen);
    live.driver.cancel_turn(&live.instance, &live.session).await.unwrap();
    assert_eq!(live.ended().await, StopReason::Cancelled, "{:?}", live.seen);
    assert!(live.failures().is_empty(), "{:?}", live.seen);
    live.stop().await;
}

/// A request the config's mode does not allow is denied by claude itself (`--permission-prompts none`): nothing waits
/// for a person, the model gets the denial and the turn completes.
#[tokio::test]
async fn s13_a_request_the_config_does_not_allow_is_denied_by_the_harness_and_the_turn_completes() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![Reply::Bash("touch denied".into()), Reply::Text("OK".into())];
    let mut live = Live::start(plan, dir.path(), 0, Some("manual")).await;
    assert_eq!(live.ended().await, StopReason::Completed, "{:?}", live.seen);
    assert!(live.failures().is_empty(), "{:?}", live.seen);
    assert!(!live.rig.join("denied").exists());
    let denied: Vec<_> = live
        .seen
        .iter()
        .filter_map(|e| match e {
            Normalized::PermissionDenied { tool, tool_use_id, .. } => Some((tool.as_str(), tool_use_id.is_some())),
            _ => None,
        })
        .collect();
    assert_eq!(denied, [("Bash", true)], "{:?}", live.seen);
    live.stop().await;
}

/// The turn ends while a background command runs: idle carries `background`, and only after the command ends
/// does claude come to idle without background (criterion 4a at the adapter).
#[tokio::test]
async fn s14_a_background_command_keeps_the_session_busy_after_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let plan = vec![
        Reply::Background("sleep 3 && touch bg".into()),
        Reply::Text("Started.".into()),
        Reply::Text("Finished.".into()),
    ];
    let mut live = Live::start(plan, dir.path(), 0, Some("bypassPermissions")).await;
    assert_eq!(live.ended().await, StopReason::Completed, "{:?}", live.seen);
    assert!(!live.rig.join("bg").exists(), "the turn ended before the command");
    let idle = |background| Normalized::State {
        state: SessionState::Idle,
        background,
    };
    let first = live
        .until(Duration::from_secs(10), |e| matches!(e, Normalized::State { .. }))
        .await;
    assert_eq!(first, Some(idle(true)), "{:?}", live.seen);
    let free = live.until(Duration::from_secs(30), |e| *e == idle(false)).await;
    assert!(free.is_some(), "{:?}", live.seen);
    assert!(live.rig.join("bg").exists());
    live.stop().await;
}

/// claude does not refuse a second process on a live session: both write, so one writer per session is the
/// core's to keep (docs/design/invariants.md, invariant 8), never the harness's.
#[tokio::test]
async fn s15_a_second_writer_is_not_refused_by_claude() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = Live::start(vec![Reply::Hang], dir.path(), 0, None).await;
    let started = first
        .until(Duration::from_secs(30), |e| *e == Normalized::TurnStarted)
        .await;
    assert!(started.is_some(), "{:?}", first.seen);
    let provider = Provider::start(vec![Reply::Text("OK".into())]).await;
    let mut second = Live::open(provider, dir.path(), 0, None, Some(first.session.clone())).await;
    assert_eq!(second.ended().await, StopReason::Completed, "{:?}", second.seen);
    assert!(second.failures().is_empty(), "{:?}", second.seen);
    second.stop().await;
    first.stop().await;
}
