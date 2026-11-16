//! How a test sets up and reads the scripted driver: scenarios, instances, tokens, what sessions were given.

use super::{NO_PID, ScriptedDriver, Session, State, Step};
use aae_contract::driver::{Capabilities, PromptKey};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

impl ScriptedDriver {
    /// `instance_idle` is zero, as for a process per session; [`Self::idle`] sets it.
    pub fn new(sessions_per_instance: usize) -> Self {
        Self {
            harness: "scripted",
            capabilities: Capabilities {
                sessions_per_instance,
                instance_idle: Duration::ZERO,
                versions: [(0, 0, 0), (u32::MAX, u32::MAX, u32::MAX)],
            },
            state: Arc::default(),
        }
    }

    /// The same driver, with its scenarios and sessions, as the harness `harness`.
    pub fn named(&self, harness: &'static str) -> Self {
        Self {
            harness,
            ..self.clone()
        }
    }

    pub fn idle(mut self, instance_idle: Duration) -> Self {
        self.capabilities.instance_idle = instance_idle;
        self
    }

    /// Queues the scenario played by the next `send_prompt` or `respond`.
    pub fn script(&self, steps: Vec<Step>) {
        self.lock().scenarios.push_back(steps);
    }

    /// Every later `start_session` never returns: a process that never reports its session.
    pub fn stall_sessions(&self) {
        self.lock().stalled = true;
    }

    /// Every later `start_session` blocks its thread until the returned sender sends or is dropped: like a call into
    /// a synchronous library, it cannot be cancelled meanwhile.
    pub fn hold_sessions(&self) -> std::sync::mpsc::Sender<()> {
        let (release, held) = std::sync::mpsc::channel();
        self.lock().held = Some(Arc::new(Mutex::new(held)));
        release
    }

    /// Every later instance has a real process (a `sleep`) behind its handle, which dies `dying` after
    /// `stop_instance` is called.
    pub fn real_processes(&self, dying: Duration) {
        self.lock().dying = Some(dying);
    }

    /// `(pid, start_time)` of every real process started so far.
    pub fn spawned(&self) -> Vec<(i32, u64)> {
        self.lock().spawned.clone()
    }

    /// Every later `send_prompt` returns but the harness never gets the prompt: no transcript entry, no ack.
    pub fn lose_prompts(&self, losing: bool) {
        self.lock().losing = losing;
    }

    /// What sessions leave in the background from now on are commands the harness can stop; by default it is work
    /// that is never stopped, as a subagent.
    pub fn stoppable_background(&self) {
        self.lock().stoppable = true;
    }

    /// How many times the core asked to stop what a session left in the background.
    pub fn background_stops(&self) -> usize {
        self.lock().background_stops
    }

    /// Every instance started so far reports `Degraded{text}` to its health checks (`None`: `Ok` again).
    pub fn degrade(&self, text: Option<&str>) {
        for instance in self.lock().instances.values_mut() {
            instance.degraded = text.map(str::to_owned);
        }
    }

    /// The key of every prompt sent so far, in order.
    pub fn keys(&self) -> Vec<PromptKey> {
        self.lock().keys.clone()
    }

    /// The driver of a daemon started after this one died: no process of it is known, the transcripts of its
    /// sessions survive (as a harness keeps them on disk), and so do the prompts and keys sent so far.
    pub fn after_crash(&self) -> Self {
        let old = self.lock();
        let sessions = old.sessions.iter().map(|(id, s)| {
            let session = Session {
                instance: String::new(),
                prompts: s.prompts.clone(),
                playing: None,
                in_turn: false,
                empty: s.empty,
            };
            (*id, session)
        });
        let state = State {
            sessions: sessions.collect(),
            started: old.started,
            texts: old.texts.clone(),
            keys: old.keys.clone(),
            ..State::default()
        };
        Self {
            harness: self.harness,
            capabilities: self.capabilities,
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// The transcripts no live process holds are gone, as when the harness's files are lost: a resume of one quietly
    /// gives an empty session, and only the transcript tells (invariant 17).
    pub fn lose_transcripts(&self) {
        self.lock().sessions.retain(|_, s| !s.instance.is_empty());
    }

    /// Whether the harness has a transcript of `session`, as the attempts table stores it.
    pub fn has_transcript(&self, session: &Value) -> bool {
        session["session"]
            .as_u64()
            .is_some_and(|id| self.lock().sessions.get(&id).is_some_and(|s| !s.empty))
    }

    /// The token of attempt `attempt` of `run`, as the agent of the attempt would find it in its environment.
    pub fn token(&self, run: &str, attempt: u32) -> Option<String> {
        self.lock().tokens.get(&(run.to_owned(), attempt)).map(|t| t.0.clone())
    }

    /// The MCP servers the session of attempt `attempt` of `run` gets, by name.
    pub fn mcp(&self, run: &str, attempt: u32) -> Value {
        self.part(run, attempt, "mcp")
    }

    /// The working directory the session of attempt `attempt` of `run` starts in.
    pub fn workdir(&self, run: &str, attempt: u32) -> Value {
        self.part(run, attempt, "workdir")
    }

    fn part(&self, run: &str, attempt: u32, field: &str) -> Value {
        let state = self.lock();
        state
            .parts
            .get(&(run.to_owned(), attempt))
            .map_or(Value::Null, |p| p[field].clone())
    }

    /// The environment the session of attempt `attempt` of `run` gets from the effective config and the tracker
    /// actor, and every prompt text sent so far, in order.
    pub fn sent(&self, run: &str, attempt: u32) -> (Option<BTreeMap<String, String>>, Vec<String>) {
        let state = self.lock();
        let env = state.tokens.get(&(run.to_owned(), attempt)).map(|t| t.1.clone());
        (env, state.texts.clone())
    }
}

/// The `(pid, start_time)` of a new real process of instance `id` under [`ScriptedDriver::real_processes`], else
/// of no process.
pub(crate) fn real_process(state: &mut State, id: &str) -> (i32, u64) {
    if state.dying.is_none() {
        return (NO_PID, 0);
    }
    let process = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("sleep starts");
    let pid = i32::try_from(process.id()).expect("a pid fits i32");
    let start_time = aae_contract::orphans::start_time(pid).expect("a live process has a start time");
    state.processes.insert(id.to_owned(), process);
    state.spawned.push((pid, start_time));
    (pid, start_time)
}
