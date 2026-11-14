use crate::process::wait_gone;
use std::time::Duration;

/// A process inside `execve` shows no marker yet: the first empty walk of `/proc` never ends the wait by itself.
#[tokio::test(start_paused = true)]
async fn an_instance_with_nothing_left_is_gone_after_a_second_empty_walk() {
    let started = tokio::time::Instant::now();
    assert!(wait_gone("aae-nothing-carries-this-marker", None, Duration::from_secs(5)).await);
    assert!(
        started.elapsed() >= Duration::from_millis(20),
        "{:?}",
        started.elapsed()
    );
}
