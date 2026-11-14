//! The scripted driver (spec §14): a fake harness for core tests that plays prepared scenarios — delays, a turn
//! that ends without an outcome, provider errors, hangs, a process dying mid-turn, permission requests, events
//! outside a turn, background work; fake providers for the real harnesses ([`anthropic`], [`openai`]) and a test
//! MCP service ([`mcp`]). Nothing of it runs in the product.

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
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::broadcast, task::JoinHandle};

pub mod anthropic;
mod control;
pub mod mcp;
pub mod openai;
mod play;
mod step;
mod tracker;
use play::session_id;
pub use step::Step;
pub use tracker::MemoryTracker;

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
    /// The attempt token the core handed over and the session environment, by `(run, attempt)`.
    tokens: HashMap<(String, u32), (String, BTreeMap<String, String>)>,
    /// The MCP servers each attempt's session starts with, as its launch gave them.
    mcp: HashMap<(String, u32), Value>,
    texts: Vec<String>,
    /// Set by [`ScriptedDriver::stall_sessions`].
    stalled: bool,
    /// Set by [`ScriptedDriver::lose_prompts`].
    losing: bool,
    /// The key of every prompt sent, in order.
    keys: Vec<PromptKey>,
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
    /// The resume of a lost transcript made this session: it is empty, whatever the core asked for.
    empty: bool,
}

/// No process has this pid (it is above every `pid_max`), so nothing real is ever signalled through it.
const NO_PID: i32 = i32::MAX;

/// Keeps the environment and the MCP servers a session starts with, as its launch gave them.
fn got(state: &mut State, part: &SessionPart) {
    let run = part.0["run"].as_str().unwrap_or_default().to_owned();
    let attempt = part.0["attempt"].as_u64().and_then(|a| u32::try_from(a).ok());
    let key = (run, attempt.unwrap_or_default());
    let env = serde_json::from_value(part.0["env"].clone()).unwrap_or_default();
    if let Some(entry) = state.tokens.get_mut(&key) {
        entry.1 = env;
    }
    state.mcp.insert(key, part.0["mcp"].clone());
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

    fn account(&self, process: &ProcessPart) -> Option<String> {
        process.0["native"]["account"].as_str().map(str::to_owned)
    }

    /// The native block needs a process of its own; the model and the working directory ride with the session.
    fn split(&self, config: &EffectiveConfig, plan: &SessionPlan) -> Result<Split, Failure> {
        let (SessionPlan::New {
            workdir,
            run,
            attempt,
            token,
        }
        | SessionPlan::Resume {
            workdir,
            run,
            attempt,
            token,
            ..
        }) = plan;
        let env: BTreeMap<_, _> = config.env.iter().map(|(k, v)| (k, &v.value)).collect();
        let mcp: BTreeMap<_, _> = config.mcp.iter().map(|(n, m)| (n, &m.value)).collect();
        let entry = (token.clone(), BTreeMap::new());
        self.lock().tokens.insert((run.clone(), *attempt), entry);
        let process = ProcessPart(json!({ "native": config.native.as_ref().map(|n| &n.value) }));
        let model = config.model.as_ref().map(|m| &m.value);
        // The environment and the MCP servers ride with the session: the launch resolves their service ports.
        let session = SessionPart(json!({
            "model": model, "workdir": workdir, "run": run, "attempt": attempt, "env": env, "mcp": mcp,
        }));
        Ok(Split::new(self.harness(), process, session))
    }

    fn start_instance<'a>(
        &'a self,
        id: &'a str,
        _process: &'a ProcessPart,
        sink: EventSink,
    ) -> BoxFuture<'a, Result<InstanceHandle, Failure>> {
        let mut state = self.lock();
        let id = id.to_owned();
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
        part: &'a SessionPart,
    ) -> BoxFuture<'a, Result<SessionRef, Failure>> {
        let mut state = self.lock();
        got(&mut state, part);
        if state.stalled {
            return Box::pin(std::future::pending());
        }
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
                empty: false,
            },
        );
        drop(state);
        let session = SessionRef(json!({ "session": id }));
        ready(Ok(session))
    }

    /// Moves a session onto `instance`; its transcript survives the old process, like a harness's does. A session
    /// with no transcript resumes too, empty, as the worst harness would have it.
    fn resume_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        part: &'a SessionPart,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        let mut state = self.lock();
        got(&mut state, part);
        if !state.instances.get(&instance.id).is_some_and(|i| i.alive) {
            return ready(Err(Failure::new(FailureKind::HarnessCrash, Source::Adapter)));
        }
        let id = match session_id(session) {
            Ok(id) => id,
            Err(e) => return ready(Err(e)),
        };
        let s = state.sessions.entry(id).or_insert_with(|| Session {
            instance: String::new(),
            prompts: Vec::new(),
            playing: None,
            in_turn: false,
            empty: true,
        });
        s.instance = instance.id.clone();
        ready(Ok(()))
    }

    fn send_prompt<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        text: &'a str,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        let recorded = {
            let mut state = self.lock();
            state.texts.push(text.to_owned());
            state.keys.push(key);
            let losing = state.losing;
            // Only a live session can drop a prompt: a dead process refuses it, as a harness's does.
            Self::live_session(&mut state, instance, session).map(|s| (!losing).then(|| s.prompts.push((key, false))))
        };
        match recorded {
            Ok(Some(())) => ready(self.play(instance, session, Some(Normalized::PromptAcked { key }))),
            Ok(None) => ready(Ok(())),
            Err(e) => ready(Err(e)),
        }
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
        // Closing detaches the session from its process; the transcript stays resumable, like a harness's does.
        let mut state = self.lock();
        if let Some(s) = session_id(session).ok().and_then(|id| state.sessions.get_mut(&id)) {
            s.instance.clear();
            if let Some(playing) = s.playing.take() {
                playing.abort();
            }
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
        let found = session_id(session)
            .ok()
            .and_then(|id| state.sessions.get(&id))
            .filter(|s| !s.empty);
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
