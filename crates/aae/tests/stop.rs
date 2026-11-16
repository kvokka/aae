//! The daemon's start and its stop by a signal (docs/guide/install.md): what its steps started goes with it
//! (invariant 7), quietly.

mod common;

use common::{Daemon, FILES, WAIT};
use std::{
    fs,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

/// The processes, the daemon aside, whose environment names the tree at `root`.
fn steps(root: &Path, daemon: u32) -> Vec<u32> {
    let needle = root.to_string_lossy().into_owned();
    let pids = fs::read_dir("/proc").unwrap().flatten();
    let pids = pids.filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok());
    let live = |pid: &u32| fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.contains(") Z "));
    pids.filter(|&pid| pid != daemon && live(&pid))
        .filter(|pid| {
            fs::read(format!("/proc/{pid}/environ"))
                .is_ok_and(|env| env.windows(needle.len()).any(|w| w == needle.as_bytes()))
        })
        .collect()
}

#[tokio::test]
async fn a_signal_stop_ends_the_step_processes_and_prints_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in FILES {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let hold = "name = \"hold\"\n[[step]]\nkind = \"exec\"\ncommand = [\"sh\", \"-c\", \"sleep 611 & sleep 611; wait\"]\n\
                timeout = \"20m\"\n";
    fs::write(dir.path().join("config/aae/shop/formulas/hold.toml"), hold).unwrap();
    let log = fs::File::create(dir.path().join("daemon.log")).unwrap();
    let child = common::command(dir.path()).stderr(Stdio::from(log)).spawn().unwrap();
    let client = aae_front::client::Client::new(dir.path().join("run/aae/aae.sock"));
    let mut d = Daemon { child, dir, client };
    let deadline = Instant::now() + WAIT;
    while d.client.request("GET", "/v1/daemon", None, None).await.is_err() {
        assert!(Instant::now() < deadline, "the daemon did not come up");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let run = d.create("hold", None).await;
    d.until_state(&run, "running").await;
    let root = d.dir.path().to_owned();
    let pid = d.child.id();
    while steps(&root, pid).len() < 3 {
        assert!(
            Instant::now() < deadline,
            "the step did not start: {:?}",
            steps(&root, pid)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(d.terminate().success());
    // Killed before the daemon exited; their parent, gone too, leaves them to be reaped by init.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !steps(&root, pid).is_empty() {
        assert!(
            Instant::now() < deadline,
            "left after the stop: {:?}",
            steps(&root, pid)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let log = fs::read_to_string(root.join("daemon.log")).unwrap();
    assert!(!log.contains("panicked"), "{log}");
}

/// An older daemon's `lock` file is not left in the state directory (docs/design/event-model.md: no lock files).
#[tokio::test]
async fn a_start_removes_the_lock_file_of_an_older_daemon() {
    let d = Daemon::start(|dir| {
        fs::create_dir_all(dir.join("state/aae")).unwrap();
        fs::write(dir.join("state/aae/lock"), "").unwrap();
    })
    .await;
    assert!(!d.dir.path().join("state/aae/lock").exists());
}
