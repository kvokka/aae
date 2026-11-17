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

/// A token sweep kills the processes of its value, wherever they went, and spares those of another value.
#[tokio::test]
async fn the_token_sweep_kills_only_its_value() {
    let (name, mine) = ("LOOP_MANAGER_TEST_TOKEN", format!("mine-{}", std::process::id()));
    let start = |value: &str| spawn(Command::new("sleep").arg("300").env(name, value)).unwrap();
    let (mut a, mut b) = (start(&mine), start("other"));
    let (a_pid, b_pid) = (a.id(), b.id());
    assert_eq!(sweep_var(name, &mine), 1);
    a.wait().await.unwrap();
    assert!(
        start_time(i32::try_from(b_pid.unwrap()).unwrap()).is_some(),
        "the other value lives"
    );
    b.kill().await.unwrap();
    waited(a_pid);
    waited(b_pid);
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

/// A `std` child of the daemon's own that exited before its `wait` is left to it by the reaper.
#[test]
fn the_reaper_leaves_a_std_child_to_its_wait() {
    let (mut child, own) = spawn_std(std::process::Command::new("true").stdin(Stdio::null())).unwrap();
    let pid = i32::try_from(child.id()).unwrap();
    while start_time(pid).is_some() {
        std::thread::sleep(Duration::from_millis(10));
    }
    reap();
    assert!(child.wait().unwrap().success(), "its wait gets its exit");
    drop(own);
    assert!(!super::own().contains(&pid));
}
