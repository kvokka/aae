//! Runs (spec §9.3, docs/design/contracts.md 6): the run state machine inside the single writer, the effects that
//! drive a harness through its [`Driver`], and the driver events fed back to the writer as messages.

mod bead_events;
mod beads;
mod decide;
mod events;
mod exec;
mod exec_step;
mod finish;
mod gates;
mod hooks;
mod machine;
mod msg;
mod prompts;
mod queue;
mod read;
mod recover;
mod registry;
mod reload;
mod row;
mod triggers;

use crate::{
    clock::Clock,
    config::Config,
    driver::{Answer, Driver, Failure, Normalized},
    journal::Journal,
    tracker::Tracker,
    writer::{self, CoreHandle, Error, Handler, Tx},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock, PoisonError},
};
use tokio::{sync::mpsc, task::JoinHandle};

/// Ceiling of the core: attempts per run (invariant 16).
pub const MAX_ATTEMPTS: u32 = 20;
pub use crate::process::Exit;
pub use finish::MAX_REMARKS;
pub use gates::{GateSpec, valid_key};
pub use hooks::{BundledHook, ExecHooks, HookRunner};
pub use msg::{Create, Decision, Msg, Outcome, Verdict};
pub use read::{Reader, RunFilter};

type Drivers = Arc<BTreeMap<String, Arc<dyn Driver>>>;

/// Starts the writer with the run machine on `conn` (opened by `store::open`), one driver per harness and the
/// trackers of the projects that have one, by project name.
pub fn start(
    conn: Connection,
    clock: Clock,
    config: Arc<Config>,
    drivers: Vec<Arc<dyn Driver>>,
    trackers: BTreeMap<String, Arc<dyn Tracker>>,
    hooks: Arc<dyn HookRunner>,
    journal: Journal,
) -> (CoreHandle<Msg>, JoinHandle<Result<(), Error>>) {
    let drivers: Drivers = Arc::new(drivers.into_iter().map(|d| (d.harness().to_owned(), d)).collect());
    let trackers = Arc::new(trackers);
    let core = Arc::new(OnceLock::new());
    let (sink, mut events) = mpsc::unbounded_channel();
    let starts = exec::Starts::default();
    let executor = exec::RunExecutor {
        drivers: drivers.clone(),
        trackers: trackers.clone(),
        core: core.clone(),
        sink,
        hooks,
        running: exec_step::Running::default(),
        starts: starts.clone(),
    };
    let (handle, task) = writer::start(
        conn,
        clock,
        RunHandler {
            config,
            generation: 1,
            stopping: None,
            drivers,
            trackers,
        },
        executor,
        journal,
    );
    let _ = core.set(handle.clone());
    let forward = handle.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            // A dead process is never started again; its record goes or is stopping, and either way its descendants
            // are swept by an effect.
            if matches!(event.event, Normalized::Exited { .. }) {
                starts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&event.instance);
            }
            if forward.send(None, Msg::Driver(event)).await.is_err() {
                return;
            }
        }
    });
    (handle, task)
}

struct RunHandler {
    config: Arc<Config>,
    /// The generation of `config` in this daemon: 1 at start, one more on every reload.
    generation: u64,
    /// The `seq` of `daemon.stopping` once the daemon stops: queued and parked runs wait from then on.
    stopping: Option<i64>,
    drivers: Drivers,
    trackers: beads::Trackers,
}

impl Handler for RunHandler {
    type Msg = Msg;

    fn handle(&mut self, tx: &mut Tx<'_>, msg: Msg) -> Result<Value, Error> {
        match msg {
            Msg::Create(create) => return self.create(tx, create),
            Msg::Finish { run, token, outcome } => return self.finish(tx, &run, &token, &outcome),
            Msg::Fire { project, rig, trigger } => return self.fire_by_hand(tx, &project, &rig, &trigger),
            Msg::Cancel { run, reason } => self.cancel(tx, &run, &reason),
            Msg::CloseGate(gate) => self.close_gate(tx, &gate, "operator"),
            Msg::OpenGate { key } => self.open_gate(tx, &key, None),
            Msg::Decide { run, seq, decision } => self.decide(tx, &run, seq, decision),
            Msg::Answer { run, request, answer } => self.answer(tx, &run, &request, &answer),
            Msg::Instance { run, attempt, handle } => Self::instance(tx, &run, attempt, &handle),
            Msg::InstanceReady { id, harness, handle } => Self::instance_ready(tx, &id, &harness, &handle),
            Msg::InstanceFailed { id, failure } => Self::instance_failed(tx, &id, &failure),
            Msg::InstanceStopped { id, reason } => Self::instance_stopped(tx, &id, &reason),
            Msg::Session { run, attempt, session } => self.session(tx, &run, attempt, &session),
            Msg::PromptReplayed {
                run,
                attempt,
                key,
                text,
            } => prompts::intent(tx, &run, attempt, 1, key, &text),
            Msg::PromptSent { run, attempt, key } => prompts::advance(tx, &run, attempt, Some(key), "sent"),
            Msg::AttemptFailed { run, attempt, failure } => self.launch_failed(tx, &run, attempt, failure),
            Msg::Driver(event) => self.driver_event(tx, event),
            Msg::ExecDone {
                run,
                attempt,
                exit,
                stdout,
                stderr,
            } => self.exec_done(tx, &run, attempt, exit, &stdout, &stderr),
            Msg::HookFailed { event, data } => Self::hook_failed(tx, &event, data),
            Msg::Tracked { payload, result } => self.tracked(tx, &payload, result),
            Msg::Ready {
                project,
                beads,
                changes,
            } => Self::ready(tx, &project, &beads, &changes),
            Msg::TrackerChanged(project) => self.sync_trackers(tx, Some(&project), "changed"),
            Msg::Reload(config) => return self.reload(tx, config),
            Msg::Stopping { reason } => return self.stopping(tx, reason),
        }?;
        Ok(Value::Null)
    }

    fn deadline(&mut self, tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        match id.rsplit_once(':').unwrap_or((id, "")) {
            (gate, "gate") => self.gate_timer(tx, gate),
            (instance, "idle") => Self::instance_idle(tx, instance),
            (_, "trigger") => self.scheduled(tx, id),
            (_, "resync") => self.resync(tx),
            (run, what) => self.on_deadline(tx, run, what),
        }
    }

    fn emitted(&mut self, tx: &mut Tx<'_>, events: &[Value]) -> Result<(), Error> {
        self.dispatch(tx, events)?;
        self.match_triggers(tx, events)
    }

    fn started(&mut self, tx: &mut Tx<'_>) -> Result<(), Error> {
        self.recover(tx)?;
        self.sync_trackers(tx, None, "start")?;
        tx.set_deadline(RESYNC, tx.now_ms() + RESYNC_MS)?;
        self.arm_schedules(tx)
    }
}

/// The deadline of the periodic resync: the trackers are read again this often whatever their watchers say, so a
/// lost signal costs at most this much (spec §5.1).
const RESYNC: &str = "daemon:resync";
const RESYNC_MS: i64 = 5 * 60 * 1000;

impl RunHandler {
    fn resync(&mut self, tx: &mut Tx<'_>) -> Result<(), Error> {
        self.sync_trackers(tx, None, "resync")?;
        tx.set_deadline(RESYNC, tx.now_ms() + RESYNC_MS)
    }
}

fn rejected(reason: impl Into<String>) -> Error {
    Error::Rejected(reason.into())
}

/// An answer as the effect payload and `run.requires_action` decisions write it (contracts 3.2).
fn answer_json(answer: &Answer) -> Value {
    match answer {
        Answer::Allow { updated_input } => json!({ "decision": "allow", "updated_input": updated_input }),
        Answer::Deny { message } => json!({ "decision": "deny", "message": message }),
        Answer::Text { text } => json!({ "decision": "answer", "text": text }),
    }
}

/// An answer in the form of `answer_json` (the API takes it so too); `None` if it is not one.
pub fn answer_from(value: &Value) -> Option<Answer> {
    let text = |key: &str| value[key].as_str().map(str::to_owned);
    Some(match value["decision"].as_str()? {
        "allow" => Answer::Allow {
            updated_input: Some(value["updated_input"].clone()).filter(|v| !v.is_null()),
        },
        "deny" => Answer::Deny {
            message: text("message")?,
        },
        "answer" => Answer::Text { text: text("text")? },
        _ => return None,
    })
}

/// A fresh attempt token: 32 random bytes as hex (contracts 6.3).
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(hex(&bytes))
}

/// The form the state keeps a token in.
fn token_sha256(token: &str) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A failure as events and the attempts table carry it (contracts 1.3).
fn failure_json(f: &Failure) -> Value {
    json!({
        "kind": f.kind, "text": f.text, "scope": f.scope,
        "retry_at": f.retry_at_ms.map(crate::journal::rfc3339_ms),
        "retried_by_harness": f.retried_by_harness, "source": f.source, "raw": f.raw,
        "account": f.account, "provider_model": f.provider_model,
    })
}
