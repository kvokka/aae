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
        command: &["/nonexistent/loop-manager-test".to_owned()],
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

#[tokio::test]
async fn a_process_is_alive_by_pid_and_start_time_until_it_is_reaped() {
    let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    let pid = i32::try_from(child.id()).unwrap();
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let start: u64 = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .nth(19)
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(crate::orphans::start_time(pid), Some(start));
    assert!(!wait_dead(pid, start, Duration::from_millis(200)).await);
    assert!(
        wait_dead(pid, start + 1, Duration::ZERO).await,
        "another start time is another process"
    );
    child.kill().unwrap();
    assert!(wait_dead(pid, start, Duration::from_secs(5)).await, "a zombie is dead");
    child.wait().unwrap();
}

#[cfg(target_os = "linux")] // the sweep reads `/proc`
#[tokio::test]
async fn wait_gone_spares_a_live_leader_and_kills_what_is_left_after_it() {
    let id = format!("wait-gone-{}", std::process::id());
    let mut command = tokio::process::Command::new("sh");
    command
        .args(["-c", "sleep 300 & exec sleep 300"])
        .env(crate::orphans::INSTANCE_ENV, &id);
    let mut leader = crate::orphans::spawn(&mut command).unwrap();
    let pid = i32::try_from(leader.id().unwrap()).unwrap();
    let start = crate::orphans::start_time(pid).unwrap();
    assert!(!wait_gone(&id, Some((pid, start)), Duration::from_millis(200)).await);
    assert_eq!(
        crate::orphans::start_time(pid),
        Some(start),
        "the leader ends by itself"
    );
    let child = leader.id();
    leader.kill().await.unwrap();
    crate::orphans::waited(child);
    assert!(wait_gone(&id, Some((pid, start)), Duration::from_secs(5)).await);
    assert_eq!(crate::orphans::sweep(&id), [], "its descendant is gone");
}

/// Death is confirmed, not only the marker gone: a killed process loses its environment before it is dead.
#[cfg(target_os = "linux")] // the sweep reads `/proc`
#[tokio::test]
async fn wait_gone_returns_once_what_it_killed_is_dead() {
    let id = format!("wait-dead-{}", std::process::id());
    // Its memory makes the gap wide: the environment goes first, the pages are freed, then the process is dead.
    let args = ["if=/dev/zero", "of=/dev/null", "bs=256M"];
    for _ in 0..5 {
        let mut command = std::process::Command::new("dd");
        let mut child = command
            .args(args)
            .env(crate::orphans::INSTANCE_ENV, &id)
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(wait_gone(&id, None, Duration::from_secs(5)).await);
        let pid = i32::try_from(child.id()).unwrap();
        // A zombie is dead; some kernels show it before `waitpid` sees it.
        assert_eq!(crate::orphans::start_time(pid), None, "dead, not only unmarked");
        child.wait().unwrap();
    }
}

#[test]
fn an_empty_walk_ends_the_wait_only_once_confirmed() {
    let mut walks = Walks::default();
    assert_eq!(walks.after(false, true), None, "the first walk is never confirmed");
    assert_eq!(walks.after(false, false), Some(true));
    let mut walks = Walks::default();
    assert_eq!(walks.after(true, false), None);
    assert_eq!(walks.after(false, true), None, "the walk before found processes");
    assert_eq!(walks.after(true, false), None);
    assert_eq!(walks.after(true, true), Some(false));
}
