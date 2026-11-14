//! How a test sets up and reads the scripted driver: scenarios, instances, tokens, what sessions were given.

use super::{ScriptedDriver, Session, State, Step};
use aae_core::driver::{Capabilities, PromptKey};
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
            capabilities: Capabilities {
                sessions_per_instance,
                instance_idle: Duration::ZERO,
                versions: [(0, 0, 0), (u32::MAX, u32::MAX, u32::MAX)],
            },
            state: Arc::default(),
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

    /// Every later `send_prompt` returns but the harness never gets the prompt: no transcript entry, no ack.
    pub fn lose_prompts(&self, losing: bool) {
        self.lock().losing = losing;
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
        self.lock()
            .mcp
            .get(&(run.to_owned(), attempt))
            .cloned()
            .unwrap_or_default()
    }

    /// The environment the session of attempt `attempt` of `run` gets from the effective config, and every prompt
    /// text sent so far, in order.
    pub fn sent(&self, run: &str, attempt: u32) -> (Option<BTreeMap<String, String>>, Vec<String>) {
        let state = self.lock();
        let env = state.tokens.get(&(run.to_owned(), attempt)).map(|t| t.1.clone());
        (env, state.texts.clone())
    }
}
