//! Criterion 6: the hop from the commit of `run.passed` of run A to the launch of the harness of run B, started by a
//! subscription on that event; no bead, no worktree, no veto hooks. 30 hops: three chains of a prompt formula on the
//! real `claude` against a fake Anthropic API, each run passing by the finisher; a chain ends at the depth limit
//! (ten hops from a run started by hand).
//!
//! The end of a hop is `instance.ready` of B's session: the launch has returned with a process id. The trigger, the
//! start of B and its launch effect commit in the transaction of A's `run.passed`, so the hop is the effect loop
//! and the spawn; the time claude itself takes to start up is not part of it.

use super::{MARK, claude_agent};
use crate::{
    Env,
    daemon::{Daemon, write},
};
use aae_testkit::anthropic::{Provider, Reply};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

const CHAINS: usize = 3;
/// Runs in a chain: the one started by hand at depth 0 and ten by the trigger, the last at depth 10.
const RUNS: usize = 11;
const P50_MAX_MS: i64 = 1000;
const P90_MAX_MS: i64 = 3000;

const TRIGGER: &str = "[[trigger]]\nname = \"again\"\non = \"run.passed\"\nwhere = { \"run.formula\" = \"shop.hop\" }\n\
                       run = \"hop\"\nrig = \"web\"\n";

/// Milliseconds of the day of an RFC 3339 UTC time `…THH:MM:SS.mmmZ`.
fn day_ms(ts: &str) -> i64 {
    let time = ts.split('T').nth(1).expect("a time").trim_end_matches('Z');
    let mut parts = time.split(':');
    let mut field = || parts.next().expect("h:m:s").parse::<f64>().expect("a number");
    let (h, m, s) = (field(), field(), field());
    ((h * 3600.0 + m * 60.0 + s) * 1000.0).round() as i64
}

/// `b - a` in ms for two times less than a day apart.
fn between(a: &str, b: &str) -> i64 {
    (day_ms(b) - day_ms(a)).rem_euclid(86_400_000)
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = Reply::Bash(format!("{} finish pass --summary hop", env.aae.display()));
    let replies = (0..CHAINS * RUNS).flat_map(|_| [finish.clone(), Reply::Text("Done.".into())]);
    let provider = Provider::start(replies.collect()).await;
    let d = Daemon::start(&env, |root| {
        claude_agent(root, "hop", &provider.base_url());
        write(
            root.join("config/aae/shop/aae.toml"),
            format!("[rigs.web]\npath = \"web\"\n{TRIGGER}"),
        );
    })
    .await;
    let passed = |seen: &[(String, Value)]| seen.iter().filter(|(k, _)| k == "run.passed").count();
    let mut journal = Vec::new();
    // One chain at a time: the fake provider answers requests in order, so two sessions must not interleave.
    for chain in 1..=CHAINS {
        d.create("hop").await;
        journal = d
            .journal_until(Duration::from_secs(300), |seen| passed(seen) == chain * RUNS)
            .await;
    }

    let mut hops = Vec::new();
    for (_, fired) in journal.iter().filter(|(k, _)| k == "trigger.fired") {
        let cause = fired["data"]["cause_seq"].as_i64().expect("an event trigger");
        let (_, a) = journal.iter().find(|(_, e)| e["seq"] == cause).expect("the cause");
        assert_eq!(a["type"], "run.passed", "{a}");
        // One chain at a time: the first instance after the cause is B's.
        let after = |kind: &str, id: Option<&Value>| {
            let found = journal.iter().find(|(k, e)| {
                k == kind && e["seq"].as_i64() > Some(cause) && id.is_none_or(|id| e["data"]["instance"]["id"] == *id)
            });
            &found.unwrap_or_else(|| panic!("no {kind} after {cause}")).1
        };
        let starting = after("instance.starting", None);
        let ready = after("instance.ready", Some(&starting["data"]["instance"]["id"]));
        hops.push(between(a["ts"].as_str().unwrap(), ready["ts"].as_str().unwrap()));
    }
    assert_eq!(hops.len(), CHAINS * (RUNS - 1), "{hops:?}");
    hops.sort_unstable();
    let at = |q: usize| hops[(hops.len() * q).div_ceil(100) - 1];
    let (p50, p90, max) = (at(50), at(90), hops[hops.len() - 1]);
    let notes = format!(
        "{} hops: p50 {p50} ms, p90 {p90} ms, max {max} ms <!-- {MARK} id=hop date={} ver={} env={} \
         src=aae-acceptance note=\"run.passed ts of A to instance.ready ts of B; claude on a fake API\" -->",
        hops.len(),
        env.date,
        env.version.replace(' ', "-"),
        env.host.replace(' ', "-"),
    );
    assert!(p50 <= P50_MAX_MS && p90 <= P90_MAX_MS, "over the limit: {notes}");
    notes
}
