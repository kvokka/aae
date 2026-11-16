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
/// The time a drain step has for every bead under the run's bead (spec §9.1).
pub(super) const DRAIN_TIMEOUT_MS: i64 = 24 * 3_600_000;

impl RunHandler {
    /// The prompt, the step timeout and the config snapshot of a run, from the config of now: the effective config
    /// of an ai step; `{"command", "env"}` of an exec step; `{"drain", "pool", "on_item_failure"}` of a drain step.
    pub(super) fn render(&self, tx: &Tx<'_>, row: &Row) -> Result<(Option<String>, i64, Value), String> {
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let project = project.ok_or(format!("project `{}` is gone from the config", row.project))?;
        let rig = project
            .place(&row.rig)
            .ok_or(format!("rig `{}` is gone from the config", row.rig))?;
        let (_, formula) = project.formula(&row.formula)?;
        let timeout_ms = |t: &Option<std::time::Duration>, default| {
            t.map_or(default, |t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX))
        };
        let step = formula.step(&row.step).map(|s| &s.kind);
        let step = step.ok_or(format!("step `{}` is gone from `{}`", row.step, row.formula))?;
        // The tracker by name, as in a session (spec §10.2).
        let env: serde_json::Map<String, Value> = (project.tracker_env(Some(rig)))
            .map(|(name, value)| (name.clone(), json!({ "value": value })))
            .collect();
        let timeout = match (step, row.step_kind.as_str()) {
            (StepKind::Exec { command, timeout }, "exec") => {
                let config = json!({ "command": command, "env": env });
                return Ok((None, timeout_ms(timeout, STEP_TIMEOUT_MS), config));
            }
            // The environment is for the check script of the step (spec §9.1).
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
                    "on_item_failure": on_item_failure, "env": env });
                return Ok((None, timeout_ms(timeout, DRAIN_TIMEOUT_MS), config));
            }
            (StepKind::Ai { timeout, .. }, "ai") => timeout,
            _ => return Err(format!("step `{}` of `{}` changed its kind", row.step, row.formula)),
        };
        let (agent, config) =
            (self.config).for_run_as(&row.project, &row.rig, &row.formula, &row.step, row.as_agent.as_deref())?;
        if agent != row.agent {
            return Err(format!("`{}` now runs agent `{agent}`", row.formula));
        }
        // The run's agent brings its template: the step's, or the one a bead's `agent:` label names (spec §8.1).
        let prompt = self.config.prompt(&row.project, &agent)?;
        let mut rig_view = json!({ "name": row.rig, "path": rig.path });
        for (key, value) in &rig.vars {
            rig_view[key] = json!(value);
        }
        let context = json!({
            "inputs": serde_json::from_str::<Value>(&row.inputs).unwrap_or_default(),
            "rig": rig_view, "run": row.view(),
            "bead": row.bead_data.as_deref().map(serde_json::from_str::<Value>).transpose().ok().flatten(),
            "project": { "name": project.name, "path": project.dir },
            "steps": step_results(tx.sql(), &row.id, &row.step).map_err(|e| e.to_string())?,
            "remarks": remarks(tx.sql(), &row.id).map_err(|e| e.to_string())?,
            "agent": prompt.files,
        });
        let mut text = prompt.template.render(&context).map_err(|e| format!("prompt: {e}"))?;
        // The remark of a repeat reaches the agent even when its template has no place for remarks (spec §9.1).
        let repeat = step_remark(tx.sql(), &row.id).map_err(|e| e.to_string())?;
        if !repeat.is_empty() && prompt.template.paths().all(|p| p != "remarks") {
            text = format!("{}\n\n{repeat}", text.trim_end());
        }
        if text.trim().is_empty() {
            return Err("prompt: empty after substitution".to_owned());
        }
        Ok((Some(text), timeout_ms(timeout, STEP_TIMEOUT_MS), json!(config)))
    }

    /// Where the session of `row`'s step works: the root for an agent of the root (spec §8.1), else the run's
    /// workdir.
    pub(super) fn session_dir(&self, row: &Row, config: &EffectiveConfig) -> PathBuf {
        let root = (self.config.projects.iter().find(|p| p.name == row.project)).map(|p| &p.root);
        match root {
            Some((name, root)) if config.scope == Scope::Root && *name != row.rig => root.path.clone(),
            _ => PathBuf::from(&row.workdir),
        }
    }
}

/// The outcome `{result, summary, data}` of each step of `run` but `except`, by step name: the last of its attempts
/// (spec §9.1).
pub(super) fn step_results(conn: &Connection, run: &str, except: &str) -> rusqlite::Result<Map<String, Value>> {
    let mut stmt = conn.prepare(
        "SELECT step, outcome FROM attempts a WHERE run = ?1 AND step != ?2 AND outcome IS NOT NULL \
         AND n = (SELECT MAX(n) FROM attempts WHERE run = a.run AND step = a.step)",
    )?;
    let rows = stmt.query_map([run, except], |r| {
        let outcome: String = r.get(1)?;
        Ok((r.get(0)?, serde_json::from_str(&outcome).unwrap_or_default()))
    })?;
    rows.collect()
}

/// The remarks of the checks that last sent `run` back to an earlier step, then the remark of the execution of its
/// step before this one; empty in the first execution of its first round (spec §9.1).
pub(super) fn remarks(conn: &Connection, run: &str) -> rusqlite::Result<String> {
    let both = "TRIM(COALESCE(remarks, '') || char(10, 10) || COALESCE(step_remarks, ''), char(10))";
    conn.query_row(&format!("SELECT {both} FROM runs WHERE id = ?1"), [run], |r| r.get(0))
}

fn step_remark(conn: &Connection, run: &str) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT COALESCE(step_remarks, '') FROM runs WHERE id = ?1",
        [run],
        |r| r.get(0),
    )
}
