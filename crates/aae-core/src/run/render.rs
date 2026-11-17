//! The prompt, the step timeout and the config snapshot of a run, rendered from the config of now.

use super::{RunHandler, row::Row};
use crate::{
    config::{EffectiveConfig, Scope, StepKind},
    writer::Tx,
};
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::path::PathBuf;

pub(super) const STEP_TIMEOUT_MS: i64 = 45 * 60_000;
/// The time a drain step has for every bead under the run's bead, and a formula step for its run
/// (docs/design/config.md).
pub(super) const DRAIN_TIMEOUT_MS: i64 = 24 * 3_600_000;
/// The time an await step waits for its outcome (docs/design/config.md).
pub(super) const AWAIT_TIMEOUT_MS: i64 = 7 * 24 * 3_600_000;

impl RunHandler {
    /// The prompt, the step timeout and the config snapshot of a run, from the config of now: the effective config
    /// of an ai step; `{"command", "env"}` of an exec step; `{"drain", "pool", "on_item_failure"}` of a drain step.
    /// `None`: the step's `if` does not hold, the run skips it (docs/design/config.md).
    pub(super) fn render(&self, tx: &Tx<'_>, row: &Row) -> Result<Option<(Option<String>, i64, Value)>, String> {
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let project = project.ok_or(format!("project `{}` is gone from the config", row.project))?;
        let rig = project
            .place(&row.rig)
            .ok_or(format!("rig `{}` is gone from the config", row.rig))?;
        let (_, formula) = project.formula(&row.formula)?;
        let timeout_ms = |t: &Option<std::time::Duration>, default| {
            t.map_or(default, |t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX))
        };
        let step = formula.step(&row.step);
        let step = step.ok_or(format!("step `{}` is gone from `{}`", row.step, row.formula))?;
        let mut rig_view = json!({ "name": row.rig, "path": rig.path });
        for (key, value) in &rig.vars {
            rig_view[key] = json!(value);
        }
        let mut context = json!({
            "inputs": serde_json::from_str::<Value>(&row.inputs).unwrap_or_default(),
            "rig": rig_view, "run": row.view(),
            "bead": row.bead_data.as_deref().map(serde_json::from_str::<Value>).transpose().ok().flatten(),
            "project": { "name": project.name, "path": project.dir },
            "steps": step_results(tx.sql(), &row.id, &row.step).map_err(|e| e.to_string())?,
            "back_from": back_from(tx.sql(), &row.id).map_err(|e| e.to_string())?,
        });
        // A repeat of the step does not ask again: the step already ran in this entry.
        if row.step_attempt == 1 && !step.condition.matches(&context) {
            return Ok(None);
        }
        // The environment of an exec step or a check script (docs/design/config.md): the tracker by name, as in a
        // session, then `[exec]`, its values as written (decision 137).
        let exec = match &step.kind {
            StepKind::Exec { env, .. } => env,
            _ => &project.exec,
        };
        let mut env: serde_json::Map<String, Value> = (project.tracker_env(Some(rig)))
            .map(|(name, value)| (name.clone(), json!({ "value": value })))
            .collect();
        env.extend((exec.env.iter()).map(|(name, value)| (name.clone(), json!(value))));
        let inherit_env = exec.inherit_env.as_ref().is_some_and(|i| i.value);
        let timeout = match (&step.kind, row.step_kind.as_str()) {
            (StepKind::Exec { command, timeout, .. }, "exec") => {
                let config = json!({ "command": command, "env": env, "inherit_env": inherit_env });
                return Ok(Some((None, timeout_ms(timeout, STEP_TIMEOUT_MS), config)));
            }
            // The environment is for the check script of the step (docs/design/config.md).
            (
                StepKind::Drain {
                    formula,
                    timeout,
                    pool,
                    on_item_failure,
                },
                "drain",
            ) => {
                let config = json!({ "drain": formula.to_string(), "pool": pool,
                    "on_item_failure": on_item_failure, "env": env, "inherit_env": inherit_env });
                return Ok(Some((None, timeout_ms(timeout, DRAIN_TIMEOUT_MS), config)));
            }
            // The inputs of the formula's run: the parent's that it declares, its task among them (the child works on
            // the bead the parent holds), then the step's own over them (docs/design/config.md).
            (
                StepKind::Formula {
                    formula: run,
                    inputs,
                    timeout,
                },
                "formula",
            ) => {
                let (_, child) = project.formula(&run.to_string())?;
                let own: Value = serde_json::from_str(&row.inputs).unwrap_or_default();
                let mut given: Map<String, Value> = (child.inputs.keys())
                    .filter_map(|k| Some((k.clone(), own.get(k)?.clone())))
                    .collect();
                let explicit = inputs
                    .resolve_in(&context)
                    .map_err(|e| format!("step `{}`: {e}", row.step))?;
                given.extend(explicit.as_object().cloned().unwrap_or_default());
                let config =
                    json!({ "formula": run.to_string(), "inputs": given, "env": env, "inherit_env": inherit_env });
                return Ok(Some((None, timeout_ms(timeout, DRAIN_TIMEOUT_MS), config)));
            }
            (
                StepKind::Await {
                    timeout,
                    fail_on_timeout,
                },
                "await",
            ) => {
                let config = json!({ "fail_on_timeout": fail_on_timeout });
                return Ok(Some((None, timeout_ms(timeout, AWAIT_TIMEOUT_MS), config)));
            }
            (StepKind::Ai { timeout, .. }, "ai") => timeout,
            _ => return Err(format!("step `{}` of `{}` changed its kind", row.step, row.formula)),
        };
        let (agent, config) =
            (self.config).for_run_as(&row.project, &row.rig, &row.formula, &row.step, row.as_agent.as_deref())?;
        if agent != row.agent {
            return Err(format!("`{}` now runs agent `{agent}`", row.formula));
        }
        // The run's agent brings its template: the step's, or the one a bead's `agent:` label names
        // (docs/design/config.md).
        let prompt = self.config.prompt(&row.project, &agent)?;
        context["remarks"] = json!(remarks(tx.sql(), &row.id).map_err(|e| e.to_string())?);
        context["agent"] = json!(prompt.files);
        let mut text = prompt.template.render(&context).map_err(|e| format!("prompt: {e}"))?;
        // The remark of a repeat reaches the agent even when its template has no place for remarks
        // (docs/design/config.md).
        let repeat = step_remark(tx.sql(), &row.id).map_err(|e| e.to_string())?;
        if !repeat.is_empty() && prompt.template.paths().all(|p| p != "remarks") {
            text = format!("{}\n\n{repeat}", text.trim_end());
        }
        if text.trim().is_empty() {
            return Err("prompt: empty after substitution".to_owned());
        }
        // The check script of the step gets the environment of an exec step, not the agent's.
        let mut config = json!(config);
        config["exec"] = json!({ "env": env, "inherit_env": inherit_env });
        Ok(Some((Some(text), timeout_ms(timeout, STEP_TIMEOUT_MS), config)))
    }

    /// Where the session of `row`'s step works: the root for an agent of the root (docs/design/config.md), else the
    /// run's workdir.
    pub(super) fn session_dir(&self, row: &Row, config: &EffectiveConfig) -> PathBuf {
        let root = (self.config.projects.iter().find(|p| p.name == row.project)).map(|p| &p.root);
        match root {
            Some((name, root)) if config.scope == Scope::Root && *name != row.rig => root.path.clone(),
            _ => PathBuf::from(&row.workdir),
        }
    }
}

/// The outcome `{result, summary, data}` of each step of `run` but `except`, by step name: the last of its attempts,
/// unless the step was skipped since (docs/design/config.md).
pub(super) fn step_results(conn: &Connection, run: &str, except: &str) -> rusqlite::Result<Map<String, Value>> {
    let mut stmt = conn.prepare(
        "SELECT step, outcome FROM attempts a WHERE run = ?1 AND step != ?2 AND outcome IS NOT NULL AND skipped = 0 \
         AND n = (SELECT MAX(n) FROM attempts WHERE run = a.run AND step = a.step)",
    )?;
    let rows = stmt.query_map([run, except], |r| {
        let outcome: String = r.get(1)?;
        Ok((r.get(0)?, serde_json::from_str(&outcome).unwrap_or_default()))
    })?;
    rows.collect()
}

/// The remarks of the checks that last sent `run` back to an earlier step, then the remark of the execution of its
/// step before this one; empty in the first execution of its first round (docs/design/config.md).
pub(super) fn remarks(conn: &Connection, run: &str) -> rusqlite::Result<String> {
    let both = "TRIM(COALESCE(remarks, '') || char(10, 10) || COALESCE(step_remarks, ''), char(10))";
    conn.query_row(&format!("SELECT {both} FROM runs WHERE id = ?1"), [run], |r| r.get(0))
}

/// The names of the checks that sent `run` back into its current round; empty in the first (docs/design/config.md).
fn back_from(conn: &Connection, run: &str) -> rusqlite::Result<Value> {
    let names: String = conn.query_row("SELECT COALESCE(back_from, '[]') FROM runs WHERE id = ?1", [run], |r| {
        r.get(0)
    })?;
    Ok(serde_json::from_str(&names).unwrap_or_default())
}

fn step_remark(conn: &Connection, run: &str) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT COALESCE(step_remarks, '') FROM runs WHERE id = ?1",
        [run],
        |r| r.get(0),
    )
}
