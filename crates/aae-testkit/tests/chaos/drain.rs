//! The checks of the drain steps of the stand (D1–D3).

use crate::steps::Play;
use rusqlite::OptionalExtension;

/// A drain step gives a bead at most one run per entry into the step (D1); once it finished, no child on a bead is
/// alive (D2); the attempt of a drain step a crash of the daemon caught ends with `daemon_crash` (D3).
pub(crate) fn drained(conn: &rusqlite::Connection, play: &Play) -> Result<(), String> {
    let twice: Option<(String, String, i64)> = conn
        .query_row(
            // The formula `sweep` enters its drain step once: a bead has one run per parent at most.
            "SELECT parent_run, bead, COUNT(*) FROM runs WHERE parent_run IS NOT NULL AND bead IS NOT NULL \
             GROUP BY parent_run, bead HAVING COUNT(*) > 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .unwrap();
    if let Some((parent, bead, n)) = twice {
        return Err(format!(
            "{parent} gave {bead} {n} runs in one entry into its drain step (D1)"
        ));
    }
    for run in &play.drained {
        let alive: Option<String> = conn
            .query_row(
                "SELECT id FROM runs WHERE parent_run = ?1 AND bead IS NOT NULL \
                 AND state NOT IN ('passed', 'failed', 'cancelled')",
                [run],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        if let Some(kid) = alive {
            return Err(format!("{run} finished its drain step with {kid} alive (D2)"));
        }
    }
    for (run, n) in &play.caught {
        let kind: Option<String> = conn
            .query_row(
                "SELECT json_extract(failure, '$.kind') FROM attempts WHERE run = ?1 AND n = ?2",
                rusqlite::params![run, n],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
            .flatten();
        if kind.as_deref() != Some("daemon_crash") {
            return Err(format!(
                "{run}: attempt {n} of its drain step, caught by a crash, ended with {kind:?} (D3)"
            ));
        }
    }
    Ok(())
}
