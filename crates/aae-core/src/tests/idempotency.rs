//! Idempotency keys of the writer (invariant 10) and how long they are kept.

use super::{Msg, T0, count, max_seq, rig, settle};
use crate::Clock;
use std::time::Duration;

/// A keyed message reads its key before it writes: under a lock held by another connection it still waits the lock
/// out instead of failing at the upgrade (docs/design/event-model.md, State).
#[tokio::test]
async fn a_keyed_message_waits_out_a_lock_held_by_another_connection() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::system(), 0, Duration::ZERO);
    // The start-up transaction is behind this reply.
    r.core
        .send(Some("cmd-0"), Msg::ClearDeadline { id: "none" })
        .await
        .unwrap();
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let held = Duration::from_millis(300);
    let sent = std::time::Instant::now();
    let release = std::thread::spawn(move || {
        std::thread::sleep(held);
        lock.execute_batch("COMMIT").unwrap();
    });
    let work = Msg::Work {
        effect_key: "run-1",
        dedupe: "start",
    };
    r.core.send(Some("cmd-1"), work).await.unwrap();
    assert!(sent.elapsed() >= held, "the reply came before the lock was released");
    release.join().unwrap();
}

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

#[tokio::test(start_paused = true)]
async fn a_key_repeated_with_another_request_is_refused_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 0, Duration::ZERO);
    let work = |dedupe| Msg::Work {
        effect_key: "run-1",
        dedupe,
    };
    let first = r.core.send_request(Some(("cmd-1", "a")), work("a")).await.unwrap();
    let again = r.core.send_request(Some(("cmd-1", "a")), work("a")).await.unwrap();
    assert_eq!(first, again);
    let other = r.core.send_request(Some(("cmd-1", "b")), work("b")).await;
    assert!(matches!(other, Err(crate::Error::KeyReused)), "{other:?}");
    settle().await;
    assert_eq!(max_seq(&rusqlite::Connection::open(&db).unwrap()), 1);
}
