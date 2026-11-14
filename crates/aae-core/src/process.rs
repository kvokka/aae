//! Runs an external command for an exec step (spec §9.1) or an orchestrator hook (docs/design/contracts.md 3.1):
//! a process group of its own, a clean environment, a timeout that ends the whole group, bounded output.

use rustix::process::{Pid, Signal, kill_process_group};
use std::{
    os::unix::process::ExitStatusExt,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Notify,
    time::timeout,
};

/// Between `SIGTERM` and `SIGKILL` to the group.
const GRACE: Duration = Duration::from_secs(2);
/// How long the pipes may stay open once the leader has exited (a descendant may hold them).
const DRAIN: Duration = Duration::from_secs(1);

pub(crate) struct Spec<'a> {
    /// The argv; `command[0]` is looked up on the `PATH` of `env`.
    pub(crate) command: &'a [String],
    pub(crate) cwd: &'a Path,
    /// The whole environment of the command: nothing is inherited.
    pub(crate) env: &'a [(String, String)],
    /// Written to stdin, then EOF.
    pub(crate) stdin: &'a [u8],
    pub(crate) timeout: Duration,
    /// Bytes kept from the end of stdout and of stderr each.
    pub(crate) keep: usize,
    /// Ends the command early, as its timeout would.
    pub(crate) stop: Option<&'a Notify>,
}

/// What a command gets from the daemon's environment: nothing else is inherited.
pub(crate) fn base_env() -> Vec<(String, String)> {
    ["PATH", "HOME", "LANG"]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_owned(), v)))
        .collect()
}

/// How a command ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
    /// The timeout passed, or the command was stopped.
    TimedOut,
    SpawnFailed(String),
}

#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) exit: Exit,
    /// The last `keep` bytes of stdout.
    pub(crate) stdout: Vec<u8>,
    /// Every byte stdout produced, kept or not.
    pub(crate) stdout_total: u64,
    /// The last `keep` bytes of stderr.
    pub(crate) stderr: Vec<u8>,
}

#[derive(Default)]
struct Tail {
    bytes: Vec<u8>,
    total: u64,
}

type Shared = Arc<Mutex<Tail>>;

/// Runs the command to its end or its timeout; it returns no later than `timeout` + 4 s.
pub(crate) async fn run(spec: Spec<'_>) -> Output {
    let Some((program, args)) = spec.command.split_first() else {
        return spawn_failed("empty command".to_owned());
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(spec.cwd)
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = match crate::orphans::spawn(&mut command) {
        Ok(child) => child,
        Err(e) => return spawn_failed(e.to_string()),
    };
    let pid = child.id();
    let _own = Own(pid);
    let group = pid.and_then(|id| i32::try_from(id).ok()).and_then(Pid::from_raw);
    let signal = |sig| {
        if let Some(group) = group {
            let _ = kill_process_group(group, sig);
        }
    };
    let (out, err) = (Shared::default(), Shared::default());
    let input = spec.stdin.to_vec();
    let stdin = child.stdin.take();
    let pumps = (
        pump(child.stdout.take(), spec.keep, out.clone()),
        pump(child.stderr.take(), spec.keep, err.clone()),
    );
    let mut io = tokio::spawn(async move {
        let feed = async move {
            // A command that does not read its stdin closes the pipe: that is its business.
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(&input).await;
            }
        };
        tokio::join!(feed, pumps.0, pumps.1);
    });
    let stopped = async {
        match spec.stop {
            Some(stop) => stop.notified().await,
            None => std::future::pending().await,
        }
    };
    let waited = tokio::select! {
        // A stop goes first, then the process's own end or its timeout: not at random, so a seed replays.
        biased;
        () = stopped => None,
        waited = timeout(spec.timeout, child.wait()) => waited.ok(),
    };
    let exit = match waited {
        Some(Ok(status)) => match (status.code(), status.signal()) {
            (Some(code), _) => Exit::Code(code),
            (None, signal) => Exit::Signal(signal.unwrap_or_default()),
        },
        Some(Err(e)) => Exit::SpawnFailed(e.to_string()),
        None => {
            signal(Signal::TERM);
            if timeout(GRACE, child.wait()).await.is_err() {
                signal(Signal::KILL);
                let _ = child.wait().await;
            }
            Exit::TimedOut
        }
    };
    if timeout(DRAIN, &mut io).await.is_err() {
        signal(Signal::KILL);
        // A descendant that left the group keeps the pipes: its output is cut where it is.
        if timeout(DRAIN, &mut io).await.is_err() {
            io.abort();
        }
    }
    let take = |tail: &Shared| std::mem::take(&mut *tail.lock().unwrap_or_else(PoisonError::into_inner));
    let (out, err) = (take(&out), take(&err));
    Output {
        exit,
        stdout: out.bytes,
        stdout_total: out.total,
        stderr: err.bytes,
    }
}

fn spawn_failed(text: String) -> Output {
    Output {
        exit: Exit::SpawnFailed(text),
        stdout: Vec::new(),
        stdout_total: 0,
        stderr: Vec::new(),
    }
}

async fn pump(from: Option<impl AsyncRead + Unpin>, keep: usize, into: Shared) {
    let Some(mut from) = from else { return };
    let mut chunk = [0u8; 8192];
    while let Ok(n @ 1..) = from.read(&mut chunk).await {
        let mut tail = into.lock().unwrap_or_else(PoisonError::into_inner);
        tail.total += n as u64;
        tail.bytes.extend_from_slice(&chunk[..n]);
        let cut = tail.bytes.len().saturating_sub(keep);
        tail.bytes.drain(..cut);
    }
}

/// Waits until the process `pid` started at `start_time` is gone (spec §5.2); `false` if it outlives `limit`.
pub(crate) async fn wait_dead(pid: i32, start_time: u64, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    while crate::orphans::start_time(pid) == Some(start_time) {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    true
}

/// Waits until nothing is left of the instance `id` (invariants 6, 7): its process `leader` (pid, start time)
/// gets `limit` to end by itself, then every process still carrying the marker is killed. `false` past `limit`.
pub(crate) async fn wait_gone(id: &str, leader: Option<(i32, u64)>, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    if let Some((pid, start_time)) = leader
        && !wait_dead(pid, start_time, limit).await
    {
        return false;
    }
    // A killed process loses its environment, so its marker, before it is dead: what was killed is awaited by pid.
    let (mut killed, mut walks) = (Vec::new(), Walks::default());
    loop {
        let marker = id.to_owned();
        // Walking `/proc` is synchronous.
        let found = tokio::task::spawn_blocking(move || crate::orphans::sweep(&marker)).await;
        killed.extend(found.unwrap_or_default());
        killed.retain(|&(pid, start)| crate::orphans::start_time(pid) == Some(start));
        let (left, past) = (!killed.is_empty(), tokio::time::Instant::now() >= deadline);
        if let Some(gone) = walks.after(left, past) {
            return gone;
        }
        tokio::time::sleep(Duration::from_millis(if walks.0 { 20 } else { 50 })).await;
    }
}

/// The walks of `/proc` in [`wait_gone`], holding whether the last one found nothing. A process inside `execve`
/// shows no environment yet: a walk that found nothing ends the wait only after the walk before found nothing too.
#[derive(Default)]
struct Walks(bool);

impl Walks {
    /// After a walk that `left` killed processes alive: `Some(gone)` ends the wait, `None` walks again.
    fn after(&mut self, left: bool, past_deadline: bool) -> Option<bool> {
        match (left, std::mem::replace(&mut self.0, !left)) {
            (false, true) => Some(true),
            (false, false) => None,
            (true, _) => past_deadline.then_some(false),
        }
    }
}

/// Tells the orphan reaper the child is no longer its own however [`run`] ends, a dropped future included.
struct Own(Option<u32>);

impl Drop for Own {
    fn drop(&mut self) {
        crate::orphans::waited(self.0);
    }
}

#[cfg(test)]
mod tests;
