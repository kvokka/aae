//! The invariants checked on the state and the journal after every step.

use crate::{TERMINAL, common::World, steps::Play};
use aae_core::run::MAX_ATTEMPTS;
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

pub(crate) fn state(w: &World) -> rusqlite::Connection {
    rusqlite::Connection::open(w.dir.path().join("state.db")).unwrap()
}

/// `(id, state, attempt, waiting)` of every run.
pub(crate) fn runs(w: &World) -> Vec<(String, String, u32, Option<i64>)> {
    let conn = state(w);
    let mut stmt = conn
        .prepare("SELECT id, state, attempt, waiting FROM runs ORDER BY n")
        .unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)));
    rows.unwrap().map(Result::unwrap).collect()
}

/// The journal lines written since the last check. A seed writes far less than the 10 MB that rotates the journal,
/// so the current file only grows and each check reads only its new whole lines.
fn new_events(w: &World, play: &mut Play) -> Result<Vec<Value>, String> {
    let Ok(mut file) = File::open(w.journal.join("events.jsonl")) else {
        return Ok(Vec::new());
    };
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len < play.read {
        return Err(format!("the journal shrank from {} to {len} bytes", play.read));
    }
    let mut text = String::new();
    file.seek(SeekFrom::Start(play.read))
        .and_then(|_| file.read_to_string(&mut text))
        .map_err(|e| e.to_string())?;
    let whole = text.rfind('\n').map_or(0, |i| i + 1);
    play.read += whole as u64;
    text[..whole]
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|e| format!("a journal line: {e}")))
        .collect()
}

/// The invariants that hold after every step; the journal is checked as it grows.
pub(crate) fn check(w: &World, play: &mut Play) -> Result<(), String> {
    if let Some(broken) = &play.broken {
        return Err(broken.clone());
    }
    for e in &new_events(w, play)? {
        let seq = e["seq"].as_i64().unwrap();
        let last = play.seq;
        if seq <= last {
            return Err(format!("journal seq {seq} after {last}: an event delivered twice"));
        }
        play.seq = seq;
        let (Some(run), Some(kind)) = (e["run"].as_str(), e["type"].as_str()) else {
            continue;
        };
        if let Some(end) = play.ended.get(run).filter(|_| kind.starts_with("run.")) {
            return Err(format!("{run}: {kind} at {seq} after {end} (an end is final)"));
        }
        if let Some(end) = kind.strip_prefix("run.").filter(|k| TERMINAL.contains(k)) {
            play.ended.insert(run.to_owned(), end.to_owned());
            if end == "passed" {
                if e["data"]["outcome"]["result"] != "pass" {
                    return Err(format!("{run} passed without a pass outcome: {}", e["data"]));
                }
                if !play.finished.contains(run) {
                    return Err(format!("{run} passed with no accepted pass finisher (invariant 12)"));
                }
            }
            if end == "failed" {
                failed(run, &e["data"], play)?;
            }
        }
    }
    let conn = state(w);
    let all = runs(w);
    if all.len() != play.runs.len() {
        return Err(format!(
            "{} runs for {} creates (invariant 10)",
            all.len(),
            play.runs.len()
        ));
    }
    for (run, state, attempt, waiting) in &all {
        // Attempts that parked on an account gate are outside the ceiling (spec §2.1 item 12).
        let counted: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE run = ?1 AND waited = 0",
                [run],
                |r| r.get(0),
            )
            .unwrap();
        if counted > MAX_ATTEMPTS {
            return Err(format!(
                "{run}: {counted} attempts of {attempt} over the ceiling (invariant 16)"
            ));
        }
        let open: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE run = ?1 AND ended_ms IS NULL",
                [run],
                |r| r.get(0),
            )
            .unwrap();
        if TERMINAL.contains(&state.as_str()) {
            if play.ended.get(run) != Some(state) {
                return Err(format!(
                    "{run} is {state} in the state, {:?} in the journal",
                    play.ended.get(run)
                ));
            }
            if open != 0 {
                return Err(format!("{run} is {state} with an attempt not ended"));
            }
            continue;
        }
        if open > 1 {
            return Err(format!("{run}: {open} attempts not ended (invariant 6)"));
        }
        let deadlines: i64 = conn
            .query_row("SELECT COUNT(*) FROM deadlines WHERE id LIKE ?1 || ':%'", [run], |r| {
                r.get(0)
            })
            .unwrap();
        if deadlines == 0 {
            return Err(format!("{run} is {state} with no deadline (invariant 14)"));
        }
        watched(&conn, run, state, waiting.is_some())?;
        if state == "running" {
            let session: Option<String> = conn
                .query_row(
                    "SELECT session FROM attempts WHERE run = ?1 AND ended_ms IS NULL",
                    [run],
                    |r| r.get(0),
                )
                .optional()
                .unwrap()
                .flatten();
            let session = session.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            if !w.driver.has_transcript(&session) {
                return Err(format!("{run} runs on {session} with no transcript (invariant 17)"));
            }
        }
    }
    // One live owner per bead, and the owner the state records is a live run on that bead (invariants 2, 3).
    let owners: Vec<(String, Option<String>, i64)> = conn
        .prepare(
            "SELECT b.id, b.owner, (SELECT COUNT(*) FROM runs r WHERE r.bead = b.id \
             AND r.state NOT IN ('passed', 'failed', 'cancelled')) FROM beads b",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (bead, owner, live) in owners {
        let held = owner.as_ref().and_then(|o| all.iter().find(|r| &r.0 == o));
        let bad = live > 1 || owner.is_some() && held.is_none_or(|r| TERMINAL.contains(&r.1.as_str()));
        if bad {
            return Err(format!("{bead}: owner {owner:?}, {live} live runs on it"));
        }
    }
    Ok(())
}

/// A failure says whose it is: the agent's only when its last accepted finisher said fail, the infrastructure's when
/// the attempts ran out (invariant 13).
fn failed(run: &str, data: &Value, play: &Play) -> Result<(), String> {
    let class = data["class"].as_str().unwrap_or_default();
    let said = play.said.get(run);
    if !["infrastructure", "agent", "check", "hollow_done"].contains(&class) {
        return Err(format!("{run} failed with the class {class:?} (invariant 13)"));
    }
    if class == "agent" && said != Some(&false) {
        return Err(format!(
            "{run} failed as agent, its last finisher said pass: {said:?} (invariant 13)"
        ));
    }
    if data["reason"] == "attempts_ceiling" && class != "infrastructure" {
        return Err(format!("{run} ran out of attempts as {class} (invariant 13)"));
    }
    Ok(())
}

/// A started run that waits for nobody has a watchdog: a first-event deadline while it starts, a silence deadline
/// while it runs, a human deadline while it asks (invariant 15).
fn watched(conn: &rusqlite::Connection, run: &str, state: &str, waiting: bool) -> Result<(), String> {
    let what = match (state, waiting) {
        ("starting", _) => "state",
        ("running", false) => "silence",
        ("requires_action", _) => "human",
        _ => return Ok(()),
    };
    let id = format!("{run}:{what}");
    let set: bool = conn
        .query_row("SELECT EXISTS (SELECT 1 FROM deadlines WHERE id = ?1)", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    if set {
        Ok(())
    } else {
        Err(format!("{run} is {state} without the deadline {id} (invariant 15)"))
    }
}
