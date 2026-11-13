//! The messages of the run machine and the values they carry.

use super::{Exit, GateSpec};
use crate::{
    driver::{Answer, DriverEvent, Failure, InstanceHandle, PromptKey, SessionRef},
    tracker::{Bead, Change, TrackerError},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

/// A message to the run machine.
pub enum Msg {
    /// Queues a run; the reply is `{"run": "<id>"}`.
    Create(Create),
    Cancel {
        run: String,
        reason: String,
    },
    /// Fires `trigger` (`<pack>.<name>`, or the name of a project trigger) on a rig by hand, as its schedule would;
    /// the reply is `{"run": "<id>"}` or `{"skipped": <reason>, "detail": <text>}`.
    Fire {
        project: String,
        rig: String,
        trigger: String,
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
    /// The stop of the registry's instance `id` is done; `reason` is the one of the stop (contracts 1.3).
    InstanceStopped {
        id: String,
        reason: String,
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
    /// The launch of an attempt sends again the prompt the daemon died under, which the harness never got: it is
    /// the attempt's first prompt now (contracts 6.4).
    PromptReplayed {
        run: String,
        attempt: u32,
        key: PromptKey,
        text: String,
    },
    /// `send_prompt` of the attempt returned for prompt `key`.
    PromptSent {
        run: String,
        attempt: u32,
        key: PromptKey,
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
    /// The result of the tracker call of the effect `payload`.
    Tracked {
        payload: Value,
        result: Result<Option<Bead>, TrackerError>,
    },
    /// The writes to the tracker of `project` after its cursor and its ready set.
    Ready {
        project: String,
        beads: Vec<Bead>,
        changes: Vec<Change>,
    },
    /// The tracker of the project changed: its ready set is read again.
    TrackerChanged(String),
    /// The config file was read again; the reply is `{config_generation, restarted_instances}`.
    Reload(std::sync::Arc<crate::config::Config>),
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
