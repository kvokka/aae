//! The process of a service (spec §11.4): started by a `start_service` effect, ready once its health check passes,
//! ended by a `stop_service` effect along with every process that carries its marker.

use super::Msg;
use crate::{
    config::Health,
    orphans::INSTANCE_ENV,
    process::{self, Exit, Spec},
    writer::CoreHandle,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Notify,
    task::JoinHandle,
};

/// The placeholder for the port the daemon picked, in a service's `command`.
pub(super) const PORT: &str = "${AAE_SERVICE_PORT}";
/// How long a service may take to pass its health check, and a launch waits for it.
pub(super) const READY_WAIT: Duration = Duration::from_secs(30);
/// How long one health check may take.
const CHECK_WAIT: Duration = Duration::from_secs(5);
/// Between two health checks of a starting service.
const CHECK_EVERY: Duration = Duration::from_millis(100);
/// Bytes kept from the end of a service's stderr for its `service.failed`.
const TAIL: usize = 4 * 1024;

/// The services this daemon runs, by id.
pub(super) type Running = Arc<Mutex<HashMap<String, Entry>>>;

/// The latest start of a service: its stop, its task, its port and token, and whether it passed its health check.
pub(super) struct Entry {
    stop: Arc<Notify>,
    task: JoinHandle<()>,
    port: u16,
    token: String,
    ready: Arc<AtomicBool>,
}

/// What stands for the port of the service `id` in a session's config until its launch: the port may change
/// before the service is first ready.
pub(super) fn port_of(id: &str) -> String {
    format!("${{AAE_SERVICE_PORT:{id}}}")
}

/// What stands for the token of the service `id` in a session's config until its launch: a service that moves to
/// a new port gets a new token.
pub(super) fn token_of(id: &str) -> String {
    format!("${{AAE_SERVICE_TOKEN:{id}}}")
}

/// Runs the service of a `start_service` effect in a task of its own: `ServiceReady` once it is healthy,
/// `ServiceFailed` once it ends unless the registry stopped it.
pub(super) fn start(running: &Running, core: &CoreHandle<Msg>, p: &Value) {
    let id = p["service"].as_str().unwrap_or_default().to_owned();
    let port = p["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or_default();
    let command: Vec<String> = serde_json::from_value(p["command"].clone()).unwrap_or_default();
    let command: Vec<String> = command.iter().map(|a| a.replace(PORT, &port.to_string())).collect();
    let health: Health = serde_json::from_value(p["health"].clone()).unwrap_or(Health::Port);
    let token = p["token"].as_str().unwrap_or_default().to_owned();
    let pairs: BTreeMap<String, String> = serde_json::from_value(p["env"].clone()).unwrap_or_default();
    let mut env = process::base_env();
    env.retain(|(k, _)| !pairs.contains_key(k));
    env.extend(pairs);
    env.extend([
        ("AAE_SERVICE_PORT".to_owned(), port.to_string()),
        ("AAE_SERVICE_TOKEN".to_owned(), token.clone()),
        (INSTANCE_ENV.to_owned(), id.clone()),
    ]);
    let delay = Duration::from_millis(p["delay_ms"].as_u64().unwrap_or_default());
    let (stop, is_up) = (Arc::new(Notify::new()), Arc::new(AtomicBool::new(false)));
    let (core, own, up_flag) = (core.clone(), stop.clone(), is_up.clone());
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = own.notified() => return,
        }
        // Starting on a port someone else listens on fails the start: the core picks another. One who takes it
        // while the service starts does not pass its health check (`healthy`), so the service fails to bind.
        if let Err(e) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            let stderr = format!("port {port} is taken: {e}");
            let failed = Msg::ServiceFailed {
                id: task_id,
                stderr,
                taken: true,
            };
            let _ = core.send(None, failed).await;
            return;
        }
        let out = process::run(Spec {
            command: &command,
            cwd: Path::new("/"),
            env: &env,
            stdin: b"",
            timeout: Duration::MAX,
            keep: TAIL,
            stop: Some(&own),
        });
        tokio::pin!(out);
        let ended = tokio::select! {
            out = &mut out => Some(out),
            up = ready(&health, &task_id, port, &token, &env) => {
                if up {
                    up_flag.store(true, Ordering::Release);
                    let _ = core.send(None, Msg::ServiceReady { id: task_id.clone() }).await;
                } else {
                    own.notify_one();
                }
                None
            }
        };
        let out = match ended {
            Some(out) => out,
            None => out.await,
        };
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        // After a stop of the registry's the core drops it: the service is `stopping`.
        let failed = Msg::ServiceFailed {
            id: task_id,
            stderr,
            taken: false,
        };
        let _ = core.send(None, failed).await;
    });
    let entry = Entry {
        stop,
        task,
        port,
        token: p["token"].as_str().unwrap_or_default().to_owned(),
        ready: is_up,
    };
    if let Some(old) = lock(running).insert(id, entry) {
        old.stop.notify_one();
    }
}

/// `payload` with the port and token of each of `services` in place of [`port_of`] and [`token_of`], once each is
/// ready, or port 0 and no token past [`READY_WAIT`]: a session connects to its services as it starts, one not up by
/// then is a tool failure, and the port it was not ready on may be someone else's, who would get the token.
pub(super) async fn resolve(running: &Running, services: &[(String, u16)], payload: &Value) -> Value {
    let start = tokio::time::Instant::now();
    let mut text = payload.to_string();
    for (id, _) in services {
        let (port, token) = loop {
            let now = lock(running)
                .get(id)
                .filter(|e| e.ready.load(Ordering::Acquire))
                .map(|e| (e.port, e.token.clone()));
            match now {
                Some(ready) => break ready,
                _ if start.elapsed() > READY_WAIT => break (0, String::new()),
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        text = text
            .replace(&port_of(id), &port.to_string())
            .replace(&token_of(id), &token);
    }
    serde_json::from_str(&text).unwrap_or_else(|_| payload.clone())
}

/// Checks `health` until it passes, up to [`READY_WAIT`]; `false` past it.
async fn ready(health: &Health, id: &str, port: u16, token: &str, env: &[(String, String)]) -> bool {
    let start = tokio::time::Instant::now();
    while !healthy(health, id, port, token, env).await {
        if start.elapsed() > READY_WAIT {
            return false;
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
    true
}

/// A port or HTTP check counts only once the service alone listens on its port: one who took the port while it
/// started would pass it and get the token. A command checks what it likes.
async fn healthy(health: &Health, id: &str, port: u16, token: &str, env: &[(String, String)]) -> bool {
    if matches!(health, Health::Port | Health::Http(_)) {
        let id = id.to_owned();
        // Walking `/proc` is synchronous.
        let alone = tokio::task::spawn_blocking(move || crate::orphans::listens_alone(&id, port)).await;
        if !alone.unwrap_or(false) {
            return false;
        }
    }
    match health {
        Health::Port => TcpStream::connect(("127.0.0.1", port)).await.is_ok(),
        Health::Http(path) => tokio::time::timeout(CHECK_WAIT, http_ok(port, path, token))
            .await
            .unwrap_or(false),
        Health::Command(command) => {
            let out = process::run(Spec {
                command,
                cwd: Path::new("/"),
                env,
                stdin: b"",
                timeout: CHECK_WAIT,
                keep: 0,
                stop: None,
            })
            .await;
            out.exit == Exit::Code(0)
        }
    }
}

/// `GET path` on loopback `port` with the service token answers 2xx; HTTP by hand, no client crate for one line.
async fn http_ok(port: u16, path: &str, token: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)).await else {
        return false;
    };
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    let mut status = [0u8; 12];
    stream.write_all(request.as_bytes()).await.is_ok()
        && stream.read_exact(&mut status).await.is_ok()
        && status.starts_with(b"HTTP/1.")
        && status[9] == b'2'
}

/// Ends the service of a `stop_service` effect and every process that carries its marker.
pub(super) async fn stop(running: &Running, id: &str) {
    let entry = lock(running).remove(id);
    if let Some(entry) = entry {
        entry.stop.notify_one();
        let _ = entry.task.await;
    }
    let id = id.to_owned();
    // Walking `/proc` is synchronous.
    let _ = tokio::task::spawn_blocking(move || crate::orphans::sweep(&id)).await;
}

fn lock(running: &Running) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
    running.lock().unwrap_or_else(PoisonError::into_inner)
}
