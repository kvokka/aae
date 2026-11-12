//! The scripted driver (spec §14): a fake harness for core tests that plays prepared scenarios — delays, a turn
//! that ends without an outcome, provider errors, hangs, a process dying mid-turn, permission requests, events
//! outside a turn, background work. Nothing of it runs in the product.

use aae_core::{
    BoxFuture,
    config::EffectiveConfig,
    driver::{
        Answer, Capabilities, Driver, DriverEvent, EventSink, Failure, FailureKind, Health, InstanceHandle, Normalized,
        ProcessPart, PromptKey, PromptState, SessionPart, SessionPlan, SessionRef, Source, Split, StopReason,
        Transcript,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::broadcast, task::JoinHandle};

/// One step of a scenario.
#[derive(Clone, Debug)]
pub enum Step {
    Wait(Duration),
    Emit(Normalized),
    /// The process dies: `exited` is sent and every later call on the instance fails with `harness_crash`.
    Exit {
        code: Option<i32>,
        signal: Option<i32>,
    },
}

/// Plays one queued scenario per `send_prompt` (after acknowledging the prompt) and per `respond` (the
/// continuation after a permission request). With no scenario left the prompt is acknowledged and the turn never
/// ends: a hang. A scenario without `turn.ended` is a hang too.
#[derive(Clone)]
pub struct ScriptedDriver {
    capabilities: Capabilities,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    scenarios: VecDeque<Vec<Step>>,
    instances: HashMap<String, Instance>,
    sessions: HashMap<u64, Session>,
    /// Numbers instances and sessions.
    started: u64,
}

struct Instance {
    sink: EventSink,
    seq: u64,
    alive: bool,
}

struct Session {
    instance: String,
    /// Prompts in order, with whether a completed turn followed.
    prompts: Vec<(PromptKey, bool)>,
    playing: Option<JoinHandle<()>>,
    /// A turn is in flight: from a prompt or a response until `turn.ended`.
    in_turn: bool,
}

/// No process has this pid (it is above every `pid_max`), so nothing real is ever signalled through it.
const NO_PID: i32 = i32::MAX;

impl ScriptedDriver {
    pub fn new(sessions_per_instance: usize) -> Self {
        Self {
            capabilities: Capabilities {
                sessions_per_instance,
                retries_429: false,
                reset_time: true,
            },
            state: Arc::default(),
        }
    }

    /// Queues the scenario played by the next `send_prompt` or `respond`.
    pub fn script(&self, steps: Vec<Step>) {
        self.lock().scenarios.push_back(steps);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The session behind `session` if it lives on `instance` and the instance is alive.
    fn live_session<'a>(
        state: &'a mut State,
        instance: &InstanceHandle,
        session: &SessionRef,
    ) -> Result<&'a mut Session, Failure> {
        if !state.instances.get(&instance.id).is_some_and(|i| i.alive) {
            return Err(Failure::new(FailureKind::HarnessCrash, Source::Adapter));
        }
        match state.sessions.get_mut(&session_id(session)?) {
            Some(s) if s.instance == instance.id => Ok(s),
            _ => Err(Failure::new(FailureKind::TranscriptDivergence, Source::Adapter)),
        }
    }

    /// Plays the next scenario on `session`, after `first` if given.
    fn play(&self, instance: &InstanceHandle, session: &SessionRef, first: Option<Normalized>) -> Result<(), Failure> {
        let mut state = self.lock();
        Self::live_session(&mut state, instance, session)?;
        let steps = state.scenarios.pop_front().unwrap_or_default();
        let target = Self::live_session(&mut state, instance, session)?;
        if let Some(previous) = target.playing.take() {
            previous.abort();
        }
        target.in_turn = true;
        let (driver, instance, session) = (self.clone(), instance.id.clone(), session.clone());
        target.playing = Some(tokio::spawn(async move {
            if let Some(event) = first {
                driver.emit(&instance, Some(&session), event);
            }
            for step in steps {
                match step {
                    Step::Wait(d) => tokio::time::sleep(d).await,
                    Step::Emit(event) => driver.emit(&instance, Some(&session), event),
                    Step::Exit { code, signal } => {
                        driver.exit(&instance, code, signal);
                        return;
                    }
                }
            }
        }));
        Ok(())
    }

    fn emit(&self, instance: &str, session: Option<&SessionRef>, event: Normalized) {
        let mut state = self.lock();
        if let (Some(session), Normalized::TurnEnded { stop_reason }) = (session, &event)
            && let Some(last) = session_id(session).ok().and_then(|id| state.sessions.get_mut(&id))
            && let Some(prompt) = last.prompts.last_mut()
        {
            prompt.1 |= *stop_reason == StopReason::Completed;
            last.in_turn = false;
        }
        if let Some(i) = state.instances.get_mut(instance).filter(|i| i.alive) {
            i.seq += 1;
            let _ = i.sink.send(DriverEvent {
                instance: instance.to_owned(),
                session: session.cloned(),
                seq: i.seq,
                event,
            });
        }
    }

    fn exit(&self, instance: &str, code: Option<i32>, signal: Option<i32>) {
        self.emit(instance, None, Normalized::Exited { code, signal });
        let mut state = self.lock();
        if let Some(i) = state.instances.get_mut(instance) {
            i.alive = false;
        }
        for session in state.sessions.values_mut().filter(|s| s.instance == instance) {
            if let Some(playing) = session.playing.take() {
                playing.abort();
            }
        }
    }
}

fn session_id(session: &SessionRef) -> Result<u64, Failure> {
    session.0["session"]
        .as_u64()
        .ok_or_else(|| Failure::new(FailureKind::TranscriptDivergence, Source::Adapter))
}

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(std::future::ready(value))
}

impl Driver for ScriptedDriver {
    fn harness(&self) -> &'static str {
        "scripted"
    }

    fn capabilities(&self, _process: &ProcessPart) -> Capabilities {
        self.capabilities
    }

    fn account(&self, _process: &ProcessPart) -> Option<String> {
        None
    }

    /// The native block needs a process of its own; the model and the working directory ride with the session.
    fn split(&self, config: &EffectiveConfig, plan: &SessionPlan) -> Result<Split, Failure> {
        let (SessionPlan::New { workdir, .. } | SessionPlan::Resume { workdir, .. }) = plan;
        let process = ProcessPart(json!({ "native": config.native.as_ref().map(|n| &n.value) }));
        let model = config.model.as_ref().map(|m| &m.value);
        let session = SessionPart(json!({ "model": model, "workdir": workdir }));
        Ok(Split::new(self.harness(), process, session))
    }

    fn start_instance<'a>(
        &'a self,
        _process: &'a ProcessPart,
        sink: EventSink,
    ) -> BoxFuture<'a, Result<InstanceHandle, Failure>> {
        let mut state = self.lock();
        state.started += 1;
        let id = format!("scripted-{}", state.started);
        state.instances.insert(
            id.clone(),
            Instance {
                sink,
                seq: 0,
                alive: true,
            },
        );
        ready(Ok(InstanceHandle {
            id,
            pid: NO_PID,
            start_time: 0,
            pgid: NO_PID,
        }))
    }

    fn health<'a>(&'a self, instance: &'a InstanceHandle) -> BoxFuture<'a, Health> {
        let alive = self.lock().instances.get(&instance.id).is_some_and(|i| i.alive);
        ready(if alive { Health::Ok } else { Health::Dead })
    }

    fn stop_instance<'a>(&'a self, instance: &'a InstanceHandle, _grace: Duration) -> BoxFuture<'a, ()> {
        let alive = self.lock().instances.get(&instance.id).is_some_and(|i| i.alive);
        if alive {
            self.exit(&instance.id, None, Some(15));
        }
        ready(())
    }

    fn start_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        _session: &'a SessionPart,
    ) -> BoxFuture<'a, Result<SessionRef, Failure>> {
        let mut state = self.lock();
        if !state.instances.get(&instance.id).is_some_and(|i| i.alive) {
            return ready(Err(Failure::new(FailureKind::HarnessCrash, Source::Adapter)));
        }
        state.started += 1;
        let id = state.started;
        state.sessions.insert(
            id,
            Session {
                instance: instance.id.clone(),
                prompts: Vec::new(),
                playing: None,
                in_turn: false,
            },
        );
        drop(state);
        let session = SessionRef(json!({ "session": id }));
        self.emit(&instance.id, Some(&session), Normalized::SessionStarted);
        ready(Ok(session))
    }

    /// Moves a known session onto `instance`; its transcript survives the old process, like a harness's does.
    fn resume_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        _part: &'a SessionPart,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        let mut state = self.lock();
        if !state.instances.get(&instance.id).is_some_and(|i| i.alive) {
            return ready(Err(Failure::new(FailureKind::HarnessCrash, Source::Adapter)));
        }
        let found = session_id(session).map(|id| state.sessions.get_mut(&id));
        ready(match found {
            Ok(Some(s)) => {
                s.instance = instance.id.clone();
                Ok(())
            }
            _ => Err(Failure::new(FailureKind::TranscriptDivergence, Source::Adapter)),
        })
    }

    fn send_prompt<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        _text: &'a str,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        let recorded = Self::live_session(&mut self.lock(), instance, session).map(|s| s.prompts.push((key, false)));
        ready(recorded.and_then(|()| self.play(instance, session, Some(Normalized::PromptAcked { key }))))
    }

    fn cancel_turn<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        let playing = Self::live_session(&mut self.lock(), instance, session).map(|s| (s.in_turn, s.playing.take()));
        ready(playing.map(|(in_turn, playing)| {
            if let Some(playing) = playing {
                playing.abort();
            }
            // A real harness ends only a turn that is in flight; an idle session sees nothing.
            if in_turn {
                let ended = Normalized::TurnEnded {
                    stop_reason: StopReason::Cancelled,
                };
                self.emit(&instance.id, Some(session), ended);
            }
        }))
    }

    fn respond<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        _request_id: &'a str,
        _answer: &'a Answer,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        ready(self.play(instance, session, None))
    }

    fn close_session<'a>(&'a self, _instance: &'a InstanceHandle, session: &'a SessionRef) -> BoxFuture<'a, ()> {
        let removed = session_id(session).ok().and_then(|id| self.lock().sessions.remove(&id));
        if let Some(playing) = removed.and_then(|s| s.playing) {
            playing.abort();
        }
        ready(())
    }

    fn transcript<'a>(
        &'a self,
        _instance: &'a InstanceHandle,
        session: &'a SessionRef,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<Transcript, Failure>> {
        let state = self.lock();
        let found = session_id(session).ok().and_then(|id| state.sessions.get(&id));
        ready(Ok(match found {
            None => Transcript::Missing,
            Some(s) => Transcript::Present {
                prompt: match s.prompts.iter().find(|(k, _)| *k == key) {
                    None => PromptState::Absent,
                    Some((_, false)) => PromptState::Unanswered,
                    Some((_, true)) => PromptState::Answered,
                },
            },
        }))
    }

    /// The scripted driver has no raw harness stream.
    fn live(&self, _session: &SessionRef) -> Option<broadcast::Receiver<Value>> {
        None
    }
}

#[cfg(test)]
mod tests;
