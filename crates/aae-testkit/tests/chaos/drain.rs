//! The checks of the drain steps of the stand (D1–D3).

use crate::steps::Play;
use rusqlite::OptionalExtension;

/// A drain step gives a bead one run per attempt of the step (D1); once it finished, no child on a bead is alive
/// (D2); its attempt, which has no process, outlives a crash of the daemon (D3).
pub(crate) fn drained(conn: &rusqlite::Connection, play: &Play) -> Result<(), String> {
    let twice: Option<(String, String, i64)> = conn
        .query_row(
            "SELECT r.parent_run, r.bead, COUNT(*) FROM runs r WHERE r.parent_run IS NOT NULL AND r.bead IS NOT NULL \
             GROUP BY r.parent_run, r.bead, (SELECT MAX(a.n) FROM attempts a WHERE a.run = r.parent_run \
             AND a.harness = 'drain' AND a.started_ms <= r.created_ms) HAVING COUNT(*) > 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .unwrap();
    if let Some((parent, bead, n)) = twice {
        return Err(format!(
            "{parent} gave {bead} {n} runs in one attempt of its drain step (D1)"
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
    let crashed: Option<String> = conn
        .query_row(
            "SELECT run FROM attempts WHERE harness = 'drain' \
             AND json_extract(failure, '$.kind') = 'daemon_crash'",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap();
    match crashed {
        Some(run) => Err(format!(
            "{run}: the attempt of its drain step failed with daemon_crash (D3)"
        )),
        None => Ok(()),
    }
}
