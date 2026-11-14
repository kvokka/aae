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

/// Kills every process but this one whose environment carries `AAE_INSTANCE=<id>`; the ones it found as (pid, start
/// time), so one it could not kill does not pass for dead.
pub(crate) fn sweep(id: &str) -> Vec<(i32, u64)> {
    sweep_all(&BTreeSet::from([id.to_owned()]))
}

/// [`sweep`] for every id of `ids` in one walk of `/proc`.
pub(crate) fn sweep_all(ids: &BTreeSet<String>) -> Vec<(i32, u64)> {
    marked()
        .filter(|(_, marker)| ids.contains(marker))
        .filter_map(|(pid, _)| Some((Pid::from_raw(pid)?, start_time(pid)?)))
        .map(|(pid, start)| {
            let _ = kill_process(pid, Signal::KILL);
            (pid.as_raw_nonzero().get(), start)
        })
        .collect()
}

/// `port` on loopback is listened on, and only by processes marked `id`: a port check passed by anyone else would
/// hand them the sessions' token (spec §11.4). Only listeners a connection to `127.0.0.1` may reach count.
#[cfg(target_os = "linux")]
pub(crate) fn listens_alone(id: &str, port: u16) -> bool {
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
/// release, §1.1); read the sockets through `proc_pidfdinfo` once macOS is checked.
#[cfg(not(target_os = "linux"))]
pub(crate) fn listens_alone(_id: &str, _port: u16) -> bool {
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
        assert_eq!(sweep(&id).len(), left);
        for _ in 0..100 {
            if marked(&id) == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("descendants with the marker are still alive");
    }

    /// A listener on `127.0.0.1:<port>` another process may share with `SO_REUSEPORT`.
    #[cfg(target_os = "linux")]
    fn shared(port: u16) -> tokio::net::TcpListener {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.set_reuseport(true).unwrap();
        socket.bind(([127, 0, 0, 1], port).into()).unwrap();
        socket.listen(1).unwrap()
    }

    /// Servers listen on every interface by default: `0.0.0.0` and `::` are reached from `127.0.0.1`, `::1` is not.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_listener_on_every_interface_is_reached() {
        let line = |address: &str, state: &str| {
            format!("0: {address}:1F90 00000000:0000 {state} 00000000:00000000 00:00000000 00000000 1000 0 77 1")
        };
        let v6_any = "0".repeat(32);
        let v6_loopback = format!("{}01000000", "0".repeat(24));
        for address in ["0100007F", "00000000", &v6_any, "0000000000000000FFFF00000100007F"] {
            assert_eq!(
                listener(&line(address, "0A"), 8080),
                Some("socket:[77]".to_owned()),
                "{address}"
            );
            assert_eq!(listener(&line(address, "0A"), 8081), None);
            assert_eq!(listener(&line(address, "01"), 8080), None, "not listening");
        }
        assert_eq!(listener(&line("0200007F", "0A"), 8080), None);
        assert_eq!(listener(&line(&v6_loopback, "0A"), 8080), None);
    }

    /// A port is the instance's only while a process with its marker alone listens on it where a connection to
    /// `127.0.0.1` may reach.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_port_is_the_instances_only_while_it_alone_listens() {
        let id = format!("test-port-{}", std::process::id());
        let listener = shared(0).into_std().unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!listens_alone(&id, port), "this unmarked process listens");
        // The listening socket moves to a marked child: the parent's copy is closed by the spawn.
        let socket = Stdio::from(std::os::fd::OwnedFd::from(listener));
        let mut child = spawn(Command::new("sleep").arg("30").env(INSTANCE_ENV, &id).stdin(socket)).unwrap();
        let mut alone = false;
        for _ in 0..100 {
            alone = listens_alone(&id, port);
            if alone {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(alone, "the marked child listens");
        assert!(!listens_alone("someone-else", port));
        let other = shared(port);
        assert!(!listens_alone(&id, port), "this unmarked process listens too");
        drop(other);
        // No connection to 127.0.0.1 reaches `[::1]` (where the host has IPv6).
        if let Ok(_v6) = std::net::TcpListener::bind(("::1", port)) {
            assert!(listens_alone(&id, port), "a listener on [::1] is no one's concern");
        }
        let pid = child.id();
        child.kill().await.unwrap();
        waited(pid);
        assert!(!listens_alone(&id, port), "nothing listens");
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
