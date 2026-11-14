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
use tokio::process::{Child, Command};

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
    let start_time = orphans::start_time(pid).ok_or_else(|| Failure::new(FailureKind::HarnessCrash, Source::Adapter))?;
    Ok((child, pid, start_time))
}

/// The environment every harness process starts from, nothing else inherited: the daemon's `vars` that are set,
/// the agent's `env` (a JSON map), the instance id the orphan sweep looks for and the API socket. The adapter adds
/// its own after these.
pub fn base_env<'a>(
    vars: impl Iterator<Item = &'a str>,
    env: &Value,
    id: &str,
    socket: Option<&Path>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vars
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_owned(), v)))
        .collect();
    let text = |v: &Value| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string());
    out.extend((env.as_object().into_iter().flatten()).map(|(k, v)| (k.clone(), text(v))));
    out.push((orphans::INSTANCE_ENV.to_owned(), id.to_owned()));
    out.extend(socket.map(|s| ("AAE_SOCKET".to_owned(), s.to_string_lossy().into_owned())));
    out
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
