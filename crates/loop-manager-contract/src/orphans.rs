//! No orphans (docs/design/invariants.md, invariant 7; docs/design/experiments.md "Descendants after `kill -9`"): every
//! instance process and its descendants carry `LOOP_MANAGER_INSTANCE=<id>` and are killed by it once the instance is gone; the
//! daemon is the child subreaper, so descendants that dropped the marker are reaped by it, not left as zombies.

// shortcut: Linux only (`/proc`, `PR_SET_CHILD_SUBREAPER`); elsewhere nothing is found or adopted and a process is
// told by its pid alone (`start_time`) — macOS ships but is not checked in the first release (docs/overview.md).

use rustix::process::{Pid, Signal, WaitOptions, getpid, kill_process, waitpid};
use std::{
    collections::BTreeSet,
    io,
    process::{Output, Stdio},
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::process::{Child, Command};

/// The variable in the environment of every instance process: its instance id.
pub const INSTANCE_ENV: &str = "LOOP_MANAGER_INSTANCE";

/// How often adopted orphans that exited are reaped.
const REAP_EVERY: Duration = Duration::from_secs(5);

/// The children the daemon spawned itself: their `wait` reaps them, the orphan reaper leaves them alone.
static OWN: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());

fn own() -> MutexGuard<'static, BTreeSet<i32>> {
    OWN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Spawns `command` as a child of the daemon's own; [`waited`] once its `wait` returned. Every child of the daemon
/// is spawned here or in [`spawn_std`] (`clippy.toml`): the reaper would take the exit of any other from its `wait`.
#[allow(clippy::disallowed_methods)]
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    // Held across the spawn: a child that exits at once is not reaped before it is known.
    let mut own = own();
    let child = command.spawn()?;
    own.extend(child.id().and_then(|id| i32::try_from(id).ok()));
    Ok(child)
}

/// [`spawn`] for a `std` command; the child is the daemon's own until the [`Own`] is dropped, once it was waited for.
#[allow(clippy::disallowed_methods)]
pub fn spawn_std(command: &mut std::process::Command) -> io::Result<(std::process::Child, Own)> {
    let mut own = own();
    let child = command.spawn()?;
    let id = child.id();
    own.extend(i32::try_from(id).ok());
    Ok((child, Own(Some(id))))
}

/// `command.output()` with the child the daemon's own while it runs.
pub async fn output(command: &mut Command) -> io::Result<Output> {
    let child = spawn(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    // Dropped after the wait, or with it when the caller gives up and the child is reaped by whoever comes first.
    let _own = Own(child.id());
    child.wait_with_output().await
}

/// `command.output()` for a `std` command, with the child the daemon's own while it runs.
pub fn output_std(command: &mut std::process::Command) -> io::Result<Output> {
    let (child, _own) = spawn_std(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    child.wait_with_output()
}

/// A child of the daemon's own, left to its `wait` by the reaper until this is dropped.
pub struct Own(Option<u32>);

impl Drop for Own {
    fn drop(&mut self) {
        waited(self.0);
    }
}

/// The child `pid` from [`spawn`] has been waited for.
pub fn waited(pid: Option<u32>) {
    if let Some(pid) = pid.and_then(|id| i32::try_from(id).ok()) {
        own().remove(&pid);
    }
}

/// Makes this process the child subreaper and reaps, from now on, the orphans it adopts once they exit.
pub fn adopt_orphans() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    rustix::process::set_child_subreaper(Some(getpid()))?;
    tokio::spawn(async {
        loop {
            tokio::time::sleep(REAP_EVERY).await;
            reap();
        }
    });
    Ok(())
}

/// The kernel start time of the live process `pid` (field 22 of `/proc/<pid>/stat`); `None` for a zombie or a
/// process that is gone. pid + start time identify a process (docs/design/event-model.md).
#[cfg(target_os = "linux")]
pub fn start_time(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    (fields.next()? != "Z").then_some(())?;
    fields.nth(18)?.parse().ok()
}

/// shortcut: without `/proc` every existing process starts at 0, so a reused pid passes for the old process and a
/// zombie for a live one; read `kinfo_proc` through `sysctl` once macOS is checked (docs/overview.md).
#[cfg(not(target_os = "linux"))]
pub fn start_time(pid: i32) -> Option<u64> {
    rustix::process::test_kill_process(Pid::from_raw(pid)?).ok().map(|()| 0)
}

/// `(pid, state, ppid)` of every process in `/proc`.
fn processes() -> impl Iterator<Item = (i32, String, i32)> {
    let entries = std::fs::read_dir("/proc").into_iter().flatten().flatten();
    entries.filter_map(|e| {
        let pid: i32 = e.file_name().to_str()?.parse().ok()?;
        let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
        let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
        let state = fields.next()?.to_owned();
        Some((pid, state, fields.next()?.parse().ok()?))
    })
}

/// Reaps the exited children of this process that it did not spawn itself; how many.
pub(crate) fn reap() -> usize {
    let me = getpid().as_raw_nonzero().get();
    let own = own();
    processes()
        .filter(|(pid, state, ppid)| state == "Z" && *ppid == me && !own.contains(pid))
        .filter_map(|(pid, ..)| waitpid(Pid::from_raw(pid), WaitOptions::NOHANG).ok().flatten())
        .count()
}

/// The value of `name` in the environment of process `pid`.
fn var(pid: i32, name: &str) -> Option<String> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{name}=");
    let var = environ
        .split(|b| *b == 0)
        .find_map(|var| var.strip_prefix(prefix.as_bytes()))?;
    String::from_utf8(var.to_vec()).ok()
}

/// Live processes but this one, with their value of `name`.
fn carrying(name: &str) -> impl Iterator<Item = (i32, String)> {
    let me = getpid().as_raw_nonzero().get();
    processes()
        .filter(move |(pid, state, _)| *pid != me && state != "Z")
        .filter_map(move |(pid, ..)| Some((pid, var(pid, name)?)))
}

/// Live processes but this one, with their `LOOP_MANAGER_INSTANCE` marker.
fn marked() -> impl Iterator<Item = (i32, String)> {
    carrying(INSTANCE_ENV)
}

/// Kills every process but this one whose environment carries `name=value`, as a token of one attempt; how many it
/// found.
pub fn sweep_var(name: &str, value: &str) -> usize {
    let found = carrying(name).filter(|(_, v)| v == value);
    found
        .filter_map(|(pid, _)| Pid::from_raw(pid))
        .inspect(|pid| {
            let _ = kill_process(*pid, Signal::KILL);
        })
        .count()
}

/// Kills every process but this one whose environment carries `LOOP_MANAGER_INSTANCE=<id>`; the ones it found as (pid, start
/// time), so one it could not kill does not pass for dead.
pub fn sweep(id: &str) -> Vec<(i32, u64)> {
    sweep_all(&BTreeSet::from([id.to_owned()]))
}

/// [`sweep`] for every id of `ids` in one walk of `/proc`.
pub fn sweep_all(ids: &BTreeSet<String>) -> Vec<(i32, u64)> {
    marked()
        .filter(|(_, marker)| ids.contains(marker))
        .filter_map(|(pid, _)| Some((Pid::from_raw(pid)?, start_time(pid)?)))
        .map(|(pid, start)| {
            let _ = kill_process(pid, Signal::KILL);
            (pid.as_raw_nonzero().get(), start)
        })
        .collect()
}

/// Kills every descendant of this process. On the daemon's stop this ends what its steps, hooks and instances
/// started, which would otherwise run unwatched until the next start sweeps it (invariant 7); as the child
/// subreaper, the daemon still has the ones that left their group or their parent. A walk repeats until it finds
/// none alive, as a dying process may fork meanwhile.
pub fn kill_descendants() {
    let me = getpid().as_raw_nonzero().get();
    for _ in 0..10 {
        let all: Vec<_> = processes().collect();
        let mut ours = BTreeSet::from([me]);
        loop {
            let more = all
                .iter()
                .filter(|(pid, _, ppid)| ours.contains(ppid) && !ours.contains(pid));
            let more: Vec<i32> = more.map(|(pid, ..)| *pid).collect();
            if more.is_empty() {
                break;
            }
            ours.extend(more);
        }
        let live = all
            .iter()
            .filter(|(pid, state, _)| *pid != me && state != "Z" && ours.contains(pid));
        let live: Vec<Pid> = live.filter_map(|(pid, ..)| Pid::from_raw(*pid)).collect();
        if live.is_empty() {
            return;
        }
        for pid in live {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
}

/// `port` on loopback is listened on, and only by processes marked `id`: a port check passed by anyone else would
/// hand them the sessions' token (docs/design/config.md). Only listeners a connection to `127.0.0.1` may reach count.
#[cfg(target_os = "linux")]
pub fn listens_alone(id: &str, port: u16) -> bool {
    let tables = ["/proc/net/tcp", "/proc/net/tcp6"].map(|table| std::fs::read_to_string(table).unwrap_or_default());
    let listening: BTreeSet<String> = tables
        .iter()
        .flat_map(|table| table.lines().skip(1))
        .filter_map(|line| listener(line, port))
        .collect();
    let fds = marked().filter(|(_, marker)| marker == id).flat_map(|(pid, _)| {
        std::fs::read_dir(format!("/proc/{pid}/fd"))
            .into_iter()
            .flatten()
            .flatten()
    });
    let owned: BTreeSet<String> = fds
        .filter_map(|fd| Some(std::fs::read_link(fd.path()).ok()?.to_str()?.to_owned()))
        .filter(|link| listening.contains(link))
        .collect();
    !listening.is_empty() && owned == listening
}

/// shortcut: without `/proc` whoever listens on the port passes for the service (macOS is not checked in the first
/// release, docs/overview.md); read the sockets through `proc_pidfdinfo` once macOS is checked.
#[cfg(not(target_os = "linux"))]
pub fn listens_alone(_id: &str, _port: u16) -> bool {
    true
}

/// The local addresses, as `/proc/net/tcp*` prints them, a connection to `127.0.0.1` may reach: itself, `0.0.0.0`,
/// `::` (unless it is IPv6-only, which `/proc` does not tell: it counts) and `::ffff:127.0.0.1`.
#[cfg(target_os = "linux")]
const REACHED: [&str; 4] = [
    "0100007F",
    "00000000",
    "00000000000000000000000000000000",
    "0000000000000000FFFF00000100007F",
];

/// `socket:[<inode>]`, as a `/proc/<pid>/fd` link reads, of a line of `/proc/net/tcp*` listening on `port` at an
/// address of [`REACHED`].
#[cfg(target_os = "linux")]
fn listener(line: &str, port: u16) -> Option<String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let (address, local) = fields.get(1)?.rsplit_once(':')?;
    let local = u16::from_str_radix(local, 16).ok()?;
    // `0A` is TCP_LISTEN.
    (local == port && REACHED.contains(&address) && *fields.get(3)? == "0A").then_some(())?;
    Some(format!("socket:[{}]", fields.get(9)?))
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests;
