//! No orphans (spec §5.5, invariant 7; docs/design/experiments.md "Descendants after `kill -9`"): every instance
//! process and its descendants carry `AAE_INSTANCE=<id>` and are killed by it once the instance is gone; the
//! daemon is the child subreaper, so descendants that dropped the marker are reaped by it, not left as zombies.

// shortcut: Linux only (`/proc`, `PR_SET_CHILD_SUBREAPER`); elsewhere nothing is found or adopted and a process is
// told by its pid alone (`start_time`) — macOS ships but is not checked in the first release (§1.1).

use rustix::process::{Pid, Signal, WaitOptions, getpid, kill_process, waitpid};
use std::{
    collections::BTreeSet,
    io,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::process::{Child, Command};

/// The variable in the environment of every instance process: its instance id.
pub const INSTANCE_ENV: &str = "AAE_INSTANCE";

/// How often adopted orphans that exited are reaped.
const REAP_EVERY: Duration = Duration::from_secs(5);

/// The children the daemon spawned itself: their `wait` reaps them, the orphan reaper leaves them alone.
static OWN: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());

fn own() -> MutexGuard<'static, BTreeSet<i32>> {
    OWN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Spawns `command` as a child of the daemon's own; [`waited`] once its `wait` returned.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    // Held across the spawn: a child that exits at once is not reaped before it is known.
    let mut own = own();
    let child = command.spawn()?;
    own.extend(child.id().and_then(|id| i32::try_from(id).ok()));
    Ok(child)
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
/// process that is gone. pid + start time identify a process (spec §5.2).
#[cfg(target_os = "linux")]
pub fn start_time(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    (fields.next()? != "Z").then_some(())?;
    fields.nth(18)?.parse().ok()
}

/// shortcut: without `/proc` every existing process starts at 0, so a reused pid passes for the old process and a
/// zombie for a live one; read `kinfo_proc` through `sysctl` once macOS is checked (§1.1).
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

/// The `AAE_INSTANCE` value in the environment of process `pid`.
fn marker(pid: i32) -> Option<String> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{INSTANCE_ENV}=");
    let var = environ
        .split(|b| *b == 0)
        .find_map(|var| var.strip_prefix(prefix.as_bytes()))?;
    String::from_utf8(var.to_vec()).ok()
}

/// Live processes but this one, with their marker.
fn marked() -> impl Iterator<Item = (i32, String)> {
    let me = getpid().as_raw_nonzero().get();
    processes()
        .filter(move |(pid, state, _)| *pid != me && state != "Z")
        .filter_map(|(pid, ..)| Some((pid, marker(pid)?)))
}

/// Kills every process but this one whose environment carries `AAE_INSTANCE=<id>`; how many it found, so one it
/// could not kill does not pass for dead.
pub(crate) fn sweep(id: &str) -> usize {
    sweep_all(&BTreeSet::from([id.to_owned()]))
}

/// [`sweep`] for every id of `ids` in one walk of `/proc`.
pub(crate) fn sweep_all(ids: &BTreeSet<String>) -> usize {
    marked()
        .filter(|(_, marker)| ids.contains(marker))
        .filter_map(|(pid, _)| Pid::from_raw(pid))
        .inspect(|pid| {
            let _ = kill_process(*pid, Signal::KILL);
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    fn marked(id: &str) -> usize {
        super::marked().filter(|(_, marker)| marker == id).count()
    }

    fn children(state: &str) -> usize {
        let me = getpid().as_raw_nonzero().get();
        processes().filter(|(_, s, ppid)| s == state && *ppid == me).count()
    }

    /// An instance killed by its group leaves descendants in a group and a session of their own; the sweep ends them.
    #[tokio::test]
    async fn the_sweep_kills_every_descendant_with_the_marker() {
        let id = format!("test-{}", std::process::id());
        let script = "setsid sleep 300 & sh -c 'sleep 300' & sleep 0.2; exit 0";
        let mut child = spawn(
            Command::new("sh")
                .args(["-c", script])
                .env(INSTANCE_ENV, &id)
                .stdin(Stdio::null())
                .process_group(0),
        )
        .unwrap();
        let pid = child.id();
        child.wait().await.unwrap();
        waited(pid);
        let left = marked(&id);
        assert!(left >= 2, "both descendants outlive their parent");
        assert_eq!(sweep(&id), left);
        for _ in 0..100 {
            if marked(&id) == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("descendants with the marker are still alive");
    }

    /// An orphan that cleared its environment is adopted and reaped; the daemon's own child is left to its `wait`.
    /// Needs a process of its own (nextest): under `cargo test` an exited child of a parallel test that spawned
    /// without [`spawn`] could be reaped too.
    #[tokio::test]
    async fn adopted_orphans_are_reaped_and_own_children_are_not() {
        adopt_orphans().unwrap();
        let mut own_child = spawn(Command::new("true").stdin(Stdio::null())).unwrap();
        let own_pid = own_child.id();
        let mut parent = Command::new("sh")
            .args(["-c", "env -i sleep 0.2 & exit 0"])
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        parent.wait().await.unwrap();
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(children("Z") >= 2, "the orphan and the own child exited unreaped");
        assert_eq!(reap(), 1, "only the orphan");
        own_child.wait().await.unwrap();
        waited(own_pid);
        assert_eq!(children("Z"), 0);
    }
}
