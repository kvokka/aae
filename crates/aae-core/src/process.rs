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
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return spawn_failed(e.to_string()),
    };
    let group = child.id().and_then(|id| i32::try_from(id).ok()).and_then(Pid::from_raw);
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
        waited = timeout(spec.timeout, child.wait()) => waited.ok(),
        () = stopped => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    async fn sh(script: &str, stdin: &[u8], limit: Duration, keep: usize) -> Output {
        let command = ["sh".to_owned(), "-c".to_owned(), script.to_owned()];
        let env = [("PATH".to_owned(), std::env::var("PATH").unwrap_or_default())];
        run(Spec {
            command: &command,
            cwd: Path::new("/"),
            env: &env,
            stdin,
            timeout: limit,
            keep,
            stop: None,
        })
        .await
    }

    #[tokio::test]
    async fn exit_code_stdin_and_a_clean_environment() {
        let out = sh(
            "cat; echo \"[$HOME]\" >&2; exit 3",
            b"hello",
            Duration::from_secs(5),
            100,
        )
        .await;
        assert_eq!(out.exit, Exit::Code(3));
        assert_eq!(out.stdout, b"hello");
        assert_eq!(out.stderr, b"[]\n");
    }

    #[tokio::test]
    async fn only_the_tail_is_kept_and_every_byte_counted() {
        let out = sh("printf 0123456789", b"", Duration::from_secs(5), 4).await;
        assert_eq!((out.stdout.as_slice(), out.stdout_total), (&b"6789"[..], 10));
    }

    #[tokio::test]
    async fn a_timeout_ends_the_group_even_when_term_is_ignored() {
        let started = Instant::now();
        let out = sh("trap '' TERM; sleep 30 & sleep 30", b"", Duration::from_millis(200), 10).await;
        assert_eq!(out.exit, Exit::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_descendant_holding_the_pipes_does_not_hold_the_result() {
        let started = Instant::now();
        let out = sh("sleep 30 & echo done", b"", Duration::from_secs(5), 10).await;
        assert_eq!((out.exit, out.stdout.as_slice()), (Exit::Code(0), &b"done\n"[..]));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[tokio::test]
    async fn a_missing_program_is_a_spawn_failure() {
        let out = run(Spec {
            command: &["/nonexistent/aae-test".to_owned()],
            cwd: Path::new("/"),
            env: &[],
            stdin: b"",
            timeout: Duration::from_secs(1),
            keep: 10,
            stop: None,
        })
        .await;
        assert!(matches!(out.exit, Exit::SpawnFailed(_)));
    }
}
