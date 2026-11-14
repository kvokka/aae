//! The prompt, the step timeout and the config snapshot of a run, rendered from the config of now.

use super::{RunHandler, row::Row};
use crate::config::Step;
use serde_json::{Value, json};

pub(super) const STEP_TIMEOUT_MS: i64 = 45 * 60_000;

impl RunHandler {
    /// The prompt, the step timeout and the config snapshot of a run, from the config of now: the effective config
    /// of a prompt step; `{"command", "env"}` of an exec step.
    pub(super) fn render(&self, row: &Row) -> Result<(Option<String>, i64, Value), String> {
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
        let (agent, prompt, timeout) = match (&formula.step, row.exec()) {
            (Step::Exec { command, timeout }, true) => {
                // The tracker by name, as in a session (spec §10.2).
                let env: serde_json::Map<String, Value> = (project.tracker.env.iter())
                    .map(|(name, value)| (name.clone(), json!({ "value": value })))
                    .collect();
                return Ok((None, timeout_ms(timeout), json!({ "command": command, "env": env })));
            }
            (Step::Prompt { agent, prompt, timeout }, false) => (agent, prompt, timeout),
            _ => return Err(format!("`{}` changed its step kind", row.formula)),
        };
        if agent.to_string() != row.agent {
            return Err(format!("`{}` now names agent `{agent}`", row.formula));
        }
        let config = self.config.effective(&row.project, Some(&row.rig), agent)?;
        let mut rig_view = json!({ "name": row.rig, "path": rig.path });
        for (key, value) in &rig.vars {
            rig_view[key] = json!(value);
        }
        let context = json!({
            "inputs": serde_json::from_str::<Value>(&row.inputs).unwrap_or_default(),
            "rig": rig_view, "run": row.view(),
            "bead": row.bead_data.as_deref().map(serde_json::from_str::<Value>).transpose().ok().flatten(),
            "project": { "name": project.name, "path": project.root },
        });
        let text = prompt.render(&context).map_err(|e| format!("prompt: {e}"))?;
        Ok((Some(text), timeout_ms(timeout), json!(config)))
    }
}
