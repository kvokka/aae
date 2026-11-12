mod config;
mod config_layers;
mod cron;
mod idempotency;
mod journal;
mod template;

use crate::{BoxFuture, Clock, Error, Event, Executor, Handler, Journal, Tx, effects, open, start};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

const T0: i64 = 1_800_000_000_000;

enum Msg {
    /// Emits one event and queues one effect.
    Work {
        effect_key: &'static str,
        dedupe: &'static str,
    },
    /// Queues an effect, then fails: nothing may be committed or executed.
    WorkThenFail,
    Deadline {
        id: &'static str,
        at_ms: i64,
    },
    ClearDeadline {
        id: &'static str,
    },
}

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<(String, i64)>>>);

impl Log {
    fn push(&self, what: String, at: i64) {
        self.0.lock().unwrap().push((what, at));
    }
    fn take(&self) -> Vec<(String, i64)> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct TestHandler {
    fired: Log,
}

impl Handler for TestHandler {
    type Msg = Msg;

    fn handle(&mut self, tx: &mut Tx<'_>, msg: Msg) -> Result<Value, Error> {
        match msg {
            Msg::Work { effect_key, dedupe } => {
                let seq = tx.emit(&event("test.work"))?;
                tx.effect(effect_key, dedupe, &json!({ "seq": seq }))?;
                Ok(json!({ "seq": seq }))
            }
            Msg::WorkThenFail => {
                tx.emit(&event("test.work"))?;
                tx.effect("k", "x", &json!({}))?;
                Err(Error::Rejected("no".into()))
            }
            Msg::Deadline { id, at_ms } => tx.set_deadline(id, at_ms).map(|()| Value::Null),
            Msg::ClearDeadline { id } => tx.clear_deadline(id).map(|()| Value::Null),
        }
    }

    fn deadline(&mut self, tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        self.fired.push(id.to_owned(), tx.now_ms());
        Ok(())
    }
}

/// Records every execution with its managed time; fails the first `fail_first` executions of each key, and
/// checks that the event that caused the effect is already committed (visible to another connection).
struct TestExecutor {
    clock: Clock,
    db: std::path::PathBuf,
    calls: Log,
    fail_first: usize,
    hold: Duration,
}

impl Executor for TestExecutor {
    fn execute<'a>(&'a self, key: &'a str, payload: &'a Value) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let seq = payload["seq"].as_i64().unwrap_or(0);
            let reader = rusqlite::Connection::open(&self.db).unwrap();
            assert!(max_seq(&reader) >= seq, "effect ran before its transaction committed");
            self.calls.push(format!("{key}:start"), self.clock.now_ms());
            tokio::time::sleep(self.hold).await;
            self.calls.push(format!("{key}:end"), self.clock.now_ms());
            let failed_before = self
                .calls
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(w, _)| *w == format!("{key}:fail"))
                .count();
            if failed_before < self.fail_first {
                self.calls.push(format!("{key}:fail"), self.clock.now_ms());
                return false;
            }
            true
        })
    }
}

fn event(kind: &str) -> Event {
    Event {
        kind: kind.into(),
        project: None,
        rig: None,
        run: None,
        correlation: None,
        data: json!({}),
    }
}

struct Rig {
    core: crate::CoreHandle<Msg>,
    task: tokio::task::JoinHandle<Result<(), Error>>,
    fired: Log,
    calls: Log,
    clock: Clock,
}

fn rig(db: &Path, clock: Clock, fail_first: usize, hold: Duration) -> Rig {
    rig_with(db, clock, fail_first, hold, Journal::new(&db.with_file_name("journal")))
}

fn rig_with(db: &Path, clock: Clock, fail_first: usize, hold: Duration, journal: Journal) -> Rig {
    let (fired, calls) = (Log::default(), Log::default());
    let executor = TestExecutor {
        clock,
        db: db.to_owned(),
        calls: calls.clone(),
        fail_first,
        hold,
    };
    let (core, task) = start(
        open(db).unwrap(),
        clock,
        TestHandler { fired: fired.clone() },
        executor,
        journal,
    );
    Rig {
        core,
        task,
        fired,
        calls,
        clock,
    }
}

async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

/// The last `seq` ever committed (the outbox itself is emptied by the journal).
fn max_seq(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM sqlite_sequence WHERE name = 'outbox'",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

fn count(db: &Path, table: &str) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn nothing_of_a_rejected_message_is_committed_or_executed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 0, Duration::ZERO);
    assert!(matches!(
        r.core.send(Some("cmd-1"), Msg::WorkThenFail).await,
        Err(Error::Rejected(_))
    ));
    settle().await;
    assert_eq!(max_seq(&rusqlite::Connection::open(&db).unwrap()), 0);
    assert_eq!(count(&db, "effects"), 0);
    assert_eq!(count(&db, "idempotency"), 0);
    assert!(r.calls.take().is_empty());
}

#[tokio::test(start_paused = true)]
async fn deadline_fires_at_its_exact_time_without_polling() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 0, Duration::ZERO);
    // An odd offset: a polling loop would fire on a multiple of its interval, not on this millisecond.
    let at = T0 + 3_600_007;
    r.core.send(None, Msg::Deadline { id: "d1", at_ms: at }).await.unwrap();
    r.core
        .send(
            None,
            Msg::Deadline {
                id: "gone",
                at_ms: T0 + 10,
            },
        )
        .await
        .unwrap();
    r.core.send(None, Msg::ClearDeadline { id: "gone" }).await.unwrap();
    tokio::time::advance(Duration::from_millis(3_600_006)).await;
    settle().await;
    assert!(r.fired.take().is_empty());
    tokio::time::sleep(Duration::from_millis(1)).await;
    settle().await;
    assert_eq!(r.fired.take(), vec![("d1".to_owned(), at)]);
    assert_eq!(r.clock.now_ms(), at);
}

#[tokio::test(start_paused = true)]
async fn deadline_and_seq_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let r = rig(&db, Clock::managed(T0), 0, Duration::ZERO);
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
            Msg::Deadline {
                id: "d1",
                at_ms: T0 + 60_000,
            },
        )
        .await
        .unwrap();
    let Rig { core, task, .. } = r;
    drop(core);
    task.await.unwrap().unwrap();
    // The daemon comes back two minutes later: the missed deadline fires once, at start.
    let r = rig(&db, Clock::managed(T0 + 120_000), 0, Duration::ZERO);
    let reply = r
        .core
        .send(
            None,
            Msg::Work {
                effect_key: "a",
                dedupe: "2",
            },
        )
        .await
        .unwrap();
    settle().await;
    assert_eq!(reply, json!({ "seq": 2 }));
    assert_eq!(r.fired.take(), vec![("d1".to_owned(), T0 + 120_000)]);
}

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

#[test]
fn the_system_clock_reads_the_system_time() {
    let wall = || {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    };
    let before = wall();
    let now = Clock::system().now_ms();
    assert!((before..=wall()).contains(&now));
}
