//! The claude adapter (docs/design/adapters.md, docs/design/contracts.md 4.6): one `claude -p` process per session,
//! stream-json on stdin and stdout.

#![deny(clippy::disallowed_methods)]

mod adapter;
mod part;
mod stream;

pub use adapter::ADAPTER;

use loop_manager_contract::{
    BoxFuture,
    config::EffectiveConfig,
    driver::{
        Capabilities, Driver, EventSink, Failure, FailureKind, Health, InstanceHandle, Normalized, ProcessPart,
        PromptKey, SessionPart, SessionPlan, SessionRef, Source, Split, Transcript, child,
    },
    orphans,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use stream::Stream;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::broadcast,
};
use uuid::Uuid;

/// The oldest supported claude, inclusive; newer versions have no upper bound.
const MINIMUM: (u32, u32, u32) = (2, 1, 329);

/// The claude driver; `state` holds the config directories of isolated sessions, `socket` is the API socket a
/// session gets in `LOOP_MANAGER_SOCKET`.
pub struct Claude {
    binary: PathBuf,
    state: PathBuf,
    socket: Option<PathBuf>,
    instances: Arc<Mutex<HashMap<String, Arc<Instance>>>>,
}

struct Instance {
    session: SessionRef,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    stream: Arc<Mutex<Stream>>,
    live: broadcast::Sender<Value>,
}

impl Instance {
    async fn write(&self, line: &Value) -> Result<(), Failure> {
        let mut stdin = self.stdin.lock().await;
        let crashed = || Failure::new(FailureKind::HarnessCrash, Source::Adapter);
        let stdin = stdin.as_mut().ok_or_else(crashed)?;
        let text = format!("{line}\n");
        stdin.write_all(text.as_bytes()).await.map_err(|_| crashed())?;
        stdin.flush().await.map_err(|_| crashed())
    }
}

impl Claude {
    pub fn new(binary: PathBuf, state: PathBuf, socket: Option<PathBuf>) -> Self {
        Self {
            binary,
            state,
            socket,
            instances: Arc::default(),
        }
    }

    fn instance(&self, id: &str) -> Result<Arc<Instance>, Failure> {
        let instances = self.instances.lock().unwrap_or_else(PoisonError::into_inner);
        instances
            .get(id)
            .cloned()
            .ok_or_else(|| Failure::new(FailureKind::HarnessCrash, Source::Adapter))
    }

    async fn write(&self, instance: &InstanceHandle, line: &Value) -> Result<(), Failure> {
        self.instance(&instance.id)?.write(line).await
    }

    async fn start(&self, id: &str, p: &Value, sink: EventSink) -> Result<InstanceHandle, Failure> {
        // The generated files are this process's alone and go with it: a resume writes them again.
        let mut p = p.clone();
        let files = PathBuf::from(p["files"].as_str().unwrap_or_default()).join(id);
        p["files"] = json!(files);
        let p = &p;
        let env = part::env(p, id, self.socket.as_deref(), &part::host_dir())?;
        let mut command = Command::new(&self.binary);
        command
            .args(part::args(p))
            .env_clear()
            .envs(env)
            .current_dir(p["workdir"].as_str().unwrap_or("."));
        let config_error = |what: &Path, e: std::io::Error| child::config_error(format!("{}: {e}", what.display()));
        part::materialize(p).map_err(|e| config_error(&files, e))?;
        if let Some(dir) = p["config_dir"].as_str() {
            std::fs::create_dir_all(dir).map_err(|e| config_error(Path::new(dir), e))?;
        }
        let (mut child, pid, start_time) = child::spawn(&mut command, &self.binary)?;
        let session = SessionRef(json!({
            "session_id": p["session_id"], "config_dir": p["config_dir"], "workdir": p["workdir"],
        }));
        let stream = Stream::new(Some(part::account(p)), p["model"].as_str().map(str::to_owned));
        let instance = Arc::new(Instance {
            session: session.clone(),
            stdin: tokio::sync::Mutex::new(child.stdin.take()),
            stream: Arc::new(Mutex::new(stream)),
            live: broadcast::channel(256).0,
        });
        let own = instance.clone();
        let mut lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
        let stderr = tokio::spawn(loop_manager_contract::driver::stderr_tail(
            child.stderr.take().expect("stderr is piped"),
        ));
        let instances = self.instances.clone();
        let id = id.to_owned();
        self.instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone(), instance);
        let handle = InstanceHandle {
            id: id.clone(),
            pid,
            start_time,
            pgid: pid,
        };
        let events = child::Events::new(&id, sink);
        let (watched, known, gone) = (events.clone(), self.instances.clone(), id.clone());
        let forget = move || drop(known.lock().unwrap_or_else(PoisonError::into_inner).remove(&gone));
        let reader = tokio::spawn(async move {
            let send = |session: Option<&SessionRef>, event| events.send(session.cloned(), event);
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let _ = own.live.send(msg.clone());
                // Nothing in loop-manager answers a request of claude's (a permission prompt goes nowhere with
                // `--permission-prompts none`); an unanswered one, say an MCP server's elicitation, would hold the
                // turn until `stall`.
                if msg["type"] == "control_request" {
                    let refusal = json!({ "type": "control_response", "response": { "subtype": "error",
                        "request_id": msg["request_id"], "error": "loop-manager answers no control request" } });
                    let _ = own.write(&refusal).await;
                }
                let events = own
                    .stream
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .line(&msg, Instant::now());
                for event in events {
                    send(Some(&session), event);
                }
            }
            let status = child.wait().await.ok();
            orphans::waited(u32::try_from(pid).ok());
            let _ = std::fs::remove_dir_all(&files);
            instances.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            let stderr = tokio::time::timeout(Duration::from_secs(1), stderr).await;
            let stderr = stderr.ok().and_then(Result::ok).unwrap_or_default();
            let (code, signal) = (status.and_then(|s| s.code()), status.and_then(|s| s.signal()));
            let events = own
                .stream
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .exited(code, signal, &stderr);
            for event in events {
                let wide = matches!(event, Normalized::Exited { .. });
                send((!wide).then_some(&session), event);
            }
        });
        child::watch(reader, watched, handle.clone(), forget);
        Ok(handle)
    }
}

impl Driver for Claude {
    fn harness(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self, _process: &ProcessPart) -> Capabilities {
        Capabilities {
            sessions_per_instance: 1,
            instance_idle: Duration::ZERO,
            min_version: MINIMUM,
        }
    }

    fn account(&self, process: &ProcessPart) -> Option<String> {
        Some(part::account(&process.0))
    }

    fn split(&self, config: &EffectiveConfig, plan: &SessionPlan) -> Result<Split, Failure> {
        let process = part::process(config, plan, &self.state)?;
        Ok(Split::new("claude", ProcessPart(process), SessionPart(Value::Null)))
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

    /// EOF on stdin first (an idle claude exits within a second) when this daemon started it, then the group is
    /// terminated, then killed.
    fn stop_instance<'a>(&'a self, instance: &'a InstanceHandle, grace: Duration) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let known = self
                .instances
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&instance.id);
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
        _session: &'a SessionPart,
    ) -> BoxFuture<'a, Result<SessionRef, Failure>> {
        // The session id is a flag of the spawn: the session exists once its process does.
        Box::pin(async move { Ok(self.instance(&instance.id)?.session.clone()) })
    }

    fn resume_session<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        _session: &'a SessionRef,
        _part: &'a SessionPart,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move { self.instance(&instance.id).map(|_| ()) })
    }

    fn send_prompt<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        _session: &'a SessionRef,
        text: &'a str,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        // The key is the message uuid: claude acknowledges it and a resend of it starts no second turn.
        let line = json!({ "type": "user", "uuid": key.0.to_string(), "session_id": "", "parent_tool_use_id": null,
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] } });
        Box::pin(async move { self.write(instance, &line).await })
    }

    fn cancel_turn<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        _session: &'a SessionRef,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            known
                .stream
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .interrupted();
            let line = json!({ "type": "control_request", "request_id": Uuid::new_v4().to_string(),
                "request": { "subtype": "interrupt" } });
            self.write(instance, &line).await
        })
    }

    /// `stop_task` on every shell task: claude kills it, reports it stopped and comes to idle without starting a
    /// turn; a signal from outside would wake the model instead (experiment 8).
    fn stop_background<'a>(&'a self, instance: &'a InstanceHandle, _session: &'a SessionRef) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let Ok(known) = self.instance(&instance.id) else { return };
            let commands = known.stream.lock().unwrap_or_else(PoisonError::into_inner).commands();
            for task in commands {
                let line = json!({ "type": "control_request", "request_id": Uuid::new_v4().to_string(),
                    "request": { "subtype": "stop_task", "task_id": task } });
                let _ = self.write(instance, &line).await;
            }
        })
    }

    fn close_session<'a>(&'a self, _instance: &'a InstanceHandle, _session: &'a SessionRef) -> BoxFuture<'a, ()> {
        // The process is the session: the registry stops it (no idle time), the transcript stays.
        Box::pin(async {})
    }

    fn transcript<'a>(
        &'a self,
        _instance: &'a InstanceHandle,
        session: &'a SessionRef,
        key: PromptKey,
    ) -> BoxFuture<'a, Result<Transcript, Failure>> {
        Box::pin(async move { Ok(transcript(&session.0, key, &part::host_dir())) })
    }

    fn live(&self, session: &SessionRef) -> Option<broadcast::Receiver<Value>> {
        let instances = self.instances.lock().unwrap_or_else(PoisonError::into_inner);
        let id = &session.0["session_id"];
        instances
            .values()
            .find(|i| i.session.0["session_id"] == *id)
            .map(|i| i.live.subscribe())
    }
}

/// Where the prompt `key` stands in the session file `<config>/projects/<workdir, non-alphanumerics as ->/<id>.jsonl`;
/// the config directory of a session inheriting the host is `host`, the one its process got (`part::daemon_vars`).
fn transcript(session: &Value, key: PromptKey, host: &Path) -> Transcript {
    let config = session["config_dir"]
        .as_str()
        .map_or_else(|| host.to_owned(), PathBuf::from);
    let workdir = session["workdir"].as_str().unwrap_or_default();
    let dir: String = workdir
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let file = config
        .join("projects")
        .join(dir)
        .join(format!("{}.jsonl", session["session_id"].as_str().unwrap_or_default()));
    let Ok(text) = std::fs::read_to_string(file) else {
        return Transcript::Missing;
    };
    let records: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let key = key.0.to_string();
    let delivered = records.iter().any(|r| r["type"] == "user" && r["uuid"] == key.as_str());
    Transcript::Present { delivered }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "test processes run outside the daemon reaper")]
mod tests;
