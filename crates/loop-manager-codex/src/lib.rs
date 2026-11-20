//! The codex adapter (docs/design/adapters.md, docs/design/contracts.md 4.6): one `codex app-server` per fingerprint
//! over stdio, JSON-RPC without the `jsonrpc` field, a thread per session.

#![deny(clippy::disallowed_methods)]

mod adapter;
mod part;
mod roles;
mod rpc;
mod stream;

pub use adapter::ADAPTER;

use loop_manager_contract::{
    BoxFuture,
    config::EffectiveConfig,
    driver::{
        Capabilities, Driver, EventSink, Failure, FailureKind, Health, InstanceHandle, ProcessPart, PromptKey,
        SessionPart, SessionPlan, SessionRef, Source, Split, Transcript, child,
    },
    orphans,
};
use rpc::{Instance, crashed, lock, unread};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicU64},
    time::{Duration, Instant},
};
use stream::Stream;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::broadcast,
};

/// The codex driver; `state` holds the homes of isolated processes, `socket` is the API socket a session gets in
/// `LOOP_MANAGER_SOCKET`.
pub struct Codex {
    binary: PathBuf,
    state: PathBuf,
    socket: Option<PathBuf>,
    max_sessions: usize,
    instance_idle: Duration,
    /// How long a resume waits for app-server to close the thread it still holds.
    unload_wait: Duration,
    instances: Arc<Mutex<HashMap<String, Arc<Instance>>>>,
}

/// The oldest supported codex, inclusive; newer versions have no upper bound.
const MINIMUM: (u32, u32, u32) = (0, 171, 0);

fn thread_id(session: &SessionRef) -> &str {
    session.0["thread_id"].as_str().unwrap_or_default()
}

impl Codex {
    /// `max_sessions`: `[harness.codex] max_sessions`, 8 by default (contracts 4.5).
    pub fn new(binary: PathBuf, state: PathBuf, socket: Option<PathBuf>, max_sessions: usize) -> Self {
        Self {
            binary,
            state,
            socket,
            max_sessions,
            instance_idle: Duration::from_secs(60),
            unload_wait: Duration::from_secs(10),
            instances: Arc::default(),
        }
    }

    /// How long an app-server without threads lives on: `[harness.codex] instance_idle_s`, 60 s by default
    /// (contracts 4.5).
    pub fn instance_idle(mut self, idle: Duration) -> Self {
        self.instance_idle = idle;
        self
    }

    fn instance(&self, id: &str) -> Result<Arc<Instance>, Failure> {
        lock(&self.instances).get(id).cloned().ok_or_else(crashed)
    }

    async fn start(&self, id: &str, p: &Value, sink: EventSink) -> Result<InstanceHandle, Failure> {
        let env = part::env(p, id, self.socket.as_deref())?;
        part::materialize(p).map_err(|e| child::config_error(format!("{}: {e}", p["home"])))?;
        let mut command = Command::new(&self.binary);
        command.args(part::args(p)).env_clear().envs(env);
        let (mut child, pid, start_time) = child::spawn(&mut command, &self.binary)?;
        let instance = Arc::new(Instance {
            home: p["home"].clone(),
            stdin: tokio::sync::Mutex::new(child.stdin.take()),
            next: AtomicU64::new(1),
            pending: Mutex::default(),
            acks: Mutex::default(),
            stream: Mutex::new(Stream::new(Some(part::account(p)))),
            turns: Mutex::default(),
            live: Mutex::default(),
            tokens: Mutex::default(),
        });
        lock(&self.instances).insert(id.to_owned(), instance.clone());
        let handle = InstanceHandle {
            id: id.to_owned(),
            pid,
            start_time,
            pgid: pid,
        };
        let lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
        let stderr = tokio::spawn(loop_manager_contract::driver::stderr_tail(
            child.stderr.take().expect("stderr is piped"),
        ));
        let (instances, id) = (self.instances.clone(), id.to_owned());
        let reader = instance.clone();
        let events = child::Events::new(&id, sink);
        let (watched, known, gone) = (events.clone(), self.instances.clone(), id.clone());
        // The calls waiting on an answer the reader will never route fail at once.
        let forget = move || lock(&known).remove(&gone).iter().for_each(|i| lock(&i.pending).clear());
        let task = tokio::spawn(async move {
            let mut send = rpc::sender(events, reader.home.clone());
            rpc::read(&reader, lines, &mut send).await;
            let status = child.wait().await.ok();
            orphans::waited(u32::try_from(pid).ok());
            lock(&instances).remove(&id);
            lock(&reader.pending).clear();
            let stderr = tokio::time::timeout(Duration::from_secs(1), stderr).await;
            let stderr = stderr.ok().and_then(Result::ok).unwrap_or_default();
            let (code, signal) = (status.and_then(|s| s.code()), status.and_then(|s| s.signal()));
            for (thread, event) in lock(&reader.stream).exited(code, signal, &stderr) {
                send(thread, event);
            }
        });
        child::watch(task, watched, handle.clone(), forget);
        let client = json!({ "name": "loop-manager", "title": "loop-manager", "version": env!("CARGO_PKG_VERSION") });
        let init = json!({ "clientInfo": client, "capabilities": { "experimentalApi": true } });
        let started = match instance.call("initialize", init, None).await {
            Ok(_) => instance.write(&json!({ "method": "initialized" })).await,
            Err(failure) => Err(failure),
        };
        if let Err(failure) = started {
            self.stop_instance(&handle, Duration::from_secs(2)).await;
            return Err(failure);
        }
        Ok(handle)
    }

    /// Binds a started or resumed thread: its model names its failures, its messages go to its live channel.
    fn bind(&self, instance: &Instance, thread: &str, result: &Value, part: &Value) {
        let token = part["config"]["shell_environment_policy"]["set"]["LOOP_MANAGER_ATTEMPT_TOKEN"].as_str();
        lock(&instance.tokens).insert(thread.to_owned(), token.unwrap_or_default().to_owned());
        let model = result["model"].as_str().map(str::to_owned);
        lock(&instance.stream).bind(thread, model);
        lock(&instance.live).insert(thread.to_owned(), broadcast::channel(256).0);
    }
}

impl Driver for Codex {
    fn harness(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self, _process: &ProcessPart) -> Capabilities {
        Capabilities {
            sessions_per_instance: self.max_sessions,
            instance_idle: self.instance_idle,
            min_version: MINIMUM,
        }
    }

    fn account(&self, process: &ProcessPart) -> Option<String> {
        Some(part::account(&process.0))
    }

    fn split(&self, config: &EffectiveConfig, plan: &SessionPlan) -> Result<Split, Failure> {
        let process = part::process(config, &self.state, &part::host_home())?;
        let session = part::session(config, plan, &self.state)?;
        Ok(Split::new("codex", ProcessPart(process), SessionPart(session)))
    }

    fn start_instance<'a>(
        &'a self,
        id: &'a str,
        process: &'a ProcessPart,
        sink: EventSink,
    ) -> BoxFuture<'a, Result<InstanceHandle, Failure>> {
        Box::pin(self.start(id, &process.0, sink))
    }

    fn health<'a>(&'a self, instance: &'a InstanceHandle) -> BoxFuture<'a, Health> {
        Box::pin(async move { child::health(instance) })
    }

    /// EOF on stdin first (app-server exits on it at once) when this daemon started it, then the group is terminated,
    /// then killed.
    fn stop_instance<'a>(&'a self, instance: &'a InstanceHandle, grace: Duration) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let known = lock(&self.instances).remove(&instance.id);
            // An instance of a dead daemon (recovery) has no stdin here to close: straight to the signals.
            let eof = known.is_some();
            if let Some(known) = known {
                known.stdin.lock().await.take();
            }
            child::stop(instance, grace, eof).await;
        })
    }

    fn start_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionPart,
    ) -> BoxFuture<'a, Result<SessionRef, Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let params = {
                let _one = lock(&self.instances);
                roles::write(&session.0)?
            };
            let result = known.call("thread/start", params, None).await?;
            let thread = result["thread"]["id"].as_str().ok_or_else(crashed)?;
            self.bind(&known, thread, &result, &session.0);
            lock(&known.stream).held(thread).map_or(Ok(()), Err)?;
            Ok(rpc::session(thread, &known.home))
        })
    }

    /// The whole thread config goes again, and only a load takes it: a thread this app-server still holds would
    /// rejoin with the last attempt's token, so it is closed first (docs/design/experiments.md).
    fn resume_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        part: &'a SessionPart,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let thread = json!({ "threadId": thread_id(session) });
            let until = Instant::now() + self.unload_wait;
            while known.call("thread/unsubscribe", thread.clone(), None).await?["status"] != "notLoaded" {
                if Instant::now() >= until {
                    let mut failure = Failure::new(FailureKind::TranscriptDivergence, Source::Adapter);
                    failure.raw = format!("app-server did not close thread {} to resume it", thread_id(session));
                    return Err(failure);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let mut params = {
                let _one = lock(&self.instances);
                roles::write(&part.0)?
            };
            params["threadId"] = json!(thread_id(session));
            params["excludeTurns"] = json!(true);
            let result = loop {
                match known.call("thread/resume", params.clone(), None).await {
                    // `notLoaded` may come while the thread is still closing; codex then asks to retry the resume.
                    Err(failure) if failure.raw.contains("retry thread/resume") && Instant::now() < until => {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    answer => break answer?,
                }
            };
            self.bind(&known, thread_id(session), &result, &part.0);
            lock(&known.stream).held(thread_id(session)).map_or(Ok(()), Err)
        })
    }

    fn send_prompt<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        text: &'a str,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let thread = thread_id(session).to_owned();
            lock(&known.stream).held(&thread).map_or(Ok(()), Err)?;
            // The key is the message's `clientUserMessageId`; app-server does not deduplicate it (experiment 2).
            let params = json!({ "threadId": thread, "clientUserMessageId": key.0.to_string(),
                "input": [{ "type": "text", "text": text, "text_elements": [] }] });
            known.call("turn/start", params, Some((thread, key))).await.map(|_| ())
        })
    }

    fn cancel_turn<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let thread = thread_id(session);
            let Some(turn) = lock(&known.turns).get(thread).cloned() else {
                return Ok(());
            };
            if !lock(&known.stream).interrupted(thread) {
                return Ok(());
            }
            let params = json!({ "threadId": thread, "turnId": turn });
            known.call("turn/interrupt", params, None).await.map(|_| ())
        })
    }

    /// Only this thread's commands go; each one's `item/completed` follows, and the stream then reports idle without
    /// background. Not `thread/unsubscribe`: it closes the thread and reports no command's end (experiment 8).
    fn stop_background<'a>(&'a self, instance: &'a InstanceHandle, session: &'a SessionRef) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let Ok(known) = self.instance(&instance.id) else { return };
            let params = json!({ "threadId": thread_id(session) });
            let _ = known.call("thread/backgroundTerminals/clean", params, None).await;
        })
    }

    fn close_session<'a>(&'a self, instance: &'a InstanceHandle, session: &'a SessionRef) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let Ok(known) = self.instance(&instance.id) else { return };
            lock(&known.live).remove(thread_id(session));
            let token = lock(&known.tokens).remove(thread_id(session)).filter(|t| !t.is_empty());
            let _ = known
                .call("thread/unsubscribe", json!({ "threadId": thread_id(session) }), None)
                .await;
            // A command that detached itself is no background of the thread and would outlive it on the shared
            // app-server; it still carries the attempt's token (experiment 8). Walking `/proc` is synchronous.
            if let Some(token) = token {
                let sweep = move || loop_manager_contract::orphans::sweep_var("LOOP_MANAGER_ATTEMPT_TOKEN", &token);
                let _ = tokio::task::spawn_blocking(sweep).await;
            }
        })
    }

    /// The thread history: the turn holding a `userMessage` with `clientId == key` is answered once completed.
    fn transcript<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<Transcript, Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let params = json!({ "threadId": thread_id(session), "includeTurns": true });
            let result = match known.call("thread/read", params, None).await {
                Ok(result) => result,
                Err(failure) => return unread(failure),
            };
            let key = key.0.to_string();
            let turns = result["thread"]["turns"].as_array().cloned().unwrap_or_default();
            let holds = |turn: &&Value| {
                let items = turn["items"].as_array().into_iter().flatten();
                items
                    .into_iter()
                    .any(|i| i["type"] == "userMessage" && i["clientId"] == key.as_str())
            };
            Ok(Transcript::Present {
                delivered: turns.iter().any(|t| holds(&t)),
            })
        })
    }

    fn live(&self, session: &SessionRef) -> Option<broadcast::Receiver<Value>> {
        let instances = lock(&self.instances);
        let thread = thread_id(session);
        instances
            .values()
            .find_map(|i| lock(&i.live).get(thread).map(broadcast::Sender::subscribe))
    }
}

#[cfg(test)]
mod tests;
