//! `default-policy` (spec §11.5): the reactions to failures as decisions of docs/design/contracts.md 10. Every
//! count comes from the event (`attempts`, `gates`, `keys`), so the policy keeps no state and the same event always
//! gets the same decision: a hook run again after a daemon crash decides the same.

use aae_core::{parse_rfc3339_ms, rfc3339_ms};
use serde::Deserialize;
use serde_json::{Value, json};

/// The thresholds, in the `[builtin.default-policy]` table of the orchestrator config. Their sources are in
/// docs/design/contracts.md 10.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Params {
    /// Added to the reset time of a quota before the account gate opens.
    quota_margin_s: i64,
    /// How long the account gate stays closed on a quota with no reset time.
    quota_wait_s: i64,
    /// How long the model gate stays closed after a harness gave up its own retries.
    model_pause_s: i64,
    /// The backoff of a retry the harness does not make itself: `min(base · 2^(n−1), cap)`.
    backoff_base_s: i64,
    backoff_cap_s: i64,
    /// From this many consecutive rate limits on, the model gate closes instead of a backoff.
    backoff_park_from: usize,
    /// The wait on `auth_required` and `billing` (postmortem 0001, spec §2.1 item 12): the account gate closes for
    /// `min(base · 2^(n−1), cap)` after `n` consecutive such failures; the attempt after it probes the cause. No
    /// escalation to an operator, and the core does not count these attempts against its ceiling.
    auth_backoff_base_s: i64,
    auth_backoff_cap_s: i64,
    /// An attempt shorter than this that crashed is a rapid crash.
    rapid_s: i64,
    /// This many consecutive rapid crashes close the agent gate for `crash_pause_s`.
    crash_loop: usize,
    crash_pause_s: i64,
    /// This many consecutive failed resumes start a new session instead.
    resume_failures: usize,
    /// The pause before a retry of an unknown failure, and how many of them in a row.
    unknown_after_s: i64,
    unknown_retries: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            quota_margin_s: 60,
            quota_wait_s: 30 * 60,
            model_pause_s: 10 * 60,
            backoff_base_s: 10,
            backoff_cap_s: 5 * 60,
            backoff_park_from: 3,
            auth_backoff_base_s: 30,
            auth_backoff_cap_s: 60 * 60,
            rapid_s: 30,
            crash_loop: 5,
            crash_pause_s: 5 * 60,
            resume_failures: 3,
            unknown_after_s: 30,
            unknown_retries: 3,
        }
    }
}

/// The decision on `run.attempt_failed` or `run.turn_ended_without_outcome`; `None` for any other event.
pub(crate) fn decide(event: &Value, p: &Params) -> Option<Value> {
    let data = &event["data"];
    match event["type"].as_str()? {
        "run.turn_ended_without_outcome" if data["remarks"].as_i64() == Some(0) => Some(json!({
            "decision": "continue",
            "message": "Your turn ended without an outcome. Finish the task, then report the outcome with the \
                        finisher as your instructions describe.",
        })),
        "run.turn_ended_without_outcome" => Some(fail("no outcome after a reminder")),
        "run.attempt_failed" => Some(attempt_failed(event, p)),
        _ => None,
    }
}

fn attempt_failed(event: &Value, p: &Params) -> Value {
    let data = &event["data"];
    let now = event["ts"].as_str().and_then(parse_rfc3339_ms).unwrap_or_default();
    let attempts = data["attempts"].as_array().map_or(&[][..], Vec::as_slice);
    let time = |v: &Value| v.as_str().and_then(parse_rfc3339_ms);
    // The attempts that failed with one of `kinds`, newest first and without a break.
    let streak = |kinds: &[&str]| {
        attempts
            .iter()
            .rev()
            .take_while(|a| a["failure_kind"].as_str().is_some_and(|k| kinds.contains(&k)))
            .count()
    };
    let key = |prefix: &str| {
        let keys = data["keys"].as_array().into_iter().flatten();
        keys.filter_map(Value::as_str)
            .find(|k| k.starts_with(prefix))
            .map(str::to_owned)
    };
    let kind = data["failure"]["kind"].as_str().unwrap_or("unknown");
    // A gate of the run that closed after this attempt started: the failure is one the gate already answers.
    let started = attempts.last().and_then(|a| time(&a["started_at"]));
    let absorbed = data["gates"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|g| started.is_some_and(|s| time(&g["closed_at"]).is_some_and(|c| c >= s)));
    if let Some(gate) = absorbed {
        let gate = json!({ "key": gate["key"], "until": gate["until"], "reason": gate["reason"] });
        return json!({ "decision": "park", "gate": gate, "then": "resume" });
    }
    let park = |key: Option<String>, until: Option<i64>| match key {
        Some(key) => {
            let gate = json!({ "key": key, "until": until.map(rfc3339_ms), "reason": kind });
            json!({ "decision": "park", "gate": gate, "then": "resume" })
        }
        // A park needs a key of the run; without one the retry just waits as long.
        None => resume(until.map_or(0, |u| (u - now) / 1000)),
    };
    match kind {
        "quota_exhausted" => {
            let reset = time(&data["failure"]["retry_at"]).map(|t| t + p.quota_margin_s * 1000);
            park(key("account:"), Some(reset.unwrap_or(now + p.quota_wait_s * 1000)))
        }
        "rate_limited" | "overloaded" => {
            let n = streak(&["rate_limited", "overloaded"]);
            if data["failure"]["retried_by_harness"] == true || n >= p.backoff_park_from {
                park(key("model:"), Some(now + p.model_pause_s * 1000))
            } else {
                resume(backoff(p.backoff_base_s, p.backoff_cap_s, n))
            }
        }
        // An auth failure may be a passing proxy or WAF error (postmortem 0001): a growing wait, re-armed on every
        // failure. A run without an account key cannot park: it fails.
        "auth_required" | "billing" => match key("account:") {
            Some(account) => {
                let n = streak(&[kind]);
                park(
                    Some(account),
                    Some(now + backoff(p.auth_backoff_base_s, p.auth_backoff_cap_s, n) * 1000),
                )
            }
            None => fail(kind),
        },
        "context_overflow" | "transcript_divergence" => json!({ "decision": "new_session" }),
        "config_error" => fail_closing(kind, key("agent:")),
        "workspace_failure" => fail_closing(kind, key("rig:")),
        "harness_crash" | "adapter_crash" => {
            let crashes = attempts.iter().rev().take_while(|a| {
                let crashed = matches!(a["failure_kind"].as_str(), Some("harness_crash" | "adapter_crash"));
                let took = time(&a["ended_at"]).zip(time(&a["started_at"])).map(|(e, s)| e - s);
                crashed && took.is_some_and(|t| t < p.rapid_s * 1000)
            });
            let failed_resumes = attempts.iter().rev().take_while(|a| a["kind"] == "resume").count();
            if crashes.count() >= p.crash_loop {
                park(key("agent:"), Some(now + p.crash_pause_s * 1000))
            } else if failed_resumes >= p.resume_failures {
                json!({ "decision": "new_session" })
            } else {
                resume(0)
            }
        }
        "daemon_crash" | "tool_failure" => resume(0),
        "stall" => match streak(&["stall"]) {
            ..=1 => resume(0),
            2 => json!({ "decision": "new_session" }),
            _ => fail("stall"),
        },
        "runaway_turn" => match streak(&["runaway_turn"]) {
            ..=1 => json!({ "decision": "new_session" }),
            _ => fail("runaway_turn"),
        },
        _ if streak(&[kind]) <= p.unknown_retries => resume(p.unknown_after_s),
        _ => fail(kind),
    }
}

/// `min(base · 2^(n−1), cap)` seconds.
fn backoff(base: i64, cap: i64, n: usize) -> i64 {
    let exp = u32::try_from(n.saturating_sub(1)).unwrap_or(u32::MAX).min(20);
    base.saturating_mul(1 << exp).min(cap)
}

fn resume(after_s: i64) -> Value {
    json!({ "decision": "resume", "after": format!("{}s", after_s.max(0)) })
}

fn fail(reason: &str) -> Value {
    json!({ "decision": "fail", "reason": reason })
}

/// `fail` that closes `key` until an operator opens it.
fn fail_closing(reason: &str, key: Option<String>) -> Value {
    let gate = key.map(|key| json!({ "key": key, "until": null, "reason": reason }));
    json!({ "decision": "fail", "reason": reason, "gate": gate })
}
