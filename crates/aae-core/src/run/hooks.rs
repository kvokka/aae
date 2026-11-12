//! Orchestrator hooks (spec §11.3, docs/design/contracts.md 3): the dispatcher queues the hooks of an event as an
//! effect that commits with the event; the runner executes them after commit, one after another in plugin order,
//! and sends the combined decision of a deciding event back to the core.

use super::{Decision, Msg, RunHandler, answer_from};
use crate::{
    BoxFuture,
    config::{Layer, Sourced},
    driver::Answer,
    process::{self, Exit, Spec},
    writer::{CoreHandle, Error, Event, Tx},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

const HOOK_TIMEOUT_MS: i64 = 30_000;
const HOOK_CEILING_MS: i64 = 300_000;
const MAX_STDOUT: u64 = 64 * 1024;
const STDERR_TAIL: usize = 4 * 1024;

/// The events whose hooks may decide (spec §5.4).
const DECIDING: [&str; 5] = [
    "run.preparing",
    "run.requires_action",
    "run.attempt_failed",
    "run.turn_ended_without_outcome",
    "run.finishing",
];

impl RunHandler {
    /// The hooks on `kind` in connection order (orchestrator plugins, then those of `project`), as the runner
    /// takes them.
    pub(super) fn hooks(&self, project: Option<&str>, kind: &str) -> Vec<Value> {
        let project = self.config.projects.iter().find(|p| Some(p.name.as_str()) == project);
        let plugins = self
            .config
            .plugins
            .iter()
            .chain(project.into_iter().flat_map(|p| &p.plugins));
        let mut hooks = Vec::new();
        for plugin in plugins {
            for hook in plugin.hooks.iter().filter(|h| h.on == kind) {
                let timeout_ms = hook
                    .timeout
                    .map_or(HOOK_TIMEOUT_MS, |t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX));
                let mut command = hook.command.clone();
                // A program path is relative to the plugin directory; a bare name is looked up on `PATH`.
                if let Some(program) = command.first_mut().filter(|p| p.contains('/')) {
                    *program = plugin.root.join(&*program).to_string_lossy().into_owned();
                }
                hooks.push(json!({
                    "plugin": plugin.name, "dir": plugin.root, "command": command,
                    "timeout_ms": timeout_ms.min(HOOK_CEILING_MS),
                }));
            }
        }
        hooks
    }

    /// Queues the hooks of every emitted event; the effect key serializes the hooks of one run in `seq` order.
    pub(super) fn dispatch(&mut self, tx: &mut Tx<'_>, events: &[Value]) -> Result<(), Error> {
        for event in events {
            let (Some(kind), Some(seq)) = (event["type"].as_str(), event["seq"].as_i64()) else {
                continue;
            };
            let hooks = self.hooks(event["project"].as_str(), kind);
            if !hooks.is_empty() {
                let key = format!("hooks:{}", event["run"].as_str().unwrap_or("-"));
                let payload = json!({ "op": "hooks", "event": event, "hooks": hooks });
                tx.effect(&key, &seq.to_string(), &payload)?;
            }
        }
        Ok(())
    }

    pub(super) fn hook_failed(tx: &mut Tx<'_>, event: &Value, data: Value) -> Result<(), Error> {
        let text = |key: &str| event[key].as_str().map(str::to_owned);
        let event = Event {
            kind: "plugin.hook_failed".to_owned(),
            project: text("project"),
            rig: text("rig"),
            run: text("run"),
            correlation: text("correlation"),
            data,
        };
        tx.emit(&event).map(drop)
    }
}

/// Runs the hooks of one event, `{"event": <envelope>, "hooks": [...]}`, after commit and sends the core what
/// they decided (contracts 3.1, 3.3). The daemon uses [`ExecHooks`].
pub trait HookRunner: Send + Sync + 'static {
    fn run<'a>(&'a self, core: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()>;
}

/// The hooks as commands of their plugins.
pub struct ExecHooks;

impl HookRunner for ExecHooks {
    fn run<'a>(&'a self, core: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(run(core, payload))
    }
}

/// What one hook said.
enum Said {
    Nothing,
    Decided(Value),
    Blocked(String),
}

/// A hook failure as `plugin.hook_failed` reports it.
struct Failed {
    error: &'static str,
    exit_code: Option<i32>,
    stderr: String,
}

/// Runs the hooks of one event and, for a deciding event, sends the decision (contracts 3.3).
async fn run(core: &CoreHandle<Msg>, payload: &Value) {
    let mut event = payload["event"].clone();
    let kind = event["type"].as_str().unwrap_or_default().to_owned();
    let deciding = DECIDING.contains(&kind.as_str());
    let mut prepared = Prepared::default();
    let (mut first, mut veto, mut remarks) = (None, None, Vec::new());
    for hook in payload["hooks"].as_array().into_iter().flatten() {
        let plugin = hook["plugin"].as_str().unwrap_or_default();
        // Hooks on observed events may only exit 0 or fail (contracts 3.2).
        let said = invoke(hook, &event).await.and_then(|said| match said {
            Said::Blocked(stderr) if !deciding => Err(Failed {
                error: "exit",
                exit_code: Some(2),
                stderr,
            }),
            Said::Decided(_) if !deciding => Ok(None),
            said => decision(&kind, said, plugin, &event),
        });
        let decision = match said {
            Ok(decision) => decision,
            Err(failed) => {
                // A failing hook on the report of a hook failure is not reported again: no loop.
                if kind != "plugin.hook_failed" {
                    report(core, &event, plugin, &kind, failed).await;
                }
                let reason = format!("hook_failed:{plugin}");
                match kind.as_str() {
                    "run.preparing" => Some(Decision::Deny { reason }),
                    "run.finishing" => Some(Decision::Veto { reason }),
                    _ => None,
                }
            }
        };
        match (kind.as_str(), decision) {
            (_, None) => {}
            (
                "run.preparing",
                Some(Decision::Proceed {
                    workdir,
                    env,
                    prompt_append,
                }),
            ) => {
                prepared.add(workdir, env, prompt_append, &mut event);
            }
            ("run.finishing", Some(Decision::Veto { reason })) => {
                veto.get_or_insert(reason);
            }
            ("run.finishing", Some(Decision::Remark { message })) => remarks.push(message),
            ("run.finishing", Some(_)) => {}
            (_, Some(decision)) => {
                first = Some(decision);
                break;
            }
        }
    }
    if !deciding {
        return;
    }
    let decision = match kind.as_str() {
        "run.preparing" => first.unwrap_or(Decision::Proceed {
            workdir: prepared.workdir,
            env: prepared.env,
            prompt_append: Some(prepared.append.join("\n\n")).filter(|a| !a.is_empty()),
        }),
        "run.finishing" => match (veto, remarks.is_empty()) {
            (Some(reason), _) => Decision::Veto { reason },
            (None, false) => Decision::Remark {
                message: remarks.join("\n\n"),
            },
            (None, true) => Decision::Accept,
        },
        _ => first.unwrap_or(Decision::Abstain),
    };
    let msg = Msg::Decide {
        run: event["run"].as_str().unwrap_or_default().to_owned(),
        seq: event["seq"].as_i64().unwrap_or_default(),
        decision,
    };
    // A stale decision is refused and changes nothing (contracts 3.4).
    let _ = core.send(None, msg).await;
}

/// The `run.preparing` modifications so far; each next hook sees them in its event.
#[derive(Default)]
struct Prepared {
    workdir: Option<PathBuf>,
    env: BTreeMap<String, Option<Sourced<String>>>,
    append: Vec<String>,
}

impl Prepared {
    fn add(
        &mut self,
        workdir: Option<PathBuf>,
        env: BTreeMap<String, Option<Sourced<String>>>,
        append: Option<String>,
        event: &mut Value,
    ) {
        let data = &mut event["data"];
        if let Some(workdir) = workdir {
            data["workdir"] = json!(workdir);
            self.workdir = Some(workdir);
        }
        let mut keys: Vec<String> = serde_json::from_value(data["env_keys"].take()).unwrap_or_default();
        for (name, value) in env {
            keys.retain(|k| *k != name);
            if value.is_some() {
                keys.push(name.clone());
            }
            self.env.insert(name, value);
        }
        keys.sort();
        data["env_keys"] = json!(keys);
        self.append.extend(append);
        data["prompt_append"] = json!(Some(self.append.join("\n\n")).filter(|a| !a.is_empty()));
    }
}

async fn invoke(hook: &Value, event: &Value) -> Result<Said, Failed> {
    let command: Vec<String> = serde_json::from_value(hook["command"].clone()).unwrap_or_default();
    let dir = PathBuf::from(hook["dir"].as_str().unwrap_or("/"));
    let mut env: Vec<(String, String)> = ["PATH", "HOME", "LANG"]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_owned(), v)))
        .collect();
    env.push(("AAE_PLUGIN_DIR".to_owned(), dir.to_string_lossy().into_owned()));
    let stdin = format!("{event}\n");
    let timeout = Duration::from_millis(hook["timeout_ms"].as_u64().unwrap_or(30_000));
    let out = process::run(Spec {
        command: &command,
        cwd: &dir,
        env: &env,
        stdin: stdin.as_bytes(),
        timeout,
        keep: usize::try_from(MAX_STDOUT).unwrap_or(usize::MAX),
    })
    .await;
    let cut = out.stderr.len().saturating_sub(STDERR_TAIL);
    let stderr = String::from_utf8_lossy(&out.stderr[cut..]).trim().to_owned();
    let failed = |error, exit_code| Failed {
        error,
        exit_code,
        stderr: stderr.clone(),
    };
    match out.exit {
        Exit::Code(0) if out.stdout_total > MAX_STDOUT => Err(failed("output", Some(0))),
        Exit::Code(0) if out.stdout.trim_ascii().is_empty() => Ok(Said::Nothing),
        Exit::Code(0) => match serde_json::from_slice::<Value>(&out.stdout) {
            Ok(value) if value.is_object() => Ok(Said::Decided(value)),
            _ => Err(failed("output", Some(0))),
        },
        Exit::Code(2) => Ok(Said::Blocked(stderr.clone())),
        Exit::Code(code) => Err(failed("exit", Some(code))),
        Exit::Signal(_) => Err(failed("exit", None)),
        Exit::TimedOut => Err(failed("timeout", None)),
        Exit::SpawnFailed(text) => Err(Failed {
            error: "spawn",
            exit_code: None,
            stderr: text,
        }),
    }
}

/// The decision of one hook on a deciding event; a decision not allowed for the event is a failure (3.2).
fn decision(kind: &str, said: Said, plugin: &str, event: &Value) -> Result<Option<Decision>, Failed> {
    let value = match said {
        Said::Nothing => return Ok(None),
        Said::Blocked(reason) => {
            return Ok(Some(match kind {
                "run.preparing" => Decision::Deny { reason },
                "run.requires_action" => Decision::Answer(Answer::Deny { message: reason }),
                "run.finishing" => Decision::Veto { reason },
                _ => Decision::Fail { reason },
            }));
        }
        Said::Decided(value) => value,
    };
    parse(kind, &value, plugin, event).map(Some).ok_or(Failed {
        error: "output",
        exit_code: Some(0),
        stderr: format!("decision not allowed on `{kind}`: {value}"),
    })
}

fn parse(kind: &str, v: &Value, plugin: &str, event: &Value) -> Option<Decision> {
    let text = |key: &str| v[key].as_str().map(str::to_owned);
    let after = || match &v["after"] {
        Value::Null => Some(Duration::ZERO),
        after => crate::config::duration(after.as_str()?).ok(),
    };
    Some(match (kind, v["decision"].as_str()?) {
        ("run.preparing", "proceed") => {
            let workdir = match &v["workdir"] {
                Value::Null => None,
                dir => Some(PathBuf::from(dir.as_str()?)),
            };
            if workdir.as_ref().is_some_and(|d| !d.is_absolute()) {
                return None;
            }
            let mut env = BTreeMap::new();
            for (name, value) in v["env"].as_object().into_iter().flatten() {
                let value = match value {
                    Value::Null => None,
                    value => Some(Sourced {
                        value: value.as_str()?.to_owned(),
                        from: Layer::Plugin(plugin.to_owned()),
                    }),
                };
                env.insert(name.clone(), value);
            }
            Decision::Proceed {
                workdir,
                env,
                prompt_append: match &v["prompt_append"] {
                    Value::Null => None,
                    append => Some(append.as_str()?.to_owned()),
                },
            }
        }
        ("run.preparing", "deny") => Decision::Deny {
            reason: text("reason")?,
        },
        ("run.requires_action", "answer") if event["data"]["request"]["kind"] != "question" => return None,
        ("run.requires_action", _) => Decision::Answer(answer_from(v)?),
        ("run.attempt_failed", "resume") => Decision::Resume { after: after()? },
        ("run.attempt_failed", "new_session") => Decision::NewSession { after: after()? },
        ("run.attempt_failed" | "run.turn_ended_without_outcome", "fail") => Decision::Fail {
            reason: text("reason")?,
        },
        ("run.turn_ended_without_outcome", "continue") => Decision::Continue {
            message: text("message")?,
        },
        ("run.finishing", "accept") => Decision::Accept,
        ("run.finishing", "veto") => Decision::Veto {
            reason: text("reason")?,
        },
        ("run.finishing", "remark") => Decision::Remark {
            message: text("message")?,
        },
        _ => return None,
    })
}

async fn report(core: &CoreHandle<Msg>, event: &Value, plugin: &str, kind: &str, failed: Failed) {
    let data = json!({
        "plugin": plugin, "hook_type": kind, "event_seq": event["seq"], "error": failed.error,
        "exit_code": failed.exit_code, "stderr_tail": failed.stderr,
    });
    let _ = core
        .send(
            None,
            Msg::HookFailed {
                event: event.clone(),
                data,
            },
        )
        .await;
}
