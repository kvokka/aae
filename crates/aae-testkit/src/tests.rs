use super::*;
use aae_core::driver::{RequestKind, SessionState};
use tokio::{sync::mpsc, time::Instant};

type Events = mpsc::UnboundedReceiver<DriverEvent>;

struct Setup {
    driver: ScriptedDriver,
    instance: InstanceHandle,
    session: SessionRef,
    events: Events,
}

async fn setup() -> Setup {
    let driver = ScriptedDriver::new(1);
    let (sink, mut events) = mpsc::unbounded_channel();
    let process = ProcessPart(Value::Null);
    let instance = driver.start_instance("i-1", &process, sink).await.unwrap();
    let session = driver
        .start_session(&instance, &SessionPart(Value::Null))
        .await
        .unwrap();
    assert_eq!(next(&mut events).await, Some(Normalized::SessionStarted));
    Setup {
        driver,
        instance,
        session,
        events,
    }
}

/// The next event, or `None` if nothing comes within an hour of managed time.
async fn next(events: &mut Events) -> Option<Normalized> {
    let event = tokio::time::timeout(Duration::from_secs(3600), events.recv()).await;
    event.ok().flatten().map(|e| e.event)
}

fn key(n: u32) -> PromptKey {
    PromptKey::new("run-1", 1, n)
}

fn ended(stop_reason: StopReason) -> Step {
    Step::Emit(Normalized::TurnEnded { stop_reason })
}

fn state(state: SessionState, background: bool) -> Step {
    Step::Emit(Normalized::State { state, background })
}

impl Setup {
    async fn prompt(&self, n: u32) {
        self.driver
            .send_prompt(&self.instance, &self.session, "go", key(n))
            .await
            .unwrap();
    }

    async fn transcript(&self, n: u32) -> Transcript {
        self.driver
            .transcript(&self.instance, &self.session, key(n))
            .await
            .unwrap()
    }
}

#[tokio::test(start_paused = true)]
async fn a_delayed_turn_ends_without_an_outcome_after_its_delay() {
    let mut s = setup().await;
    s.driver.script(vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Wait(Duration::from_secs(90)),
        ended(StopReason::Completed),
        state(SessionState::Idle, false),
    ]);
    let start = Instant::now();
    s.prompt(1).await;
    assert_eq!(
        s.transcript(1).await,
        Transcript::Present {
            prompt: PromptState::Unanswered
        }
    );
    assert_eq!(next(&mut s.events).await, Some(Normalized::PromptAcked { key: key(1) }));
    assert_eq!(next(&mut s.events).await, Some(Normalized::TurnStarted));
    let end = next(&mut s.events).await;
    assert_eq!(start.elapsed(), Duration::from_secs(90));
    assert_eq!(
        end,
        Some(Normalized::TurnEnded {
            stop_reason: StopReason::Completed
        })
    );
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::State {
            state: SessionState::Idle,
            background: false
        })
    );
    assert_eq!(
        s.transcript(1).await,
        Transcript::Present {
            prompt: PromptState::Answered
        }
    );
    assert_eq!(
        s.transcript(2).await,
        Transcript::Present {
            prompt: PromptState::Absent
        }
    );
}

#[tokio::test(start_paused = true)]
async fn a_provider_error_fails_the_turn() {
    let mut s = setup().await;
    let mut failure = Failure::new(FailureKind::RateLimited, Source::Harness);
    failure.retry_at_ms = Some(60_000);
    s.driver.script(vec![
        Step::Emit(Normalized::Failure(failure.clone())),
        ended(StopReason::Failed),
    ]);
    s.prompt(1).await;
    next(&mut s.events).await;
    assert_eq!(next(&mut s.events).await, Some(Normalized::Failure(failure)));
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::TurnEnded {
            stop_reason: StopReason::Failed
        })
    );
    assert_eq!(
        s.transcript(1).await,
        Transcript::Present {
            prompt: PromptState::Unanswered
        }
    );
}

#[tokio::test(start_paused = true)]
async fn a_hang_sends_nothing_after_the_ack_until_cancelled() {
    let mut s = setup().await;
    s.driver.script(vec![Step::Emit(Normalized::TurnStarted)]);
    s.prompt(1).await;
    next(&mut s.events).await;
    assert_eq!(next(&mut s.events).await, Some(Normalized::TurnStarted));
    assert_eq!(next(&mut s.events).await, None);
    s.driver.cancel_turn(&s.instance, &s.session).await.unwrap();
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::TurnEnded {
            stop_reason: StopReason::Cancelled
        })
    );
    // With no scenario queued, the next prompt hangs right after the ack.
    s.prompt(2).await;
    assert_eq!(next(&mut s.events).await, Some(Normalized::PromptAcked { key: key(2) }));
    assert_eq!(next(&mut s.events).await, None);
}

#[tokio::test(start_paused = true)]
async fn cancelling_an_idle_session_ends_no_turn() {
    let mut s = setup().await;
    s.driver.cancel_turn(&s.instance, &s.session).await.unwrap();
    assert_eq!(next(&mut s.events).await, None);
    s.driver.script(vec![Step::Emit(Normalized::TurnEnded {
        stop_reason: StopReason::Completed,
    })]);
    s.prompt(1).await;
    next(&mut s.events).await;
    next(&mut s.events).await;
    // The scenario has played out: the turn is over, cancelling it again sends nothing.
    s.driver.cancel_turn(&s.instance, &s.session).await.unwrap();
    assert_eq!(next(&mut s.events).await, None);
}

#[tokio::test(start_paused = true)]
async fn the_process_dies_mid_turn_and_later_calls_fail() {
    let mut s = setup().await;
    s.driver.script(vec![
        Step::Emit(Normalized::TurnStarted),
        Step::Emit(Normalized::Activity { tool_running: true }),
        Step::Exit {
            code: None,
            signal: Some(9),
        },
        ended(StopReason::Completed),
    ]);
    s.prompt(1).await;
    next(&mut s.events).await;
    next(&mut s.events).await;
    next(&mut s.events).await;
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::Exited {
            code: None,
            signal: Some(9)
        })
    );
    assert_eq!(next(&mut s.events).await, None, "nothing after the exit");
    assert_eq!(s.driver.health(&s.instance).await, Health::Dead);
    let err = s
        .driver
        .send_prompt(&s.instance, &s.session, "go", key(2))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FailureKind::HarnessCrash);

    // The transcript outlives the process: the session resumes on a new instance.
    let (sink, mut events) = mpsc::unbounded_channel();
    let instance = s
        .driver
        .start_instance("i-1", &ProcessPart(Value::Null), sink)
        .await
        .unwrap();
    s.driver
        .resume_session(&instance, &s.session, &SessionPart(Value::Null))
        .await
        .unwrap();
    assert_eq!(
        s.driver.transcript(&instance, &s.session, key(1)).await.unwrap(),
        Transcript::Present {
            prompt: PromptState::Unanswered
        }
    );
    s.driver.script(vec![ended(StopReason::Completed)]);
    s.driver
        .send_prompt(&instance, &s.session, "go on", key(2))
        .await
        .unwrap();
    let first = events.recv().await.unwrap();
    assert_eq!((first.seq, first.event), (1, Normalized::PromptAcked { key: key(2) }));
}

#[tokio::test(start_paused = true)]
async fn a_permission_request_continues_after_the_answer() {
    let mut s = setup().await;
    let request = Normalized::PermissionRequested {
        request_id: "r1".into(),
        kind: RequestKind::Permission,
        tool: "Bash".into(),
        input: json!({ "command": "rm -rf build" }),
        text: String::new(),
    };
    s.driver.script(vec![
        Step::Emit(request.clone()),
        state(SessionState::RequiresAction, false),
    ]);
    s.driver
        .script(vec![state(SessionState::Running, false), ended(StopReason::Completed)]);
    s.prompt(1).await;
    next(&mut s.events).await;
    assert_eq!(next(&mut s.events).await, Some(request));
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::State {
            state: SessionState::RequiresAction,
            background: false
        })
    );
    assert_eq!(next(&mut s.events).await, None, "waits for the answer");
    let answer = Answer::Allow { updated_input: None };
    s.driver.respond(&s.instance, &s.session, "r1", &answer).await.unwrap();
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::State {
            state: SessionState::Running,
            background: false
        })
    );
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::TurnEnded {
            stop_reason: StopReason::Completed
        })
    );
    assert_eq!(
        s.transcript(1).await,
        Transcript::Present {
            prompt: PromptState::Answered
        }
    );
}

#[tokio::test(start_paused = true)]
async fn background_work_and_events_outside_a_turn() {
    let mut s = setup().await;
    s.driver.script(vec![
        ended(StopReason::Completed),
        state(SessionState::Idle, true),
        Step::Wait(Duration::from_secs(600)),
        Step::Emit(Normalized::Retrying {
            attempt: 1,
            delay: Duration::from_secs(5),
            text: "overloaded".into(),
        }),
        state(SessionState::Idle, false),
    ]);
    s.prompt(1).await;
    next(&mut s.events).await;
    next(&mut s.events).await;
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::State {
            state: SessionState::Idle,
            background: true
        })
    );
    let late = next(&mut s.events).await;
    assert!(
        matches!(late, Some(Normalized::Retrying { attempt: 1, .. })),
        "after the turn ended: {late:?}"
    );
    assert_eq!(
        next(&mut s.events).await,
        Some(Normalized::State {
            state: SessionState::Idle,
            background: false
        })
    );
}

#[tokio::test]
async fn unknown_sessions_have_no_transcript_and_closed_ones_resume() {
    let s = setup().await;
    // The core holds drivers as trait objects.
    let driver: &dyn Driver = &s.driver;
    let unknown = SessionRef(json!({ "session": 99 }));
    let transcript = driver.transcript(&s.instance, &unknown, key(1)).await.unwrap();
    assert_eq!(transcript, Transcript::Missing);
    let err = s
        .driver
        .resume_session(&s.instance, &unknown, &SessionPart(Value::Null))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FailureKind::TranscriptDivergence);
    driver.close_session(&s.instance, &s.session).await;
    let closed = s.transcript(1).await;
    assert_eq!(
        closed,
        Transcript::Present {
            prompt: PromptState::Absent
        },
        "a closed session keeps its transcript"
    );
    let part = SessionPart(Value::Null);
    s.driver.resume_session(&s.instance, &s.session, &part).await.unwrap();
}

#[test]
fn prompt_keys_are_stable_and_distinct() {
    assert_eq!(key(1), PromptKey::new("run-1", 1, 1));
    assert_ne!(key(1), key(2));
    assert_ne!(key(1), PromptKey::new("run-1", 2, 1));
    assert_eq!(key(1).0.get_version_num(), 5);
}

#[test]
fn failures_take_the_default_scope_of_their_kind() {
    use aae_core::driver::Scope;
    let scope = |kind| Failure::new(kind, Source::Adapter).scope;
    assert_eq!(scope(FailureKind::QuotaExhausted), Scope::Account);
    assert_eq!(scope(FailureKind::Overloaded), Scope::ProviderModel);
    assert_eq!(scope(FailureKind::AdapterCrash), Scope::Instance);
    assert_eq!(scope(FailureKind::HollowDone), Scope::Run);
    assert_eq!(scope(FailureKind::WorkspaceFailure), Scope::Rig);
    assert_eq!(scope(FailureKind::Stall), Scope::Session);
}
