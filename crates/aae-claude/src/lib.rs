//! The claude adapter (spec §7, docs/design/contracts.md 4.6): one `claude -p` process per session, stream-json
//! on stdin and stdout.

mod part;
mod stream;

use aae_core::{
    BoxFuture,
    config::EffectiveConfig,
    driver::{
        Answer, Capabilities, Driver, DriverEvent, EventSink, Failure, FailureKind, Health, InstanceHandle, Normalized,
        ProcessPart, PromptKey, PromptState, SessionPart, SessionPlan, SessionRef, Source, Split, Transcript,
    },
    orphans,
};
use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::Stdio,
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

/// The claude driver; `state` holds the config directories of isolated sessions, `socket` is the API socket a
/// session gets in `AAE_SOCKET`.
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
    /// The input of each open permission request: `allow` hands it back.
    requests: Arc<Mutex<HashMap<String, Value>>>,
    live: broadcast::Sender<Value>,
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
        let instance = self.instance(&instance.id)?;
        let mut stdin = instance.stdin.lock().await;
        let crashed = || Failure::new(FailureKind::HarnessCrash, Source::Adapter);
        let stdin = stdin.as_mut().ok_or_else(crashed)?;
        let text = format!("{line}\n");
        stdin.write_all(text.as_bytes()).await.map_err(|_| crashed())?;
        stdin.flush().await.map_err(|_| crashed())
    }

    async fn start(&self, id: &str, p: &Value, sink: EventSink) -> Result<InstanceHandle, Failure> {
        let mut command = Command::new(&self.binary);
        command
            .args(part::args(p))
            .env_clear()
            .envs(part::env(p, id, self.socket.as_deref(), &part::host_dir()))
            .current_dir(p["workdir"].as_str().unwrap_or("."))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let config_error = |what: &Path, e: std::io::Error| {
            let mut failure = Failure::new(FailureKind::ConfigError, Source::Adapter);
            failure.raw = format!("{}: {e}", what.display());
            failure
        };
        let files = Path::new(p["files"].as_str().unwrap_or_default());
        part::materialize(p).map_err(|e| config_error(files, e))?;
        if let Some(dir) = p["config_dir"].as_str() {
            std::fs::create_dir_all(dir).map_err(|e| config_error(Path::new(dir), e))?;
        }
        let mut child = orphans::spawn(&mut command).map_err(|e| config_error(&self.binary, e))?;
        let pid = child.id().and_then(|p| i32::try_from(p).ok()).unwrap_or_default();
        let crashed = Failure::new(FailureKind::HarnessCrash, Source::Adapter);
        let start_time = orphans::start_time(pid).ok_or(crashed)?;
        let session = SessionRef(json!({
            "session_id": p["session_id"], "config_dir": p["config_dir"], "workdir": p["workdir"],
        }));
        let stream = Stream::new(Some(part::account(p)), p["model"].as_str().map(str::to_owned));
        let instance = Arc::new(Instance {
            session: session.clone(),
            stdin: tokio::sync::Mutex::new(child.stdin.take()),
            stream: Arc::new(Mutex::new(stream)),
            requests: Arc::default(),
            live: broadcast::channel(256).0,
        });
        let (stream, requests, live) = (
            instance.stream.clone(),
            instance.requests.clone(),
            instance.live.clone(),
        );
        let mut lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
        let stderr = tokio::spawn(aae_core::driver::stderr_tail(
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
        tokio::spawn(async move {
            let mut seq = 0;
            let mut send = |session: Option<&SessionRef>, event: Normalized| {
                seq += 1;
                let _ = sink.send(DriverEvent {
                    instance: id.clone(),
                    session: session.cloned(),
                    seq,
                    event,
                });
            };
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let _ = live.send(msg.clone());
                let events = stream
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .line(&msg, Instant::now());
                for event in events {
                    if let Normalized::PermissionRequested { request_id, input, .. } = &event {
                        let mut requests = requests.lock().unwrap_or_else(PoisonError::into_inner);
                        requests.insert(request_id.clone(), input.clone());
                    }
                    send(Some(&session), event);
                }
            }
            let status = child.wait().await.ok();
            orphans::waited(u32::try_from(pid).ok());
            instances.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            let stderr = tokio::time::timeout(Duration::from_secs(1), stderr).await;
            let stderr = stderr.ok().and_then(Result::ok).unwrap_or_default();
            let (code, signal) = (status.and_then(|s| s.code()), status.and_then(|s| s.signal()));
            let events = stream
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .exited(code, signal, &stderr);
            for event in events {
                let wide = matches!(event, Normalized::Exited { .. });
                send((!wide).then_some(&session), event);
            }
        });
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
            // The versions the contract tests and experiments ran on, inclusive.
            versions: [(2, 1, 322), (2, 1, 325)],
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
        Box::pin(async move {
            match orphans::start_time(instance.pid) == Some(instance.start_time) {
                true => Health::Ok,
                false => Health::Dead,
            }
        })
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
            let alive = || orphans::start_time(instance.pid) == Some(instance.start_time);
            let signals = [None, Some(Signal::TERM), Some(Signal::KILL)];
            for signal in signals.into_iter().skip(usize::from(!eof)) {
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

    fn respond<'a>(
        &'a self,
        instance: &'a InstanceHandle,
        _session: &'a SessionRef,
        request_id: &'a str,
        answer: &'a Answer,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let known = self.instance(&instance.id)?;
            let input = known
                .requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(request_id);
            let response = match answer {
                Answer::Allow { updated_input } => {
                    json!({ "behavior": "allow", "updatedInput": updated_input.clone().or(input).unwrap_or(json!({})) })
                }
                Answer::Deny { message } => json!({ "behavior": "deny", "message": message }),
                // A denial's message reaches the model as the tool result (experiment 1a): the answer is read.
                Answer::Text { text } => json!({ "behavior": "deny", "message": text }),
            };
            let line = json!({ "type": "control_response",
                "response": { "subtype": "success", "request_id": request_id, "response": response } });
            self.write(instance, &line).await
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
        Box::pin(async move { Ok(transcript(&session.0, key)) })
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

/// Where the prompt `key` stands in the session file `<config>/projects/<workdir, non-alphanumerics as ->/<id>.jsonl`.
fn transcript(session: &Value, key: PromptKey) -> Transcript {
    let config = match session["config_dir"].as_str() {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(&std::env::var("HOME").unwrap_or_default()).join(".claude"),
    };
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
    let prompt = match records
        .iter()
        .position(|r| r["type"] == "user" && r["uuid"] == key.as_str())
    {
        None => PromptState::Absent,
        Some(at) if records[at..].iter().any(|r| r["type"] == "assistant") => PromptState::Answered,
        Some(_) => PromptState::Unanswered,
    };
    Transcript::Present { prompt }
}

#[cfg(test)]
mod tests;
