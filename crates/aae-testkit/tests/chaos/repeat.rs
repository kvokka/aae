//! Repeats of a step and its check script (spec §9.1): the first two steps of `ship` run at most twice and have a
//! check script, which the stand plays instead of running. A kill lands between the record of the script and its word.

use crate::{
    Rng,
    check::{formula_steps, undone},
    common::World,
    steps::{KILLS, Play},
};
use aae_core::run::Exit;
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::sync::Mutex;
use tokio::task::AbortHandle;

/// The executions each step of the stand has: two for the first two steps of `ship`, one for the others.
fn max_attempts(formula: &str, step: &str) -> u32 {
    if formula.ends_with(".ship") && step != "3" {
        2
    } else {
        1
    }
}

/// How the check script of `payload` ends: by the seed, the run and the attempt alone, so a script cut by a kill and
/// run again says the same. Mostly 0, now and then 1, seldom 75. A script kills the daemon of `cut`, if armed, while it
/// runs: a check script is seldom under way when a step of the stand kills.
pub(crate) fn exit(seed: u64, payload: &Value, cut: &Mutex<Option<AbortHandle>>) -> Option<Exit> {
    if payload["check"] != true {
        return None;
    }
    let run = payload["run"].as_str().unwrap_or_default();
    let of = run
        .bytes()
        .fold(payload["attempt"].as_u64().unwrap_or_default(), |h, b| {
            h.wrapping_mul(0x100_0000_01B3) ^ u64::from(b)
        });
    let code = match Rng::new(seed ^ of).below(10) {
        0..=5 => 0,
        6..=8 => 1,
        _ => 75,
    };
    if let Some(writer) = cut.lock().unwrap().take() {
        writer.abort();
    }
    Some(Exit::Code(code))
}

/// Arms the cut of a check script with the daemon of `w`, for the step to come.
pub(crate) fn arm(w: &World, cut: &Mutex<Option<AbortHandle>>) {
    *cut.lock().unwrap() = Some(w.killer());
}

/// After the step: the daemon a check script cut starts again, and the cut is disarmed. Recovery runs the script
/// again, unarmed: its word is not lost twice.
pub(crate) async fn restart(w: &mut World, play: &mut Play, cut: &Mutex<Option<AbortHandle>>) {
    if cut.lock().unwrap().take().is_some() {
        return;
    }
    let undone = undone(w);
    for at in &undone {
        *KILLS.lock().unwrap().entry(at).or_default() += 1;
    }
    w.restart().await;
    play.log.push(format!("crash in a check script before {undone:?} done"));
}

/// A `run.step_finished` that did not pass is a repeat of the step it names, the step the run is at (S1): it is not a
/// finished step. A step repeats at most `max_attempts − 1` times, the k-th repeat from its k-th execution (S5); the
/// stand has no `back_to`, so a step has one round. Returns whether the event was a repeat.
pub(crate) fn repeated(run: &str, kind: &str, data: &Value, play: &mut Play) -> Result<bool, String> {
    let view = &data["run"];
    let (formula, step) = (
        view["formula"].as_str().unwrap_or_default(),
        view["step"].as_str().unwrap_or_default(),
    );
    let key = (run.to_owned(), step.to_owned());
    let execution = view["step_attempt"].as_u64().unwrap_or_default();
    let repeats = play.repeats.get(&key).copied().unwrap_or_default();
    if kind == "run.step_finished" && u64::from(repeats) + 1 != execution {
        return Err(format!(
            "{run}: the step {step} finished its execution {execution} after {repeats} repeats (S5)"
        ));
    }
    if kind != "run.step_finished" || data["outcome"]["result"] != "fail" {
        return Ok(false);
    }
    let done = play.steps.get(run).map_or(0, Vec::len);
    if formula_steps(formula).get(done) != Some(&step) {
        return Err(format!(
            "{run}: the step {step} repeated after {done} steps finished (S1)"
        ));
    }
    if repeats + 1 >= max_attempts(formula, step) {
        return Err(format!("{run}: the step {step} repeated {} times (S5)", repeats + 1));
    }
    play.repeats.insert(key, repeats + 1);
    Ok(true)
}

/// No run is past the executions of its step, nor behind the repeats the journal says its step had: the count survives
/// a crash. A check script that said 75 ended its run: the run failed and its step never ran again (S5).
pub(crate) fn counted(conn: &rusqlite::Connection, play: &Play) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT id, formula, step, step_attempt FROM runs")
        .unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)));
    for (run, formula, step, at) in rows.unwrap().map(Result::unwrap) {
        let (run, formula, step, at): (String, String, String, u32) = (run, formula, step, at);
        // The journal comes after the state: it may say fewer repeats, never more.
        let repeats = play
            .repeats
            .get(&(run.clone(), step.clone()))
            .copied()
            .unwrap_or_default();
        if at > max_attempts(&formula, &step) || at <= repeats {
            return Err(format!(
                "{run} is at the execution {at} of the step {step}, {repeats} repeats (S5)"
            ));
        }
    }
    let fatal: Option<(String, String)> = conn
        .query_row(
            "SELECT a.run, r.state FROM attempts a JOIN runs r ON r.id = a.run \
             WHERE json_extract(a.outcome, '$.summary') = 'check script: exit 75' AND (r.state != 'failed' \
             OR EXISTS (SELECT 1 FROM attempts b WHERE b.run = a.run AND b.n > a.n))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .unwrap();
    match fatal {
        Some((run, state)) => Err(format!("{run}: its check script said 75, it is {state} or ran on (S5)")),
        None => Ok(()),
    }
}
