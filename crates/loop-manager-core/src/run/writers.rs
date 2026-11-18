//! A rig's `max_writers` (docs/design/config.md "Project", contracts 6.1): the runs that work in the rig's own
//! directory at once. A run counts by its formula, not by the step it is at: it holds its place from `preparing`
//! (parked and interrupted too) to its terminal state.

use super::{RunHandler, row::Row};
use crate::{
    config::{Project, Scope, StepKind},
    writer::{Error, Tx},
};
use rusqlite::params;

impl RunHandler {
    /// Whether a run of `formula` on `rig` works in the rig's own directory: its formula has no worktree, and an exec
    /// step, or an ai step of an agent that is not of scope root, is among its steps. A formula or agent that does
    /// not resolve counts.
    fn writes(&self, project: &Project, rig: &str, formula: &str, as_agent: Option<&str>) -> bool {
        let Ok((_, f)) = project.formula(formula) else {
            return true;
        };
        !f.worktree
            && f.steps.iter().any(|step| match step.kind {
                StepKind::Exec { .. } => true,
                StepKind::Ai { .. } => (self
                    .config
                    .for_run_as(&project.name, rig, formula, &step.name, as_agent))
                .ok()
                .is_none_or(|(_, config)| config.scope != Scope::Root),
                StepKind::Drain { .. } | StepKind::Formula { .. } | StepKind::Await { .. } => false,
            })
    }

    /// Whether the queued `row` works in its rig's directory and the rig already has `max_writers` runs that do.
    /// A run in a drain or formula step lends its place to the runs that step started, and to theirs down the chain.
    pub(super) fn rig_full(&self, tx: &Tx<'_>, row: &Row) -> Result<bool, Error> {
        let Some(project) = self.config.projects.iter().find(|p| p.name == row.project) else {
            return Ok(false);
        };
        let Some(limit) = project.rigs.get(&row.rig).and_then(|r| r.max_writers) else {
            return Ok(false);
        };
        if !self.writes(project, &row.rig, &row.formula, row.as_agent.as_deref()) {
            return Ok(false);
        }
        let mut stmt = tx.sql().prepare(
            "WITH RECURSIVE lender(id, parent) AS ( \
                 SELECT p.id, p.parent_run FROM runs c JOIN runs p ON p.id = c.parent_run \
                 WHERE c.id = ?3 AND p.step_kind IN ('drain', 'formula') \
                 UNION ALL \
                 SELECT p.id, p.parent_run FROM lender JOIN runs p ON p.id = lender.parent \
                 WHERE p.step_kind IN ('drain', 'formula')) \
             SELECT formula, as_agent FROM runs \
             WHERE project = ?1 AND rig = ?2 AND state NOT IN ('queued', 'passed', 'failed', 'cancelled') \
               AND id NOT IN (SELECT id FROM lender)",
        )?;
        let live = stmt.query_map(params![row.project, row.rig, row.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut writers = 0;
        for run in live {
            let (formula, as_agent) = run?;
            writers += u32::from(self.writes(project, &row.rig, &formula, as_agent.as_deref()));
        }
        Ok(writers >= limit)
    }
}
