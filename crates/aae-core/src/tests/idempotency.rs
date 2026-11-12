//! Idempotency keys of the writer (invariant 10) and how long they are kept.

use super::{Msg, T0, count, max_seq, rig, settle};
use crate::Clock;
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn repeated_message_does_not_repeat_the_effect() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 0, Duration::ZERO);
    let work = || Msg::Work {
        effect_key: "run-1",
        dedupe: "start",
    };
    let first = r.core.send(Some("cmd-1"), work()).await.unwrap();
    let second = r.core.send(Some("cmd-1"), work()).await.unwrap();
    settle().await;
    assert_eq!(first, second);
    assert_eq!(max_seq(&rusqlite::Connection::open(&db).unwrap()), 1);
    assert_eq!(r.calls.take().iter().filter(|(w, _)| w.ends_with(":start")).count(), 1);
    assert_eq!(count(&db, "effects"), 0);
}

#[tokio::test(start_paused = true)]
async fn a_key_is_forgotten_a_day_after_its_message() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 0, Duration::ZERO);
    let work = |dedupe| Msg::Work {
        effect_key: "run-1",
        dedupe,
    };
    r.core.send(Some("cmd-1"), work("a")).await.unwrap();
    tokio::time::sleep(Duration::from_secs(86_400) + Duration::from_millis(1)).await;
    r.core.send(Some("cmd-2"), work("b")).await.unwrap();
    assert_eq!(count(&db, "idempotency"), 1, "the day-old key is gone");
    r.core.send(Some("cmd-1"), work("c")).await.unwrap();
    settle().await;
    assert_eq!(
        max_seq(&rusqlite::Connection::open(&db).unwrap()),
        3,
        "a forgotten key is a new message"
    );
}
