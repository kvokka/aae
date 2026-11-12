//! The payload shapes of docs/design/contracts.md 1.3 and 5.2, as far as the loader needs them: which paths of
//! `where`, `inputs` and prompt templates can exist (contracts 2.1).

enum Shape {
    /// A scalar or an array: there is no indexing, so no path goes below it.
    Leaf,
    /// Any JSON: every path below it may exist.
    Any,
    Obj(&'static [(&'static str, Shape)]),
}

use Shape::{Any, Leaf, Obj};

const BEAD: Shape = Obj(&[
    ("id", Leaf),
    ("title", Leaf),
    ("description", Leaf),
    ("status", Leaf),
    ("priority", Leaf),
    ("labels", Leaf),
    ("assignee", Leaf),
    ("epic", Leaf),
    ("created_at", Leaf),
    ("updated_at", Leaf),
    ("closed_at", Leaf),
]);

const RUN: Shape = Obj(&[
    ("id", Leaf),
    ("formula", Leaf),
    ("agent", Leaf),
    ("step_kind", Leaf),
    ("state", Leaf),
    ("class", Leaf),
    ("attempt", Leaf),
    ("generation", Leaf),
    ("bead", Leaf),
    ("workdir", Leaf),
    ("depth", Leaf),
    ("parent_run", Leaf),
    ("created_at", Leaf),
    ("started_at", Leaf),
]);

const FAILURE: Shape = Obj(&[
    ("kind", Leaf),
    ("text", Leaf),
    ("scope", Leaf),
    ("retry_at", Leaf),
    ("retried_by_harness", Leaf),
    ("source", Leaf),
    ("raw", Leaf),
    ("account", Leaf),
    ("provider_model", Leaf),
]);

const GATE: Shape = Obj(&[
    ("key", Leaf),
    ("until", Leaf),
    ("reason", Leaf),
    ("closed_by", Leaf),
    ("closed_at", Leaf),
]);

const OUTCOME: Shape = Obj(&[("result", Leaf), ("summary", Leaf), ("data", Any)]);

const INSTANCE: Shape = Obj(&[
    ("id", Leaf),
    ("harness", Leaf),
    ("fingerprint", Leaf),
    ("pid", Leaf),
    ("pgid", Leaf),
]);

const SERVICE: Shape = Obj(&[
    ("id", Leaf),
    ("name", Leaf),
    ("scope", Leaf),
    ("pid", Leaf),
    ("pgid", Leaf),
]);

const REQUEST: Shape = Obj(&[
    ("id", Leaf),
    ("kind", Leaf),
    ("tool", Leaf),
    ("input", Any),
    ("text", Leaf),
]);

/// The envelope fields of the event view (contracts 1.2); `run` is a string here and a `RunView` in payloads.
const ENVELOPE: &[&str] = &["seq", "ts", "type", "project", "rig", "run", "correlation"];

/// The `data` keys of an event type, `None` for a type that 1.3 does not define.
fn payload(event_type: &str) -> Option<&'static [(&'static str, Shape)]> {
    Some(match event_type {
        "daemon.started" => &[("version", Leaf), ("pid", Leaf), ("interrupted_runs", Leaf)],
        "daemon.stopping" => &[("reason", Leaf)],
        "config.reloaded" => &[("config_generation", Leaf), ("restarted_instances", Leaf)],
        "instance.starting" | "instance.ready" | "instance.stopped" => &[("instance", INSTANCE), ("reason", Leaf)],
        "instance.failed" => &[("instance", INSTANCE), ("failure", FAILURE)],
        "service.starting" | "service.ready" | "service.stopped" => &[("service", SERVICE), ("reason", Leaf)],
        "service.failed" => &[("service", SERVICE), ("failure", FAILURE)],
        "run.queued" => &[("run", RUN), ("waiting_for", Leaf), ("gate", GATE)],
        "run.started" => &[("run", RUN)],
        "run.state_changed" => &[
            ("from", Leaf),
            ("to", Leaf),
            ("attempt", Leaf),
            ("generation", Leaf),
            ("reason", Leaf),
        ],
        "run.parked" => &[("gate", GATE), ("then", Leaf)],
        "run.resumed" => &[("gate_key", Leaf)],
        "run.passed" => &[("run", RUN), ("outcome", OUTCOME)],
        "run.failed" => &[("run", RUN), ("class", Leaf), ("reason", Leaf), ("outcome", OUTCOME)],
        "run.cancelled" => &[("run", RUN), ("reason", Leaf)],
        "trigger.fired" => &[
            ("trigger", Leaf),
            ("cause_seq", Leaf),
            ("new_run", Leaf),
            ("inputs", Any),
        ],
        "trigger.skipped" => &[
            ("trigger", Leaf),
            ("cause_seq", Leaf),
            ("reason", Leaf),
            ("detail", Leaf),
        ],
        "gate.closed" => &[("gate", GATE)],
        "gate.opened" => &[("key", Leaf), ("by", Leaf)],
        "bead.created" | "bead.updated" | "bead.closed" | "bead.ready" => {
            &[("bead", BEAD), ("actor", Leaf), ("changed", Leaf)]
        }
        "tracker.sync_failed" => &[("op", Leaf), ("bead", Leaf), ("error", Leaf), ("next_retry_at", Leaf)],
        "plugin.hook_failed" => &[
            ("plugin", Leaf),
            ("hook_type", Leaf),
            ("event_seq", Leaf),
            ("error", Leaf),
            ("exit_code", Leaf),
            ("stderr_tail", Leaf),
        ],
        "resync.diverged" => &[("subject", Leaf), ("id", Leaf), ("expected", Any), ("observed", Any)],
        "run.preparing" => &[
            ("run", RUN),
            ("workdir", Leaf),
            ("env_keys", Leaf),
            ("prompt_append", Leaf),
            ("deadline", Leaf),
        ],
        "run.requires_action" => &[("run", RUN), ("request", REQUEST), ("deadline", Leaf)],
        "run.attempt_failed" => &[
            ("run", RUN),
            ("failure", FAILURE),
            ("attempts", Leaf),
            ("gates", Leaf),
            ("agent_failures_1h", Leaf),
            ("deadline", Leaf),
        ],
        "run.turn_ended_without_outcome" => &[
            ("run", RUN),
            ("stop_reason", Leaf),
            ("remarks", Leaf),
            ("attempts", Leaf),
            ("deadline", Leaf),
        ],
        "run.finishing" => &[
            ("run", RUN),
            ("outcome", OUTCOME),
            ("remarks", Leaf),
            ("deadline", Leaf),
        ],
        _ => return None,
    })
}

fn has(shape: &Shape, path: &str) -> bool {
    let mut shape = shape;
    for key in path.split('.') {
        shape = match shape {
            Any => return true,
            Leaf => return false,
            Obj(fields) => match fields.iter().find(|(name, _)| *name == key) {
                Some((_, inner)) => inner,
                None => return false,
            },
        };
    }
    true
}

/// Whether `path` can exist in the event view of `event_type`; `None` for a type that 1.3 does not define.
pub(crate) fn event_path(event_type: &str, path: &str) -> Option<bool> {
    let data = payload(event_type)?;
    let top = path.split('.').next().unwrap_or_default();
    Some(has(&Obj(data), path) || (path == top && ENVELOPE.contains(&top)))
}

/// Whether `path` exists in a `Bead` (contracts 5.2).
pub(crate) fn bead_path(path: &str) -> bool {
    has(&BEAD, path)
}

/// Whether `path` exists in a `RunView` (contracts 1.3).
pub(crate) fn run_path(path: &str) -> bool {
    has(&RUN, path)
}
