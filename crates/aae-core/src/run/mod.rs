//! Runs (spec §9.3, docs/design/contracts.md 6): the run state machine inside the single writer, the effects that
//! drive a harness through its [`Driver`], and the driver events fed back to the writer as messages.

mod decide;
mod events;
mod exec;
mod exec_step;
mod finish;
mod gates;
mod hooks;
mod machine;
mod queue;
mod read;
mod registry;
mod row;

use crate::{
    clock::Clock,
    config::Config,
    driver::{Answer, Driver, DriverEvent, Failure, InstanceHandle, Normalized, SessionRef},
    journal::Journal,
    writer::{self, CoreHandle, Error, Handler, Tx},
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock, PoisonError},
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle};

/// Ceiling of the core: attempts per run (invariant 16).
pub const MAX_ATTEMPTS: u32 = 20;
pub use crate::process::Exit;
pub use finish::MAX_REMARKS;
pub use gates::{GateSpec, valid_key};
pub use hooks::{ExecHooks, HookRunner};
pub use read::{Reader, RunFilter};

type Drivers = Arc<BTreeMap<String, Arc<dyn Driver>>>;

/// A message to the run machine.
pub enum Msg {
    /// Queues a run; the reply is `{"run": "<id>"}`.
    Create(Create),
    Cancel {
        run: String,
        reason: String,
    },
    /// The operator closes a gate (spec §5.7): automation never removes this closure (invariant 18).
    CloseGate(GateSpec),
    /// The operator opens a gate: every closure of `key` goes.
    OpenGate {
        key: String,
    },
    /// A decision on the deciding event `seq` the run waits for (contracts 3.2).
    Decide {
        run: String,
        seq: i64,
        decision: Decision,
    },
    /// A human answer to the open request of a run in `requires_action`.
    Answer {
        run: String,
        request: String,
        answer: Answer,
    },
    /// The registry's instance `id` runs.
    InstanceReady {
        id: String,
        harness: String,
        handle: InstanceHandle,
    },
    /// The registry's instance `id` could not start.
    InstanceFailed {
        id: String,
        failure: Failure,
    },
    /// The stop of the registry's instance `id` is done.
    InstanceStopped {
        id: String,
    },
    /// The instance of an attempt runs.
    Instance {
        run: String,
        attempt: u32,
        handle: InstanceHandle,
    },
    /// The executor started or resumed the session of an attempt.
    Session {
        run: String,
        attempt: u32,
        session: SessionRef,
    },
    /// A driver call of an attempt failed.
    AttemptFailed {
        run: String,
        attempt: u32,
        failure: Failure,
    },
    Driver(DriverEvent),
    /// The command of an exec step ended; `stdout` and `stderr` are their last 4 KiB.
    ExecDone {
        run: String,
        attempt: u32,
        exit: Exit,
        stdout: String,
        stderr: String,
    },
    /// A hook on `event` failed: the core reports it as `plugin.hook_failed` with `data` (contracts 1.3).
    HookFailed {
        event: Value,
        data: Value,
    },
    /// The finisher of an attempt; the reply is `{"answer": "accepted"|"stale_attempt"|"run_terminal"|"invalid"}`
    /// (contracts 6.3).
    Finish {
        run: String,
        token: String,
        outcome: Outcome,
    },
}

/// The outcome an agent declares (contracts 1.3 `Outcome`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub result: Verdict,
    pub summary: String,
    pub data: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
}

/// A run to create: by hand (`correlation` and `parent_run` absent, depth 0) or by a trigger.
pub struct Create {
    pub project: String,
    pub rig: String,
    /// A formula reference as a project writes it: `name` or `<pack>.<name>`.
    pub formula: String,
    pub inputs: serde_json::Map<String, Value>,
    pub correlation: Option<String>,
    pub depth: u32,
    pub parent_run: Option<String>,
    /// A terminal run whose last session this run continues (spec §9.3); never implicit.
    pub continue_from: Option<String>,
}

/// The decisions of contracts 3.2 the run machine applies.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Every hook on the event finished without a decision: the "no plugin decision" outcome, at once (3.3).
    Abstain,
    /// On `run.preparing`: the session starts in `workdir`, with `env` changed (`None` removes a variable) and
    /// `prompt_append` after the prompt.
    Proceed {
        workdir: Option<std::path::PathBuf>,
        env: BTreeMap<String, Option<crate::config::Sourced<String>>>,
        prompt_append: Option<String>,
    },
    Deny {
        reason: String,
    },
    /// On `run.attempt_failed` (as `NewSession` and `Fail`): `gate`, if any, is closed in the same transaction.
    Resume {
        after: Duration,
        gate: Option<GateSpec>,
    },
    NewSession {
        after: Duration,
        gate: Option<GateSpec>,
    },
    /// On `run.attempt_failed`: close `gate` and wait in `parked` until the run's keys are open, then start the
    /// attempt `then` (`resume` or `new_session`).
    Park {
        gate: GateSpec,
        then: String,
    },
    Fail {
        reason: String,
        gate: Option<GateSpec>,
    },
    /// On `run.turn_ended_without_outcome`: `message` goes as the next prompt of the attempt.
    Continue {
        message: String,
    },
    /// On `run.requires_action`: the answer to the open request.
    Answer(Answer),
    Accept,
    Veto {
        reason: String,
    },
    /// On `run.finishing`: `message` goes as the next prompt of the attempt; the declared outcome is cleared.
    Remark {
        message: String,
    },
}

/// Starts the writer with the run machine on `conn` (opened by `store::open`), one driver per harness.
pub fn start(
    conn: Connection,
    clock: Clock,
    config: Arc<Config>,
    drivers: Vec<Arc<dyn Driver>>,
    hooks: Arc<dyn HookRunner>,
    journal: Journal,
) -> (CoreHandle<Msg>, JoinHandle<Result<(), Error>>) {
    let drivers: Drivers = Arc::new(drivers.into_iter().map(|d| (d.harness().to_owned(), d)).collect());
    let core = Arc::new(OnceLock::new());
    let (sink, mut events) = mpsc::unbounded_channel();
    let starts = exec::Starts::default();
    let executor = exec::RunExecutor {
        drivers: drivers.clone(),
        core: core.clone(),
        sink,
        hooks,
        running: exec_step::Running::default(),
        starts: starts.clone(),
    };
    let (handle, task) = writer::start(conn, clock, RunHandler { config, drivers }, executor, journal);
    let _ = core.set(handle.clone());
    let forward = handle.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            // A dead process is never started again (its record goes or is stopping); its descendants go with it.
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
    drivers: Drivers,
}

impl Handler for RunHandler {
    type Msg = Msg;

    fn handle(&mut self, tx: &mut Tx<'_>, msg: Msg) -> Result<Value, Error> {
        match msg {
            Msg::Create(create) => return self.create(tx, create),
            Msg::Finish { run, token, outcome } => return self.finish(tx, &run, &token, &outcome),
            Msg::Cancel { run, reason } => self.cancel(tx, &run, &reason),
            Msg::CloseGate(gate) => self.close_gate(tx, &gate, "operator"),
            Msg::OpenGate { key } => self.open_gate(tx, &key, None),
            Msg::Decide { run, seq, decision } => self.decide(tx, &run, seq, decision),
            Msg::Answer { run, request, answer } => self.answer(tx, &run, &request, &answer),
            Msg::Instance { run, attempt, handle } => Self::instance(tx, &run, attempt, &handle),
            Msg::InstanceReady { id, harness, handle } => Self::instance_ready(tx, &id, &harness, &handle),
            Msg::InstanceFailed { id, failure } => Self::instance_failed(tx, &id, &failure),
            Msg::InstanceStopped { id } => Self::instance_stopped(tx, &id),
            Msg::Session { run, attempt, session } => self.session(tx, &run, attempt, &session),
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
        }?;
        Ok(Value::Null)
    }

    fn deadline(&mut self, tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        match id.rsplit_once(':').unwrap_or((id, "")) {
            (gate, "gate") => self.gate_timer(tx, gate),
            (instance, "idle") => Self::instance_idle(tx, instance),
            (run, what) => self.on_deadline(tx, run, what),
        }
    }

    fn emitted(&mut self, tx: &mut Tx<'_>, events: &[Value]) -> Result<(), Error> {
        self.dispatch(tx, events)
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
