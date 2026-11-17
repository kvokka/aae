use super::*;
use std::time::Instant;

/// The `exec` effect of attempt `attempt` of run `r`: `sh -c <script>` with a 30 s timeout.
fn exec(attempt: u32, script: &str) -> Value {
    json!({
        "op": "exec", "run": "r", "attempt": attempt, "command": ["sh", "-c", script],
        "cwd": "/", "env": [], "timeout_ms": 30_000, "marker": "x-test",
    })
}

/// Runs `effect`, sends `stop` once its command is running, and returns how the command ended.
async fn stopped(effect: Value, stop: Value) -> (Exit, String) {
    let running = Running::default();
    let command = tokio::spawn({
        let running = running.clone();
        async move { run(&running, &effect, None).await }
    });
    let started = Instant::now();
    while running.lock().unwrap().is_empty() {
        assert!(started.elapsed() < Duration::from_secs(5), "the command did not start");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    super::stop(&running, &stop);
    let Msg::ExecDone { exit, stdout, .. } = command.await.unwrap() else {
        panic!("not an exec result")
    };
    assert!(running.lock().unwrap().is_empty(), "the command stays listed");
    (exit, stdout)
}

#[tokio::test]
async fn a_stop_of_an_attempt_ends_its_command() {
    let started = Instant::now();
    let stop = json!({ "op": "stop_exec", "run": "r", "attempt": 2 });
    let (exit, _) = stopped(exec(2, "sleep 30"), stop).await;
    assert_eq!(exit, Exit::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn a_late_stop_of_the_last_attempt_leaves_the_next_one_running() {
    let stop = json!({ "op": "stop_exec", "run": "r", "attempt": 1 });
    let (exit, stdout) = stopped(exec(2, "sleep 1; echo done"), stop).await;
    assert_eq!((exit, stdout.as_str()), (Exit::Code(0), "done\n"));
}
