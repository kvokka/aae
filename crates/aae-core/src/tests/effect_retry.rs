use super::*;

#[tokio::test(start_paused = true)]
async fn effects_retry_with_backoff_one_at_a_time_per_key() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 3, Duration::from_millis(100));
    r.core
        .send(
            None,
            Msg::Work {
                effect_key: "a",
                dedupe: "1",
            },
        )
        .await
        .unwrap();
    r.core
        .send(
            None,
            Msg::Work {
                effect_key: "a",
                dedupe: "2",
            },
        )
        .await
        .unwrap();
    r.core
        .send(
            None,
            Msg::Work {
                effect_key: "b",
                dedupe: "1",
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(10)).await;
    let starts: Vec<(String, i64)> = r
        .calls
        .take()
        .into_iter()
        .filter(|(w, _)| w.ends_with(":start"))
        .map(|(w, at)| (w, at - T0))
        .collect();
    // Key "a": three failures (100 ms each, then 500 ms, 1 s, 2 s of backoff), the success, then the second
    // effect of "a". Key "b" runs alongside "a" with its own backoff.
    let a: Vec<i64> = starts.iter().filter(|(w, _)| w == "a:start").map(|(_, t)| *t).collect();
    let b: Vec<i64> = starts.iter().filter(|(w, _)| w == "b:start").map(|(_, t)| *t).collect();
    assert_eq!(a, vec![0, 600, 1700, 3800, 3900]);
    assert_eq!(b, vec![0, 600, 1700, 3800]);
    assert_eq!(count(&db, "effects"), 0);
}

#[tokio::test(start_paused = true)]
async fn a_panicked_effect_is_retried_like_a_failed_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 0, Duration::from_millis(100));
    let msg = Msg::Work {
        effect_key: "panics",
        dedupe: "1",
    };
    r.core.send(None, msg).await.unwrap();
    tokio::time::sleep(Duration::from_secs(10)).await;
    let starts: Vec<i64> = r
        .calls
        .take()
        .into_iter()
        .filter(|(w, _)| w == "panics:start")
        .map(|(_, at)| at - T0)
        .collect();
    assert_eq!(starts, vec![0, 600]);
    assert_eq!(count(&db, "effects"), 0);
}

#[test]
fn backoff_doubles_from_half_a_second_to_five_minutes() {
    let delays: Vec<i64> = (0..12).map(effects::backoff_ms).collect();
    assert_eq!(&delays[..4], &[500, 1000, 2000, 4000]);
    assert_eq!(delays[9], 256_000);
    assert_eq!(delays[10], 300_000);
    assert_eq!(effects::backoff_ms(1000), 300_000);
}

#[tokio::test(start_paused = true)]
async fn a_pending_effect_absorbs_its_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 1, Duration::ZERO);
    // Two distinct messages queue the same (key, dedupe) while the first copy waits for its retry.
    r.core
        .send(
            None,
            Msg::Work {
                effect_key: "a",
                dedupe: "stop",
            },
        )
        .await
        .unwrap();
    settle().await;
    r.core
        .send(
            None,
            Msg::Work {
                effect_key: "a",
                dedupe: "stop",
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let starts = r.calls.take().into_iter().filter(|(w, _)| w == "a:start").count();
    assert_eq!(
        starts, 2,
        "one failed try and one retry, no third run for the duplicate"
    );
    assert_eq!(max_seq(&rusqlite::Connection::open(&db).unwrap()), 2);
}
