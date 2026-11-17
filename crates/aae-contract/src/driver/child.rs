//! The harness process of an adapter, run the same way by every adapter (docs/design/adapters.md): its own process
//! group with the instance marker, its environment base, its event sender and its stop ladder.

use super::{DriverEvent, EventSink, Failure, FailureKind, Health, InstanceHandle, Normalized, SessionRef, Source};
use crate::orphans;
use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::Value;
use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    process::{Child, Command},
    task::JoinHandle,
};

/// A `config_error` failure with the harness text `raw`.
pub fn config_error(raw: String) -> Failure {
    let mut failure = Failure::new(FailureKind::ConfigError, Source::Adapter);
    failure.raw = raw;
    failure
}

/// Spawns `command` of `binary` with piped stdio in a process group of its own, and the identity of the process: pid
/// and kernel start time. The caller set its arguments, environment and directory.
pub fn spawn(command: &mut Command, binary: &Path) -> Result<(Child, i32, u64), Failure> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let child = orphans::spawn(command).map_err(|e| config_error(format!("{}: {e}", binary.display())))?;
    let pid = child.id().and_then(|p| i32::try_from(p).ok()).unwrap_or_default();
    let start_time =
        orphans::start_time(pid).ok_or_else(|| Failure::new(FailureKind::HarnessCrash, Source::Adapter))?;
    Ok((child, pid, start_time))
}

/// The environment every harness process starts from, nothing else inherited: the daemon's `vars` that are set,
/// the agent's `env` (a JSON map of `env` values, its references resolved here and nowhere earlier), the instance
/// id the orphan sweep looks for and the API socket. The adapter adds its own after these. A reference to a
/// variable the daemon lacks is a `config_error`.
pub fn base_env<'a>(
    vars: impl Iterator<Item = &'a str>,
    env: &Value,
    id: &str,
    socket: Option<&Path>,
) -> Result<Vec<(String, String)>, Failure> {
    let mut out: Vec<(String, String)> = vars
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_owned(), v)))
        .collect();
    let text = |v: &Value| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string());
    for (k, v) in env.as_object().into_iter().flatten() {
        let value = crate::config::env_value(&text(v), |n| std::env::var(n).ok());
        out.push((k.clone(), value.map_err(|e| config_error(format!("env `{k}`: {e}")))?));
    }
    out.push((orphans::INSTANCE_ENV.to_owned(), id.to_owned()));
    out.extend(socket.map(|s| ("AAE_SOCKET".to_owned(), s.to_string_lossy().into_owned())));
    Ok(out)
}

/// The event sender of one instance; `seq` counts from 1 across every clone.
#[derive(Clone)]
pub struct Events {
    instance: String,
    seq: Arc<AtomicU64>,
    sink: EventSink,
}

impl Events {
    pub fn new(instance: &str, sink: EventSink) -> Self {
        Self {
            instance: instance.to_owned(),
            seq: Arc::default(),
            sink,
        }
    }

    /// `session` is `None` for an instance-wide event.
    pub fn send(&self, session: Option<SessionRef>, event: Normalized) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let instance = self.instance.clone();
        let _ = self.sink.send(DriverEvent {
            instance,
            session,
            seq,
            event,
        });
    }
}

/// Whether the process the handle names is still the one running.
pub fn alive(instance: &InstanceHandle) -> bool {
    orphans::start_time(instance.pid) == Some(instance.start_time)
}

pub fn health(instance: &InstanceHandle) -> Health {
    match alive(instance) {
        true => Health::Ok,
        false => Health::Dead,
    }
}

/// The stop ladder once the adapter closed the process's stdin (`eof`): wait, terminate the group, wait, kill it,
/// each wait half of `grace`. Without `eof` (an instance of a dead daemon has no stdin here) it starts at the
/// signals.
pub async fn stop(instance: &InstanceHandle, grace: Duration, eof: bool) {
    let signals = [None, Some(Signal::TERM), Some(Signal::KILL)];
    for signal in signals.into_iter().skip(usize::from(!eof)) {
        if let (Some(signal), Some(group)) = (signal, Pid::from_raw(instance.pgid)) {
            let _ = kill_process_group(group, signal);
        }
        let until = Instant::now() + grace / 2;
        while alive(instance) && Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if !alive(instance) {
            return;
        }
    }
}

/// Watches the task that reads the harness output of `instance`: its panic is the adapter's crash
/// (docs/design/contracts.md 4). Every session of the instance gets `adapter_crash`, `forget` drops the instance from
/// the adapter, the process group is killed, and the instance is reported gone.
pub fn watch(reader: JoinHandle<()>, events: Events, instance: InstanceHandle, forget: impl FnOnce() + Send + 'static) {
    tokio::spawn(async move {
        let Err(Ok(panic)) = reader.await.map_err(tokio::task::JoinError::try_into_panic) else {
            return;
        };
        let text = (panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        forget();
        let mut failure = Failure::new(FailureKind::AdapterCrash, Source::Adapter);
        failure.raw = format!("the adapter's reader of instance {} panicked: {text}", instance.id);
        events.send(None, Normalized::Failure(failure));
        if let Some(group) = Pid::from_raw(instance.pgid) {
            let _ = kill_process_group(group, Signal::KILL);
        }
        let signal = Some(Signal::KILL.as_raw());
        events.send(None, Normalized::Exited { code: None, signal });
    });
}

#[cfg(test)]
mod tests;
