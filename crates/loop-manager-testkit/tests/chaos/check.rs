//! The invariants checked on the state and the journal after every step.

use crate::{TERMINAL, common::World, steps::Play};
use loop_manager_core::run::MAX_ATTEMPTS;
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::{
    collections::BTreeSet,
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
    for event in new_events(w, play)? {
        check_event(&event, play)?;
    }
    let conn = state(w);
    let all = runs(w);
    // The children of drain steps come from no create of the test.
    let created: u32 = conn
        .query_row("SELECT COUNT(*) FROM runs WHERE parent_run IS NULL", [], |r| r.get(0))
        .unwrap();
    if usize::try_from(created) != Ok(play.runs.len()) {
        return Err(format!("{created} runs for {} creates (invariant 10)", play.runs.len()));
    }
    for (run, state, attempt, waiting) in &all {
        // Attempts that parked on an account gate are outside the ceiling (decisions 49, 50).
        let counted: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE run = ?1 AND waited = 0 AND failure IS NOT NULL",
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
            return Err(format!("{run}: {open} attempts not ended (invariant 2)"));
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
                    // A turn that ended before a restart waits for its decision with no session: recovery let go of
                    // the handle, and a decision that needs the session fails the attempt.
                    "SELECT session FROM attempts WHERE run = ?1 AND ended_ms IS NULL AND handle IS NOT NULL",
                    [run],
                    |r| r.get(0),
                )
                .optional()
                .unwrap()
                .flatten();
            let session: Option<serde_json::Value> = session.and_then(|s| serde_json::from_str(&s).ok());
            if let Some(session) = session.filter(|s| !w.driver.has_transcript(s)) {
                return Err(format!("{run} runs on {session} with no transcript (invariant 17)"));
            }
        }
    }
    stepped(&conn, play)?;
    crate::repeat::counted(&conn, play)?;
    crate::drain::drained(&conn, play)?;
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

fn check_event(e: &Value, play: &mut Play) -> Result<(), String> {
    let seq = e["seq"].as_i64().unwrap();
    let last = play.seq;
    if seq <= last {
        return Err(format!("journal seq {seq} after {last}: an event delivered twice"));
    }
    play.seq = seq;
    let (Some(run), Some(kind)) = (e["run"].as_str(), e["type"].as_str()) else {
        return Ok(());
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
    if !crate::repeat::repeated(run, kind, &e["data"], play)? {
        step_finished(run, kind, &e["data"]["run"], play)?;
    }
    Ok(())
}

/// The steps of each formula of the stand (`shop.ship` and the like), in order: `ship` has three, named by position,
/// `sweep` two, the first of them a drain step.
pub(crate) fn formula_steps(formula: &str) -> &'static [&'static str] {
    if formula.ends_with(".ship") {
        &["1", "2", "3"]
    } else if formula.ends_with(".sweep") {
        &["1", "2"]
    } else {
        &["1"]
    }
}

/// Each step but the last finishes once, a repeat aside, in the order of the formula (S1); a run passes once every step before the
/// last finished and from the last one (S4).
fn step_finished(run: &str, kind: &str, view: &Value, play: &mut Play) -> Result<(), String> {
    let steps = formula_steps(view["formula"].as_str().unwrap_or_default());
    let step = view["step"].as_str().unwrap_or_default();
    let done = play.steps.entry(run.to_owned()).or_default();
    if kind == "run.step_finished" {
        if done.len() + 1 >= steps.len() || steps[done.len()] != step {
            return Err(format!("{run}: the step {step} finished after {done:?} (S1)"));
        }
        done.push(step.to_owned());
        if view["formula"].as_str().is_some_and(|f| f.ends_with(".sweep")) && step == "1" {
            play.drained.insert(run.to_owned());
        }
    }
    if kind == "run.passed" && (done.len() + 1 != steps.len() || steps.last() != Some(&step)) {
        return Err(format!("{run} passed at the step {step} after {done:?} finished (S4)"));
    }
    Ok(())
}

/// `(run, formula, step of the run, step of an attempt, whether the attempt said pass)`.
type StepRow = (String, String, String, Option<String>, Option<bool>);

/// The attempts of a run go through the steps of its formula in order; the run is at the step of its last attempt or,
/// once that attempt said pass, at the next one, and the journal says every step before it finished (S2). No session
/// serves two steps (S3, docs/design/contracts.md 6).
fn stepped(conn: &rusqlite::Connection, play: &Play) -> Result<(), String> {
    let rows: Vec<StepRow> = conn
        .prepare(
            "SELECT r.id, r.formula, r.step, a.step, json_extract(a.outcome, '$.result') = 'pass' \
             FROM runs r LEFT JOIN attempts a ON a.run = r.id ORDER BY r.n, a.n",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for of in rows.chunk_by(|a, b| a.0 == b.0) {
        let (run, formula, at) = (&of[0].0, &of[0].1, &of[0].2);
        let steps = formula_steps(formula);
        let pos = |step: &str| steps.iter().position(|s| *s == step);
        let mut last: Option<(usize, bool)> = None;
        for (.., step, pass) in of {
            let Some(step) = step else { continue };
            let i = pos(step).ok_or_else(|| format!("{run}: an attempt at the step {step} of `{formula}` (S2)"))?;
            if last.is_some_and(|(l, _)| i < l) {
                return Err(format!("{run}: an attempt at the step {step} after a later one (S2)"));
            }
            last = Some((i, *pass == Some(true)));
        }
        let i = pos(at).ok_or_else(|| format!("{run} is at the step {at} of `{formula}` (S2)"))?;
        let held = last.map_or(i == 0, |(l, pass)| i == l || pass && i == l + 1);
        let finished = play.steps.get(run).map_or(0, Vec::len);
        if !held || i != finished {
            return Err(format!(
                "{run} is at the step {at}, its last attempt at {last:?}, {finished} steps finished (S2)"
            ));
        }
    }
    let shared: Option<String> = conn
        .query_row(
            "SELECT session FROM attempts WHERE session IS NOT NULL GROUP BY session \
             HAVING COUNT(DISTINCT run || ':' || step) > 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap();
    match shared {
        Some(session) => Err(format!("the session {session} serves two steps (S3)")),
        None => Ok(()),
    }
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
/// while it runs (invariant 15).
fn watched(conn: &rusqlite::Connection, run: &str, state: &str, waiting: bool) -> Result<(), String> {
    let draining: bool = conn
        .query_row("SELECT step_kind = 'drain' FROM runs WHERE id = ?1", [run], |r| {
            r.get(0)
        })
        .unwrap();
    let what = match (state, waiting) {
        ("starting", _) => "state",
        // A drain step has no session: the step's timeout is its deadline.
        ("running", false) if draining => "state",
        ("running", false) => "silence",
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

/// What the state recorded and the daemon had not done yet: tracker calls, harness calls, hooks, journal lines.
pub(crate) fn undone(w: &World) -> Vec<&'static str> {
    let conn = state(w);
    let ops: Vec<String> = conn
        .prepare("SELECT DISTINCT json_extract(payload, '$.op') FROM effects")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let behind: bool = conn
        .query_row(
            "SELECT COALESCE((SELECT MAX(seq) FROM outbox), 0) > (SELECT seq FROM consumers WHERE name = 'journal')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut undone: BTreeSet<_> = ops
        .iter()
        .map(|op| match op.as_str() {
            "tracker" => "tracker",
            "hooks" => "hooks",
            "launch" | "prompt" | "stop" | "stop_instance" => "harness",
            _ => "other",
        })
        .collect();
    if behind {
        undone.insert("journal");
    }
    if past_first(&conn) {
        undone.insert("step");
    }
    let draining: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM runs WHERE step_kind = 'drain' AND state = 'running')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if draining {
        undone.insert("drain");
    }
    // A run in `finishing` that waits for no decision waits for the word of its check script.
    let checking: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM runs WHERE state = 'finishing' AND waiting IS NULL)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if checking {
        undone.insert("check");
    }
    undone.into_iter().collect()
}

/// Whether a live run is past the first step of its formula.
pub(crate) fn past_first(conn: &rusqlite::Connection) -> bool {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE step != '1' AND state NOT IN ('passed', 'failed', 'cancelled'))",
        [],
        |r| r.get(0),
    )
    .unwrap()
}
