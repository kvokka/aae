//! The codex adapter (spec §7, docs/design/contracts.md 4.6): one `codex app-server` per fingerprint over stdio,
//! JSON-RPC without the `jsonrpc` field, a thread per session.

mod part;
mod rpc;
mod stream;

use aae_core::{
    BoxFuture,
    config::EffectiveConfig,
    driver::{
        Answer, Capabilities, Driver, EventSink, Failure, FailureKind, Health, InstanceHandle, ProcessPart, PromptKey,
        PromptState, SessionPart, SessionPlan, SessionRef, Source, Split, Transcript,
    },
    orphans,
};
use rpc::{Instance, crashed, lock};
use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    process::Stdio,
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
/// `AAE_SOCKET`.
pub struct Codex {
    binary: PathBuf,
    state: PathBuf,
    socket: Option<PathBuf>,
    instances: Arc<Mutex<HashMap<String, Arc<Instance>>>>,
}

fn thread_id(session: &SessionRef) -> &str {
    session.0["thread_id"].as_str().unwrap_or_default()
}

impl Codex {
    pub fn new(binary: PathBuf, state: PathBuf, socket: Option<PathBuf>) -> Self {
        Self {
            binary,
            state,
            socket,
            instances: Arc::default(),
        }
    }

    fn instance(&self, id: &str) -> Result<Arc<Instance>, Failure> {
        lock(&self.instances).get(id).cloned().ok_or_else(crashed)
    }

    async fn start(&self, id: &str, p: &Value, sink: EventSink) -> Result<InstanceHandle, Failure> {
        let config_error = |what: String| {
            let mut failure = Failure::new(FailureKind::ConfigError, Source::Adapter);
            failure.raw = what;
            failure
        };
        part::materialize(p).map_err(|e| config_error(format!("{}: {e}", p["home"])))?;
        let mut command = Command::new(&self.binary);
        command
            .args(part::args(p))
            .env_clear()
            .envs(part::env(p, id, self.socket.as_deref()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child =
            orphans::spawn(&mut command).map_err(|e| config_error(format!("{}: {e}", self.binary.display())))?;
        let pid = child.id().and_then(|p| i32::try_from(p).ok()).unwrap_or_default();
        let start_time = orphans::start_time(pid).ok_or_else(crashed)?;
        let instance = Arc::new(Instance {
            home: p["home"].clone(),
            stdin: tokio::sync::Mutex::new(child.stdin.take()),
            next: AtomicU64::new(1),
            pending: Mutex::default(),
            acks: Mutex::default(),
            stream: Mutex::new(Stream::new(Some(part::account(p)))),
            requests: Mutex::default(),
            turns: Mutex::default(),
            live: Mutex::default(),
        });
        lock(&self.instances).insert(id.to_owned(), instance.clone());
        let handle = InstanceHandle {
            id: id.to_owned(),
            pid,
            start_time,
            pgid: pid,
        };
        let lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
        let stderr = tokio::spawn(rpc::tail(child.stderr.take().expect("stderr is piped")));
        let (instances, id) = (self.instances.clone(), id.to_owned());
        let reader = instance.clone();
        tokio::spawn(async move {
            let mut send = rpc::sender(id.clone(), sink);
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
        let client = json!({ "name": "aae", "title": "aae", "version": env!("CARGO_PKG_VERSION") });
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
    fn bind(&self, instance: &Instance, thread: &str, result: &Value) {
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
            // shortcut: the contract's `max_sessions` config value (default 8) is not read yet; WP-29b.
            sessions_per_instance: 8,
            instance_idle: Duration::from_secs(60),
            retries_429: false,
            reset_time: true,
            versions: [(0, 167, 2), (0, 167, 2)],
        }
    }

    fn account(&self, process: &ProcessPart) -> Option<String> {
        Some(part::account(&process.0))
    }

    fn split(&self, config: &EffectiveConfig, plan: &SessionPlan) -> Result<Split, Failure> {
        let process = part::process(config, &self.state)?;
        let session = part::session(config, plan)?;
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
        Box::pin(async move {
            match orphans::start_time(instance.pid) == Some(instance.start_time) {
                true => Health::Ok,
                false => Health::Dead,
            }
        })
    }

    /// EOF on stdin first (app-server exits on it at once), then the group is terminated, then killed.
    fn stop_instance<'a>(&'a self, instance: &'a InstanceHandle, grace: Duration) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let known = lock(&self.instances).remove(&instance.id);
            if let Some(known) = known {
                known.stdin.lock().await.take();
            }
            let alive = || orphans::start_time(instance.pid) == Some(instance.start_time);
            for signal in [None, Some(Signal::TERM), Some(Signal::KILL)] {
                if let (Some(signal), Some(group)) = (signal, Pid::from_raw(instance.pgid)) {
                    let _ = kill_process_group(group, signal);
                }
                let until = Instant::now() + grace / 2;
                while alive() && Instant::now() < until {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                if !alive() {
                    return;
                }
            }
        })
    }

    fn start_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionPart,
    ) -> BoxFuture<'a, Result<SessionRef, Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let result = known.call("thread/start", session.0.clone(), None).await?;
            let thread = result["thread"]["id"].as_str().ok_or_else(crashed)?;
            self.bind(&known, thread, &result);
            Ok(SessionRef(json!({ "thread_id": thread, "codex_home": known.home })))
        })
    }

    /// The whole thread config goes again: whether app-server keeps it across a restart is not known.
    fn resume_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        session: &'a SessionRef,
        part: &'a SessionPart,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let mut params = part.0.clone();
            params["threadId"] = json!(thread_id(session));
            params["excludeTurns"] = json!(true);
            let result = known.call("thread/resume", params, None).await?;
            self.bind(&known, thread_id(session), &result);
            Ok(())
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
            lock(&known.stream).interrupted(thread);
            let params = json!({ "threadId": thread, "turnId": turn });
            known.call("turn/interrupt", params, None).await.map(|_| ())
        })
    }

    fn respond<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        _session: &'a SessionRef,
        request_id: &'a str,
        answer: &'a Answer,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let Some((id, method)) = lock(&known.requests).remove(request_id) else {
                let mut failure = Failure::new(FailureKind::Unknown, Source::Adapter);
                failure.raw = format!("no open request `{request_id}`");
                return Err(failure);
            };
            let result = match (method.as_str(), answer) {
                // shortcut: the answer form of `requestUserInput` is the schema's, not seen live; WP-30 pins it.
                ("item/tool/requestUserInput", Answer::Text { text }) => json!({ "answers": { "answer": [text] } }),
                (_, Answer::Allow { .. }) => json!({ "decision": "accept" }),
                (_, Answer::Deny { .. } | Answer::Text { .. }) => json!({ "decision": "decline" }),
            };
            known.write(&json!({ "id": id, "result": result })).await
        })
    }

    fn close_session<'a>(&'a self, instance: &'a InstanceHandle, session: &'a SessionRef) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let Ok(known) = self.instance(&instance.id) else { return };
            lock(&known.live).remove(thread_id(session));
            let _ = known
                .call("thread/unsubscribe", json!({ "threadId": thread_id(session) }), None)
                .await;
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
            let Ok(result) = known.call("thread/read", params, None).await else {
                return Ok(Transcript::Missing);
            };
            let key = key.0.to_string();
            let turns = result["thread"]["turns"].as_array().cloned().unwrap_or_default();
            let holds = |turn: &&Value| {
                let items = turn["items"].as_array().into_iter().flatten();
                items
                    .into_iter()
                    .any(|i| i["type"] == "userMessage" && i["clientId"] == key.as_str())
            };
            let prompt = match turns.iter().find(holds) {
                None => PromptState::Absent,
                Some(turn) if turn["status"] == "completed" => PromptState::Answered,
                Some(_) => PromptState::Unanswered,
            };
            Ok(Transcript::Present { prompt })
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
