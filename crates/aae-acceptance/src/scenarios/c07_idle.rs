//! Criterion 7: the daemon idle with one registered project holds RSS at or under 50 MB and uses at most 1 % of
//! one core on average over 10 minutes. Linux only, as acceptance runs on the owner's Linux host (spec §15).

use super::MARK;
use crate::{Env, daemon::Daemon};
use std::{fs, process::Command, sync::Arc, time::Duration};

const WINDOW: Duration = Duration::from_secs(600);
const SAMPLE: Duration = Duration::from_secs(10);
const RSS_MAX_KB: u64 = 50 * 1000;
const CPU_MAX: f64 = 0.01;

/// VmRSS of `pid` in kB.
fn rss_kb(pid: u32) -> u64 {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).expect("the daemon lives");
    let line = status.lines().find(|l| l.starts_with("VmRSS:")).expect("VmRSS");
    line.split_whitespace()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .expect("a number of kB")
}

/// utime + stime of `pid` in clock ticks.
fn cpu_ticks(pid: u32) -> u64 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("the daemon lives");
    // The fields after the command name, which may hold spaces, start at the third.
    let fields: Vec<&str> = stat[stat.rfind(')').expect("(comm)") + 2..].split(' ').collect();
    fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let out = Command::new("getconf").arg("CLK_TCK").output().expect("getconf");
    let hz: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().expect("CLK_TCK");
    let d = Daemon::start(&env, |_| {}).await;
    let pid = d.pid();
    // Start-up work is not idle.
    tokio::time::sleep(SAMPLE).await;
    let ticks = cpu_ticks(pid);
    let mut rss = rss_kb(pid);
    let mut left = WINDOW;
    while !left.is_zero() {
        tokio::time::sleep(SAMPLE).await;
        left = left.saturating_sub(SAMPLE);
        rss = rss.max(rss_kb(pid));
    }
    let cpu = (cpu_ticks(pid) - ticks) as f64 / hz / WINDOW.as_secs_f64();
    let notes = format!(
        "RSS max {:.1} MB, CPU {:.3} % over {} min ({}, {}) \
         <!-- {MARK} id=idle-daemon date={} ver={} env={} src=aae-acceptance note=\"one project, no runs; \
         RSS sampled every {} s, CPU from /proc stat\" -->",
        rss as f64 / 1000.0,
        cpu * 100.0,
        WINDOW.as_secs() / 60,
        env.date,
        env.version,
        env.date,
        env.version.replace(' ', "-"),
        env.host.replace(' ', "-"),
        SAMPLE.as_secs(),
    );
    assert!(rss <= RSS_MAX_KB && cpu <= CPU_MAX, "over the limit: {notes}");
    notes
}
