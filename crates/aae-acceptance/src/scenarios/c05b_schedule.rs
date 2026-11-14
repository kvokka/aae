//! Criterion 5b: a schedule fires on time, and the firings missed while the daemon was down fire exactly once when
//! it is back, the next one an interval later.
//!
//! The real daemon has no managed clock (contracts 9: no test mode), so this runs on the wall clock with an `every`
//! of a second. It checks the times the daemon wrote in its journal, never what the test saw at some moment: a loaded
//! host delays when the test looks, not what the daemon did. The same rules on the managed clock, `cron` included,
//! are core tests (`aae-testkit/tests/triggers.rs`).

use super::exec_formula;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_core::parse_rfc3339_ms;
use serde_json::Value;
use std::{sync::Arc, time::Duration};

/// Two seconds, not one: the catch-up at start must come before an interval, also on a loaded machine.
const EVERY: Duration = Duration::from_secs(2);
const EVERY_MS: i64 = 2000;
const WITHIN: Duration = Duration::from_secs(60);

/// Per `daemon.started` in the journal: its time and the times of the firings after it, in milliseconds. A firing
/// skipped because the last run is still on (`overlap`: one the kill interrupted is retried at start) counts: the
/// schedule did fire.
fn lives(seen: &[(String, Value)]) -> Vec<(i64, Vec<i64>)> {
    let mut lives: Vec<(i64, Vec<i64>)> = Vec::new();
    for (kind, event) in seen {
        let ts = || parse_rfc3339_ms(event["ts"].as_str().expect("ts")).expect("an RFC 3339 ts");
        match kind.as_str() {
            "daemon.started" => lives.push((ts(), Vec::new())),
            "trigger.fired" | "trigger.skipped" => lives.last_mut().expect("a firing after a start").1.push(ts()),
            _ => {}
        }
    }
    lives
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let mut d = Daemon::start(&env, |root| {
        let trigger = format!(
            "[[trigger]]\nname = \"tick\"\nevery = \"{}s\"\nrun = \"tick\"\nrig = \"web\"\n",
            EVERY.as_secs()
        );
        write(
            root.join("config/aae/shop/aae.toml"),
            format!("[rigs.web]\npath = \"web\"\n{trigger}"),
        );
        exec_formula(root, "tick", "", "\"true\"");
    })
    .await;
    // Two runs over, so the kill seldom interrupts one.
    d.journal_until(WITHIN, |seen| {
        seen.iter().filter(|(kind, _)| kind == "run.passed").count() >= 2
    })
    .await;
    // Down for three intervals and more: three firings missed.
    d.restart_after(EVERY * 3 + EVERY / 2).await;
    let seen = d
        .journal_until(WITHIN, |seen| {
            let lives = lives(seen);
            lives.len() == 2 && lives[1].1.len() >= 2
        })
        .await;
    let lives = lives(&seen);
    let [(up, before), (back, after)] = lives.as_slice() else {
        panic!("two daemon starts: {lives:?}")
    };

    // `every` first fires one interval after the daemon first sees the trigger, then an interval after each firing.
    assert!(
        before[0] >= up + EVERY_MS,
        "nothing before the first interval: {lives:?}"
    );
    let spaced = |f: &[i64]| f.windows(2).all(|w| w[1] - w[0] >= EVERY_MS);
    assert!(spaced(before), "once per interval: {lives:?}");
    let last = before.last().expect("a firing");
    assert!(back - last >= 3 * EVERY_MS, "down for three intervals: {lives:?}");
    // Set again from start, the first firing would be an interval after it, not before.
    assert!(
        after[0] - back < EVERY_MS,
        "the missed firings fire at start: {lives:?}"
    );
    assert!(
        spaced(after),
        "once, the next one an interval after the catch-up: {lives:?}"
    );
    format!(
        "every {} s; {} firings, 3 missed while down, 1 at start ({} ms after it), the next {} ms later",
        EVERY.as_secs(),
        before.len(),
        after[0] - back,
        after[1] - after[0]
    )
}
