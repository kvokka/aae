//! Orchestrator hooks (spec §11.3, docs/design/contracts.md 3): the dispatcher queues the hooks of an event as an
//! effect that commits with the event; the runner executes them after commit, one after another in plugin order,
//! and sends the combined decision of a deciding event back to the core.

use super::{Decision, Msg, RunHandler, decide::parse};
use crate::{
    BoxFuture,
    config::Sourced,
    driver::Answer,
    process::{self, Exit, Spec},
    writer::{CoreHandle, Error, Event, Tx},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

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
                    "timeout_ms": timeout_ms.min(HOOK_CEILING_MS), "params": plugin.params,
                }));
            }
        }
        // The bundled plugins come last: a declared plugin decides before the default policy.
        let bundled = project.map_or(&self.config.bundled, |p| &p.bundled);
        for bundled in bundled.iter().filter(|b| b.on.contains(&kind)) {
            hooks.push(json!({
                "plugin": bundled.name, "bundled": bundled.name, "params": bundled.params,
                "timeout_ms": HOOK_TIMEOUT_MS,
            }));
        }
        hooks
    }

    /// Queues the hooks of every emitted event. The effect key serializes the hooks of one run in `seq` order,
    /// with the deciding events on their own key: a slow observer never eats the deadline of a decision.
    pub(super) fn dispatch(&mut self, tx: &mut Tx<'_>, events: &[Value]) -> Result<(), Error> {
        for event in events {
            let (Some(kind), Some(seq)) = (event["type"].as_str(), event["seq"].as_i64()) else {
                continue;
            };
            let hooks = self.hooks(event["project"].as_str(), kind);
            if !hooks.is_empty() {
                let run = event["run"].as_str().unwrap_or("-");
                let key = if DECIDING.contains(&kind) {
                    format!("hooks:{run}")
                } else {
                    format!("hooks:{run}:obs")
                };
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

/// A bundled plugin's hook: `(plugin, event, params)` to its decision, `None` for none, or why it failed.
pub type BundledHook = fn(&str, &Value, &Value) -> Result<Option<Value>, String>;

/// The hooks as commands of their plugins; `socket` is the API socket a hook gets in `AAE_SOCKET`; `bundled` runs
/// the hooks of the bundled plugins in process (the daemon passes `aae_plugins::run`).
#[derive(Default)]
pub struct ExecHooks {
    pub socket: Option<PathBuf>,
    pub bundled: Option<BundledHook>,
}

impl HookRunner for ExecHooks {
    fn run<'a>(&'a self, core: &'a CoreHandle<Msg>, payload: &'a Value) -> BoxFuture<'a, ()> {
        Box::pin(run(core, payload, self))
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
async fn run(core: &CoreHandle<Msg>, payload: &Value, hooks: &ExecHooks) {
    let mut event = payload["event"].clone();
    let kind = event["type"].as_str().unwrap_or_default().to_owned();
    let deciding = DECIDING.contains(&kind.as_str());
    let mut prepared = Prepared::default();
    let (mut first, mut veto, mut remarks) = (None, None, Vec::new());
    for hook in payload["hooks"].as_array().into_iter().flatten() {
        let plugin = hook["plugin"].as_str().unwrap_or_default();
        // Hooks on observed events may only exit 0 or fail (contracts 3.2).
        let said = match hook["bundled"].as_str() {
            Some(name) => bundled(hooks.bundled, name, hook, &event).await,
            None => invoke(hook, &event, hooks.socket.as_deref()).await,
        };
        let said = said.and_then(|said| match said {
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

/// Runs a bundled plugin's hook off the async threads (it may call git) within the hook timeout. A timeout ends
/// the wait only: the blocking task and a `git` it started run on to their end.
async fn bundled(run: Option<BundledHook>, name: &str, hook: &Value, event: &Value) -> Result<Said, Failed> {
    let failed = |error, stderr: String| Failed {
        error,
        exit_code: None,
        stderr,
    };
    let run = run.ok_or_else(|| failed("spawn", format!("this daemon does not run the bundled `{name}`")))?;
    let (name, event, params) = (name.to_owned(), event.clone(), hook["params"].clone());
    let timeout = Duration::from_millis(hook["timeout_ms"].as_u64().unwrap_or(30_000));
    let task = tokio::task::spawn_blocking(move || run(&name, &event, &params));
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(Ok(Some(decision)))) => Ok(Said::Decided(decision)),
        Ok(Ok(Ok(None))) => Ok(Said::Nothing),
        Ok(Ok(Err(text))) => Err(failed("exit", text)),
        Ok(Err(panic)) => Err(failed("exit", panic.to_string())),
        Err(_) => Err(failed("timeout", String::new())),
    }
}

async fn invoke(hook: &Value, event: &Value, socket: Option<&Path>) -> Result<Said, Failed> {
    let command: Vec<String> = serde_json::from_value(hook["command"].clone()).unwrap_or_default();
    let dir = PathBuf::from(hook["dir"].as_str().unwrap_or("/"));
    let mut env = process::base_env();
    env.push(("AAE_PLUGIN_DIR".to_owned(), dir.to_string_lossy().into_owned()));
    let params = hook.get("params").filter(|p| p.is_object());
    env.extend(params.map(|p| ("AAE_PLUGIN_PARAMS".to_owned(), p.to_string())));
    env.extend(socket.map(|s| ("AAE_SOCKET".to_owned(), s.to_string_lossy().into_owned())));
    let stdin = format!("{event}\n");
    let timeout = Duration::from_millis(hook["timeout_ms"].as_u64().unwrap_or(30_000));
    let out = process::run(Spec {
        command: &command,
        cwd: &dir,
        env: &env,
        stdin: stdin.as_bytes(),
        timeout,
        keep: usize::try_from(MAX_STDOUT).unwrap_or(usize::MAX),
        stop: None,
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
                _ => Decision::Fail { reason, gate: None },
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
