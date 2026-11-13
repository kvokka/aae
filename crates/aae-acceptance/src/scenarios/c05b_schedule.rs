//! Criterion 5b: a schedule fires on time, and the firings missed while the daemon was down fire exactly once when
//! it is back, the next one an interval later.
//!
//! The real daemon has no managed clock (contracts 9: no test mode), so this runs on the wall clock with an `every`
//! of seconds; the same rules on the managed clock, `cron` included, are core tests (`aae-testkit/tests/triggers.rs`).

use super::exec_formula;
use crate::{
    Env,
    daemon::{Daemon, write},
};
use std::{sync::Arc, time::Duration};

const EVERY: Duration = Duration::from_secs(1);

/// The runs of the scheduled formula so far.
async fn fired(d: &Daemon) -> usize {
    let runs = d.get("/v1/runs?project=shop&limit=1000").await;
    let runs = runs.as_array().expect("a list of runs");
    runs.iter().filter(|r| r["formula"] == "shop.tick").count()
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
    // `every` first fires one interval after the daemon first sees the trigger.
    tokio::time::sleep(EVERY / 2).await;
    assert_eq!(fired(&d).await, 0, "nothing before the first interval");
    tokio::time::sleep(EVERY * 2).await;
    let before = fired(&d).await;
    assert_eq!(before, 2, "on time: once per interval");

    // Down for three intervals and more: three firings missed.
    d.restart_after(EVERY * 3 + EVERY / 2).await;
    tokio::time::sleep(EVERY / 2).await;
    assert_eq!(fired(&d).await, before + 1, "the missed firings fire once at start");
    tokio::time::sleep(EVERY).await;
    assert_eq!(
        fired(&d).await,
        before + 2,
        "the next one an interval after the catch-up"
    );
    format!(
        "every {} s; {before} firings, 3 missed while down, 1 at start, 1 after",
        EVERY.as_secs()
    )
}
