//! The prompt, the step timeout and the config snapshot of a run, rendered from the config of now.

use super::{RunHandler, row::Row};
use crate::{config::StepKind, writer::Tx};
use rusqlite::Connection;
use serde_json::{Map, Value, json};

pub(super) const STEP_TIMEOUT_MS: i64 = 45 * 60_000;

impl RunHandler {
    /// The prompt, the step timeout and the config snapshot of a run, from the config of now: the effective config
    /// of a prompt step; `{"command", "env"}` of an exec step.
    pub(super) fn render(&self, tx: &Tx<'_>, row: &Row) -> Result<(Option<String>, i64, Value), String> {
        let project = self.config.projects.iter().find(|p| p.name == row.project);
        let project = project.ok_or(format!("project `{}` is gone from the config", row.project))?;
        let rig = project
            .rigs
            .get(&row.rig)
            .ok_or(format!("rig `{}` is gone from the config", row.rig))?;
        let (_, formula) = project.formula(&row.formula)?;
        let timeout_ms = |t: &Option<std::time::Duration>| {
            t.map_or(STEP_TIMEOUT_MS, |t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX))
        };
        let step = formula.step(&row.step).map(|s| &s.kind);
        let step = step.ok_or(format!("step `{}` is gone from `{}`", row.step, row.formula))?;
        let (prompt, timeout) = match (step, row.exec()) {
            (StepKind::Exec { command, timeout }, true) => {
                // The tracker by name, as in a session (spec §10.2).
                let env: serde_json::Map<String, Value> = (project.tracker.env.iter())
                    .map(|(name, value)| (name.clone(), json!({ "value": value })))
                    .collect();
                return Ok((None, timeout_ms(timeout), json!({ "command": command, "env": env })));
            }
            (StepKind::Prompt { prompt, timeout, .. }, false) => (prompt, timeout),
            _ => return Err(format!("step `{}` of `{}` changed its kind", row.step, row.formula)),
        };
        let (agent, config) = self.config.for_run(&row.project, &row.rig, &row.formula, &row.step)?;
        if agent != row.agent {
            return Err(format!("`{}` now runs agent `{agent}`", row.formula));
        }
        let mut rig_view = json!({ "name": row.rig, "path": rig.path });
        for (key, value) in &rig.vars {
            rig_view[key] = json!(value);
        }
        let context = json!({
            "inputs": serde_json::from_str::<Value>(&row.inputs).unwrap_or_default(),
            "rig": rig_view, "run": row.view(),
            "bead": row.bead_data.as_deref().map(serde_json::from_str::<Value>).transpose().ok().flatten(),
            "project": { "name": project.name, "path": project.root },
            "steps": step_results(tx.sql(), row).map_err(|e| e.to_string())?,
        });
        let text = prompt.render(&context).map_err(|e| format!("prompt: {e}"))?;
        if text.trim().is_empty() {
            return Err("prompt: empty after substitution".to_owned());
        }
        Ok((Some(text), timeout_ms(timeout), json!(config)))
    }
}

/// The outcome `{result, summary, data}` of each step before the current one, by step name: the last of its
/// attempts (spec §9.1).
pub(super) fn step_results(conn: &Connection, row: &Row) -> rusqlite::Result<Map<String, Value>> {
    let mut stmt = conn.prepare(
        "SELECT step, outcome FROM attempts a WHERE run = ?1 AND step != ?2 AND outcome IS NOT NULL \
         AND n = (SELECT MAX(n) FROM attempts WHERE run = a.run AND step = a.step)",
    )?;
    let rows = stmt.query_map([&row.id, &row.step], |r| {
        let outcome: String = r.get(1)?;
        Ok((r.get(0)?, serde_json::from_str(&outcome).unwrap_or_default()))
    })?;
    rows.collect()
}
