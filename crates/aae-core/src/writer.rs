//! The single writer (spec §5.1, invariants 1, 9, 10): every message is handled in one SQLite transaction that
//! also records the events (outbox), the effects and the deadlines it caused; effects run only after commit.

use crate::{clock::Clock, effects, journal::Journal};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{collections::HashSet, fmt, future::Future, pin::Pin, sync::Arc};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// How long an idempotency key is remembered.
const IDEMPOTENCY_KEEP_MS: i64 = 86_400_000;
/// The longest the writer sleeps without reading the wall clock (deadlines are wall-clock times).
const RECHECK_MS: i64 = 30_000;
/// How long after a failed firing a deadline fires again.
const DEADLINE_RETRY_MS: i64 = 60_000;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug)]
pub enum Error {
    Storage(rusqlite::Error),
    /// The handler refused the message; nothing of it was committed.
    Rejected(String),
    /// The writer has stopped.
    Stopped,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "state storage: {e}"),
            Self::Rejected(reason) => write!(f, "rejected: {reason}"),
            Self::Stopped => f.write_str("the core has stopped"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Storage(e)
    }
}

/// An event before commit; `seq` and the time are assigned by the outbox (docs/design/contracts.md 1.1).
pub struct Event {
    pub kind: String,
    pub project: Option<String>,
    pub rig: Option<String>,
    pub run: Option<String>,
    pub correlation: Option<String>,
    pub data: Value,
}

/// The open transaction of one message.
pub struct Tx<'a> {
    tx: rusqlite::Transaction<'a>,
    now_ms: i64,
    /// The envelopes of the events emitted so far, for [`Handler::emitted`].
    events: Vec<Value>,
}

impl Tx<'_> {
    pub fn now_ms(&self) -> i64 {
        self.now_ms
    }

    /// The transaction itself, for the domain tables of the core.
    pub(crate) fn sql(&self) -> &rusqlite::Transaction<'_> {
        &self.tx
    }

    /// Appends the event to the outbox and returns its `seq`.
    pub fn emit(&mut self, event: &Event) -> Result<i64, Error> {
        let body = json!({
            "type": event.kind,
            "project": event.project,
            "rig": event.rig,
            "run": event.run,
            "correlation": event.correlation,
            "data": event.data,
        });
        let body = body.to_string();
        self.tx.execute(
            "INSERT INTO outbox (ts_ms, body) VALUES (?1, ?2)",
            params![self.now_ms, body],
        )?;
        let seq = self.tx.last_insert_rowid();
        self.events.push(crate::journal::envelope(seq, self.now_ms, &body));
        Ok(seq)
    }

    /// Queues an effect for after commit; a pending effect with the same `(key, dedupe)` absorbs it.
    pub fn effect(&mut self, key: &str, dedupe: &str, payload: &Value) -> Result<(), Error> {
        self.tx.execute(
            "INSERT OR IGNORE INTO effects (key, dedupe, payload, next_at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![key, dedupe, payload.to_string(), self.now_ms],
        )?;
        Ok(())
    }

    /// Sets (or moves) the deadline `id`; the writer wakes exactly at `at_ms` and calls `Handler::deadline`.
    pub fn set_deadline(&mut self, id: &str, at_ms: i64) -> Result<(), Error> {
        self.tx.execute(
            "INSERT INTO deadlines (id, at_ms) VALUES (?1, ?2) ON CONFLICT (id) DO UPDATE SET at_ms = ?2",
            params![id, at_ms],
        )?;
        Ok(())
    }

    pub fn clear_deadline(&mut self, id: &str) -> Result<(), Error> {
        self.tx.execute("DELETE FROM deadlines WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Runs `f` as one unit of the transaction: when it fails, what it wrote and emitted is undone and the rest of
    /// the transaction stays.
    pub(crate) fn unit<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        let emitted = self.events.len();
        self.tx.execute_batch("SAVEPOINT unit")?;
        let result = f(self);
        match result {
            Ok(_) => self.tx.execute_batch("RELEASE unit")?,
            Err(_) => {
                self.tx.execute_batch("ROLLBACK TO unit; RELEASE unit")?;
                self.events.truncate(emitted);
            }
        }
        result
    }
}

/// The domain logic run inside the writer. It keeps its state in the database: an error rolls the transaction
/// back, and in-memory changes would survive that.
pub trait Handler: Send + 'static {
    type Msg: Send + 'static;
    fn handle(&mut self, tx: &mut Tx<'_>, msg: Self::Msg) -> Result<Value, Error>;
    fn deadline(&mut self, tx: &mut Tx<'_>, id: &str) -> Result<(), Error>;
    /// Sees, before commit, the envelopes of the events the transaction emitted: what the handler queues or emits
    /// here commits with them (the hook dispatcher and the trigger matcher of docs/design/contracts.md 3.5). The
    /// events it emits here come back to it, until a call emits none.
    fn emitted(&mut self, _tx: &mut Tx<'_>, _events: &[Value]) -> Result<(), Error> {
        Ok(())
    }
    /// Runs once in its own transaction when the writer starts, before any message or deadline.
    fn started(&mut self, _tx: &mut Tx<'_>) -> Result<(), Error> {
        Ok(())
    }
}

/// Runs committed effects. `true` means done; `false` retries the effect with backoff.
pub trait Executor: Send + Sync + 'static {
    fn execute<'a>(&'a self, key: &'a str, payload: &'a Value) -> BoxFuture<'a, bool>;
}

struct Input<M> {
    key: Option<String>,
    msg: M,
    reply: oneshot::Sender<Result<Value, Error>>,
}

/// The input queue of the writer.
pub struct CoreHandle<M> {
    tx: mpsc::Sender<Input<M>>,
}

impl<M> Clone for CoreHandle<M> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

impl<M: Send + 'static> CoreHandle<M> {
    /// Handles `msg` and returns the handler's reply. With an idempotency key, a repeated message returns the
    /// stored reply of the first one and changes nothing (invariant 10).
    pub async fn send(&self, key: Option<&str>, msg: M) -> Result<Value, Error> {
        let (reply, answer) = oneshot::channel();
        let input = Input {
            key: key.map(str::to_owned),
            msg,
            reply,
        };
        self.tx.send(input).await.map_err(|_| Error::Stopped)?;
        answer.await.map_err(|_| Error::Stopped)?
    }
}

/// Starts the writer on `conn` (opened by `store::open`). It stops when every handle is dropped.
pub fn start<H: Handler, E: Executor>(
    conn: Connection,
    clock: Clock,
    handler: H,
    executor: E,
    journal: Journal,
) -> (CoreHandle<H::Msg>, JoinHandle<Result<(), Error>>) {
    let (tx, inbox) = mpsc::channel(256);
    let task = tokio::spawn(run(conn, clock, handler, Arc::new(executor), journal, inbox));
    (CoreHandle { tx }, task)
}

async fn run<H: Handler, E: Executor>(
    mut conn: Connection,
    clock: Clock,
    mut handler: H,
    executor: Arc<E>,
    mut journal: Journal,
    mut inbox: mpsc::Receiver<Input<H::Msg>>,
) -> Result<(), Error> {
    {
        let mut tx = Tx {
            tx: conn.transaction()?,
            now_ms: clock.now_ms(),
            events: Vec::new(),
        };
        handler.started(&mut tx)?;
        settle(&mut tx, &mut handler)?;
        tx.tx.commit()?;
    }
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<(i64, String, bool)>();
    let mut in_flight = HashSet::new();
    loop {
        let now = clock.now_ms();
        let deadline_wake = fire_deadlines(&mut conn, &mut handler, now)?;
        // A failed journal write leaves the events in the outbox and is retried with the effect backoff.
        let journal_wake =
            (journal.drain(&mut conn).is_err()).then(|| now + effects::backoff_ms(i64::from(journal.failures()) - 1));
        let (due, effect_wake) = effects::heads(&conn, &in_flight, now)?;
        for effect in due {
            in_flight.insert(effect.key.clone());
            let executor = executor.clone();
            let done = done_tx.clone();
            tokio::spawn(async move {
                let ok = executor.execute(&effect.key, &effect.payload).await;
                let _ = done.send((effect.id, effect.key, ok));
            });
        }
        // The sleep is monotonic and wakes late after the machine slept: the wall clock is read again this often.
        let recheck = Some(now + RECHECK_MS);
        let wake = [deadline_wake, effect_wake, journal_wake, recheck]
            .into_iter()
            .flatten()
            .min();
        let sleep = async {
            match wake {
                Some(at) => tokio::time::sleep_until(clock.instant_at(at)).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            input = inbox.recv() => {
                let Some(Input { key, msg, reply }) = input else { return Ok(()) };
                let _ = reply.send(process(&mut conn, &mut handler, clock.now_ms(), key, msg));
            }
            Some((id, key, ok)) = done_rx.recv() => {
                in_flight.remove(&key);
                effects::finish(&conn, id, ok, clock.now_ms())?;
            }
            () = sleep => {}
        }
    }
}

fn process<H: Handler>(
    conn: &mut Connection,
    handler: &mut H,
    now_ms: i64,
    key: Option<String>,
    msg: H::Msg,
) -> Result<Value, Error> {
    let tx = conn.transaction()?;
    if let Some(key) = &key {
        let stored: Option<Value> = tx
            .query_row("SELECT reply FROM idempotency WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        if let Some(stored) = stored {
            return Ok(stored);
        }
    }
    let mut tx = Tx {
        tx,
        now_ms,
        events: Vec::new(),
    };
    let reply = handler.handle(&mut tx, msg)?;
    settle(&mut tx, handler)?;
    if let Some(key) = key {
        // A key is kept a day: a client retries within seconds or minutes, and the table must not grow forever.
        tx.tx.execute(
            "DELETE FROM idempotency WHERE at_ms < ?1",
            [now_ms - IDEMPOTENCY_KEEP_MS],
        )?;
        tx.tx.execute(
            "INSERT INTO idempotency (key, reply, at_ms) VALUES (?1, ?2, ?3)",
            params![key, reply.to_string(), now_ms],
        )?;
    }
    tx.tx.commit()?;
    Ok(reply)
}

/// Fires every deadline due at `now_ms`, each in its own transaction, and returns the next wake-up time.
fn fire_deadlines<H: Handler>(conn: &mut Connection, handler: &mut H, now_ms: i64) -> Result<Option<i64>, Error> {
    loop {
        let next: Option<(String, i64)> = conn
            .query_row("SELECT id, at_ms FROM deadlines ORDER BY at_ms, id LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let Some((id, at_ms)) = next else { return Ok(None) };
        if at_ms > now_ms {
            return Ok(Some(at_ms));
        }
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM deadlines WHERE id = ?1", [&id])?;
        let mut tx = Tx {
            tx,
            now_ms,
            events: Vec::new(),
        };
        let fired = handler.deadline(&mut tx, &id).and_then(|()| settle(&mut tx, handler));
        match fired {
            Ok(()) => tx.tx.commit()?,
            // Its effects are rolled back and it fires again later: a dropped deadline would leave a run with no exit
            // or a schedule silent until the next start (invariant 14).
            Err(_) => {
                drop(tx);
                conn.execute(
                    "UPDATE deadlines SET at_ms = ?2 WHERE id = ?1",
                    params![id, now_ms + DEADLINE_RETRY_MS],
                )?;
            }
        }
    }
}

/// Hands the handler the events of the transaction until it emits no more.
fn settle<H: Handler>(tx: &mut Tx<'_>, handler: &mut H) -> Result<(), Error> {
    loop {
        let events = std::mem::take(&mut tx.events);
        if events.is_empty() {
            return Ok(());
        }
        handler.emitted(tx, &events)?;
    }
}
