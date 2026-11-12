//! The effects of the run machine: driver calls made after commit, with their results sent back as messages.

use super::{Drivers, Msg, answer_from};
use crate::{
    BoxFuture,
    driver::{
        Driver, DriverEvent, EventSink, Failure, FailureKind, Health, InstanceHandle, Normalized, ProcessPart,
        PromptKey, PromptState, SessionPart, SessionRef, Source, Transcript,
    },
    writer::{CoreHandle, Executor},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::Duration,
};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// How long a stopped instance gets before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(10);
/// How long a launch waits for the processes it replaces to be gone (invariant 6).
const DEATH_WAIT: Duration = Duration::from_secs(30);
/// How often a started instance's health is checked.
const HEALTH_EVERY: Duration = Duration::from_secs(30);

/// The starts of registry instances in this daemon: every launch on an instance shares its one start.
pub(super) type Starts = Arc<Mutex<HashMap<String, Arc<OnceCell<Result<InstanceHandle, Failure>>>>>>;

pub(super) struct RunExecutor {
    pub drivers: Drivers,
    pub trackers: super::beads::Trackers,
    pub core: Arc<OnceLock<CoreHandle<Msg>>>,
    pub sink: EventSink,
    pub hooks: Arc<dyn super::HookRunner>,
    pub running: super::exec_step::Running,
    pub starts: Starts,
}

impl Executor for RunExecutor {
    /// A driver failure is an attempt failure reported to the writer, never a retry of the effect.
    fn execute<'a>(&'a self, _key: &'a str, payload: &'a Value) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let Some(core) = self.core.get() else { return true };
            match payload["op"].as_str() {
                Some("hooks") => self.hooks.run(core, payload).await,
                Some("tracker") => return super::beads::execute(&self.trackers, core, payload).await,
                Some("exec") => {
                    let msg = super::exec_step::run(&self.running, payload).await;
                    // A stopped writer drops the message; recovery (WP-20) finds the attempt on restart.
                    let _ = core.send(None, msg).await;
                }
                Some("stop_exec") => super::exec_step::stop(&self.running, payload["run"].as_str().unwrap_or_default()),
                Some("sweep") => {
                    let id = payload["instance"].as_str().unwrap_or_default().to_owned();
                    // Walking `/proc` is synchronous.
                    let _ = tokio::task::spawn_blocking(move || crate::orphans::sweep(&id)).await;
                }
                _ => {}
            }
            if payload["harness"].is_null() {
                return true;
            }
            let Some(driver) = payload["harness"].as_str().and_then(|h| self.drivers.get(h)) else {
                return true;
            };
            let handle = serde_json::from_value::<InstanceHandle>(payload["handle"].clone());
            let session = SessionRef(payload["session"].clone());
            let result = match (payload["op"].as_str(), handle) {
                (Some("launch"), _) => self.launch(core, driver.as_ref(), payload).await,
                (Some("stop"), Ok(handle)) => {
                    if !session.0.is_null() {
                        let _ = driver.cancel_turn(&handle, &session).await;
                        driver.close_session(&handle, &session).await;
                    }
                    Ok(())
                }
                (Some("stop_instance"), handle) => {
                    if let Ok(handle) = handle {
                        driver.stop_instance(&handle, STOP_GRACE).await;
                    }
                    let id = payload["instance"].as_str().unwrap_or_default().to_owned();
                    crate::orphans::sweep(&id);
                    self.starts.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
                    // A stopped writer drops the message; recovery (WP-20) finds the record on restart.
                    let reason = payload["reason"].as_str().unwrap_or("idle").to_owned();
                    let _ = core.send(None, Msg::InstanceStopped { id, reason }).await;
                    return true;
                }
                (Some("prompt"), Ok(handle)) => {
                    let (text, key) = (payload["text"].as_str().unwrap_or_default(), key(payload));
                    let sent = driver.send_prompt(&handle, &session, text, key).await;
                    if sent.is_ok() {
                        let run = payload["run"].as_str().unwrap_or_default().to_owned();
                        let msg = Msg::PromptSent {
                            run,
                            attempt: attempt(payload),
                            key,
                        };
                        let _ = core.send(None, msg).await;
                    }
                    sent
                }
                (Some("respond"), Ok(handle)) => match answer_from(&payload["answer"]) {
                    Some(answer) => {
                        let request = payload["request"].as_str().unwrap_or_default();
                        driver.respond(&handle, &session, request, &answer).await
                    }
                    None => Err(Failure::new(FailureKind::Unknown, Source::Core)),
                },
                _ => Ok(()),
            };
            if let Err(failure) = result {
                let msg = Msg::AttemptFailed {
                    run: payload["run"].as_str().unwrap_or_default().to_owned(),
                    attempt: attempt(payload),
                    failure,
                };
                // A stopped writer drops the message; recovery (WP-20) finds the attempt on restart.
                let _ = core.send(None, msg).await;
            }
            true
        })
    }
}

pub(super) fn attempt(payload: &Value) -> u32 {
    payload["attempt"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or_default()
}

fn key(payload: &Value) -> PromptKey {
    PromptKey(
        payload["key"]
            .as_str()
            .and_then(|k| Uuid::parse_str(k).ok())
            .unwrap_or_default(),
    )
}

impl RunExecutor {
    /// The processes it replaces gone (invariant 6), the instance, then the session (new or resumed after a
    /// transcript check, invariant 17), then the prompt. The writer learns the instance and the session before the
    /// next call, so every event of them finds its attempt.
    async fn launch(&self, core: &CoreHandle<Msg>, driver: &dyn Driver, p: &Value) -> Result<(), Failure> {
        let run = p["run"].as_str().unwrap_or_default().to_owned();
        let attempt = attempt(p);
        let key = key(p);
        let part = SessionPart(p["session"].clone());
        for old in p["after"].as_array().into_iter().flatten() {
            let Ok(old) = serde_json::from_value::<InstanceHandle>(old.clone()) else {
                continue;
            };
            if !crate::process::wait_dead(old.pid, old.start_time, DEATH_WAIT).await {
                return Err(Failure::new(FailureKind::Unknown, Source::Core));
            }
        }
        let handle = match serde_json::from_value::<InstanceHandle>(p["handle"].clone()) {
            Ok(handle) => handle,
            Err(_) => self.start(core, driver, p).await?,
        };
        let msg = Msg::Instance {
            run: run.clone(),
            attempt,
            handle: handle.clone(),
        };
        if core.send(None, msg).await.is_err() {
            return Ok(());
        }
        let (mut key, mut text) = (key, p["prompt"].as_str().unwrap_or_default().to_owned());
        let session = match p["resume"].clone() {
            Value::Null => driver.start_session(&handle, &part).await?,
            value => {
                let session = SessionRef(value);
                driver.resume_session(&handle, &session, &part).await?;
                // The prompt the daemon died under is checked, else the new one (it only shows the session is there).
                let retry = &p["retry"];
                let check = if retry.is_null() { key } else { self::key(retry) };
                match driver.transcript(&handle, &session, check).await? {
                    Transcript::Missing => {
                        return Err(Failure::new(FailureKind::TranscriptDivergence, Source::Core));
                    }
                    // The harness never got it: the same prompt with the same key, so it cannot run twice.
                    Transcript::Present {
                        prompt: PromptState::Absent,
                    } if !retry.is_null() => {
                        (key, text) = (check, retry["text"].as_str().unwrap_or_default().to_owned());
                        let msg = Msg::PromptReplayed {
                            run: run.clone(),
                            attempt,
                            key,
                            text: text.clone(),
                        };
                        if core.send(None, msg).await.is_err() {
                            return Ok(());
                        }
                    }
                    Transcript::Present { .. } => {}
                }
                session
            }
        };
        let msg = Msg::Session {
            run: run.clone(),
            attempt,
            session: session.clone(),
        };
        let _ = core.send(None, msg).await;
        driver.send_prompt(&handle, &session, &text, key).await?;
        let _ = core.send(None, Msg::PromptSent { run, attempt, key }).await;
        Ok(())
    }

    /// The one start of the registry's instance `p["instance"]`, shared by every launch waiting for it.
    async fn start(&self, core: &CoreHandle<Msg>, driver: &dyn Driver, p: &Value) -> Result<InstanceHandle, Failure> {
        let id = p["instance"].as_str().unwrap_or_default().to_owned();
        let cell = (self.starts.lock().unwrap_or_else(PoisonError::into_inner))
            .entry(id.clone())
            .or_default()
            .clone();
        let started = cell
            .get_or_init(|| async {
                let process = ProcessPart(p["process"].clone());
                let started = driver.start_instance(&id, &process, self.sink.clone()).await;
                if started.is_err() {
                    // The record goes with `instance_failed`; a later instance gets a new id.
                    self.starts.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
                }
                if let (Ok(handle), Some(driver)) = (&started, self.drivers.get(driver.harness())) {
                    self.watch(driver.clone(), handle.clone());
                }
                let msg = match &started {
                    Ok(handle) => Msg::InstanceReady {
                        id: id.clone(),
                        harness: driver.harness().to_owned(),
                        handle: handle.clone(),
                    },
                    Err(failure) => Msg::InstanceFailed {
                        id: id.clone(),
                        failure: failure.clone(),
                    },
                };
                // A stopped writer drops the message; recovery (WP-20) finds the record on restart.
                let _ = core.send(None, msg).await;
                started
            })
            .await;
        started.clone()
    }
    /// Checks the health of `handle` while its start is kept; a dead instance reports its exit, as a harness whose
    /// process ended would (contracts 8).
    fn watch(&self, driver: Arc<dyn Driver>, handle: InstanceHandle) {
        let (starts, sink) = (self.starts.clone(), self.sink.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(HEALTH_EVERY).await;
                if !(starts.lock().unwrap_or_else(PoisonError::into_inner)).contains_key(&handle.id) {
                    return;
                }
                if matches!(driver.health(&handle).await, Health::Dead) {
                    let event = Normalized::Exited {
                        code: None,
                        signal: None,
                    };
                    let _ = sink.send(DriverEvent {
                        instance: handle.id.clone(),
                        session: None,
                        seq: 0,
                        event,
                    });
                    return;
                }
            }
        });
    }
}
