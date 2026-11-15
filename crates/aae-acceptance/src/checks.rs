//! The rows of spec §15 that live in cargo builds, cargo tests and the CI scripts (spec §2, decision 44): each runs its
//! commands in the workspace, with the target directory of the spec, and passes when all of them succeed. They run
//! one after another once the scenarios are done (see `main`).

use crate::{Env, Launch, Scenario};
use std::{env::consts, path::PathBuf, sync::Arc};

/// The four release targets of spec §13.
const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

pub(crate) const ALL: &[Scenario] = &[
    Scenario {
        criterion: "1",
        title: "one binary builds for the four targets; the build for this host runs without extra runtimes",
        launch: Launch::Release,
        run: |env| Box::pin(release(env)),
    },
    Scenario {
        criterion: "9",
        title: "kill -9 of claude and of the codex app-server mid-tool is typed, resumed and reaches its outcome",
        launch: Launch::Ci,
        run: |env| Box::pin(tests(env, "-p aae -E test(/killed_in_a_command_is_resumed/)", 2)),
    },
    Scenario {
        criterion: "16",
        title: "the claim holds through the death of a claude session and its resume; the session has the tracker's path",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p aae -E test(a_claude_killed_in_a_command_is_resumed_and_its_run_passes)",
                1,
            ))
        },
    },
    Scenario {
        criterion: "5b",
        title: "a cron and an hourly schedule fire on the managed clock, and once after downtime",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p aae-testkit -E test(a_schedule_fires_on_the_managed_clock_and_once_after_downtime)",
                1,
            ))
        },
    },
    Scenario {
        criterion: "11",
        title: "the contract tests of claude and codex (§7.8), codex's model list, and of the br tracker (§10.2) pass",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p aae-claude -p aae-codex -p aae-br -E binary(contract)|binary(models)",
                38,
            ))
        },
    },
    Scenario {
        criterion: "17",
        title: "a CLI command per API operation, the committed OpenAPI matches the code and real replies fit it, \
            MCP serves its five tools",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p aae-front -p aae -E binary(openapi)|binary(replies)|test(both_epochs_serve_the_same_five_tools)\
                 |test(a_codex_thread_that_calls_the_finisher_passes_its_run)",
                7,
            ))
        },
    },
    Scenario {
        criterion: "20",
        title: "the pack and the project of each demo in demos/ load",
        launch: Launch::Ci,
        run: |env| Box::pin(demos_load(env)),
    },
    Scenario {
        criterion: "20",
        title: "demo 1: a request, a planner that files beads, a drain to coders on claude and codex, a report that \
            every bead under the root is closed, unattended",
        launch: Launch::Request,
        run: |env| Box::pin(demo(env, "1-planner-executor")),
    },
    Scenario {
        criterion: "22",
        title: "line budgets, MEASURE markers, a current readiness file whose scores the rows allow, all 8 or more",
        launch: Launch::Ci,
        run: |env| Box::pin(budget(env)),
    },
    Scenario {
        criterion: "23",
        title: "no todo!/unimplemented!, no unused public items or dependencies (the rest is review)",
        launch: Launch::Ci,
        run: |env| Box::pin(no_stubs(env)),
    },
];

/// The workspace this binary was built from.
pub(crate) fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// Runs `program args` in the workspace; its stdout and stderr, or a panic with their last lines.
async fn command(env: &Env, program: &str, args: &[&str]) -> String {
    let out = tokio::process::Command::new(program)
        .args(args)
        .current_dir(root())
        .env("CARGO_TARGET_DIR", &env.target)
        .output()
        .await
        .unwrap_or_else(|e| panic!("{program}: {e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        out.status.success(),
        "`{program} {}` failed: {}",
        args.join(" "),
        lines[lines.len().saturating_sub(12)..].join(" / ")
    );
    text
}

/// The first line of `text` holding `word`, trimmed.
fn line_with(text: &str, word: &str) -> String {
    text.lines()
        .find(|l| l.contains(word))
        .unwrap_or_default()
        .trim()
        .to_owned()
}

async fn release(env: Arc<Env>) -> String {
    for target in TARGETS {
        let args = [
            "zigbuild",
            "-q",
            "--release",
            "--locked",
            "-p",
            "aae",
            "--target",
            target,
        ];
        command(&env, "cargo", &args).await;
    }
    let os = if consts::OS == "macos" {
        "apple-darwin"
    } else {
        "unknown-linux-musl"
    };
    let host = format!("{}-{os}", consts::ARCH);
    let binary = env.target.join(&host).join("release/aae");
    let version = command(&env, &binary.to_string_lossy(), &["--version"]).await;
    format!(
        "built for {}; the {host} build runs: {}",
        TARGETS.join(", "),
        version.trim()
    )
}

/// The nextest summary of `filter`, which must run `want` tests: a renamed test would leave its row passing.
async fn tests(env: Arc<Env>, filter: &str, want: u32) -> String {
    let mut args = vec!["nextest", "run", "--locked", "--no-fail-fast"];
    args.extend(filter.split(' '));
    let summary = line_with(&command(&env, "cargo", &args).await, "Summary");
    assert!(
        summary.contains(&format!(" {want} test")) && summary.contains(" run:"),
        "{want} tests expected: {summary}"
    );
    summary
}

async fn demos_load(env: Arc<Env>) -> String {
    let aae = env.aae.to_string_lossy().into_owned();
    for demo in ["1-planner-executor"] {
        let config = format!("demos/{demo}/root/config.toml");
        command(&env, &aae, &["config", "check", "--config", &config]).await;
    }
    "`aae config check` of demos/<demo>/root/config.toml is clean for each demo".into()
}

/// One demo on real models (spec §15 "Demo"); its `run.sh` leaves the result in the demo's directory, ignored by git.
async fn demo(env: Arc<Env>, name: &str) -> String {
    let aae = format!("AAE={}", env.aae.display());
    let out = command(&env, "env", &[&aae, &format!("demos/{name}/run.sh")]).await;
    line_with(&out, name)
}

async fn budget(env: Arc<Env>) -> String {
    let out = command(&env, "scripts/budget.sh", &[]).await;
    line_with(&out, "budget: total")
}

async fn no_stubs(env: Arc<Env>) -> String {
    command(
        &env,
        "cargo",
        &["clippy", "--all-targets", "--locked", "-q", "--", "-D", "warnings"],
    )
    .await;
    // Not `cargo machete`: under `cargo run` it inherits `CARGO_PKG_NAME`, takes "machete" for a path and fails.
    command(&env, "cargo-machete", &[]).await;
    command(&env, "scripts/unused-pub.sh", &[]).await;
    "clippy (todo, unimplemented, unreachable_pub denied), cargo machete and scripts/unused-pub.sh are clean".into()
}

/// Criterion 22 against the rest of the report (spec §14, §2, decision 53), the report's rows as `(criterion, mark)`.
/// A score of 8 is "the component's §15 criteria green", so one of its criteria (the `Criteria` column of
/// docs/READINESS.md) open caps it at 6: a `FAIL` row, or in a full run a row not run or left out. A 10 needs every
/// row of criterion 20 green in this run. A full run also needs every component at 8 or more, the letter of
/// criterion 22; a CI run cannot judge the rows with a model and checks only the caps. `Ok`: the components below 8.
pub(crate) fn readiness_verdict(readiness: &str, rows: &[(&str, &str)], full: bool) -> Result<String, String> {
    let marks = |c: &str| -> Vec<&str> { rows.iter().filter(|(r, _)| *r == c).map(|(_, m)| *m).collect() };
    let open = |c: &&str| {
        let marks = marks(c);
        (full && marks.is_empty()) || marks.iter().any(|m| *m == "FAIL" || (full && *m == "not run"))
    };
    let twenty = marks("20");
    let twenty = !twenty.is_empty() && twenty.iter().all(|m| *m == "pass");
    let (mut low, mut wrong) = (Vec::new(), Vec::new());
    let mut table = readiness.lines().filter(|l| l.starts_with("| "));
    let header: Vec<&str> = table.next().unwrap_or_default().split('|').map(str::trim).collect();
    let column = |name: &str| header.iter().position(|h| *h == name);
    let (Some(criteria_at), Some(score_at)) = (column("Criteria"), column("Score")) else {
        return Err("docs/READINESS.md: the table has no `Criteria` and `Score` columns".into());
    };
    for line in table {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (name, score) = (cells[1], cells.get(score_at).copied().unwrap_or_default());
        let Ok(score) = score.parse::<f64>() else {
            wrong.push(format!("{name}: `{score}` is not a score"));
            continue;
        };
        let criteria = cells
            .get(criteria_at)
            .copied()
            .unwrap_or_default()
            .split(',')
            .map(str::trim);
        if let Some(c) = criteria.filter(|c| !c.is_empty() && *c != "-").find(open)
            && score > 6.0
        {
            wrong.push(format!("{name} scores {score} with criterion {c} open: at most 6"));
        }
        if score >= 10.0 && !twenty {
            wrong.push(format!(
                "{name} scores {score} while criterion 20 is not green in this run"
            ));
        }
        if score < 8.0 {
            low.push(format!("{name} {score}"));
        }
    }
    if !wrong.is_empty() {
        return Err(format!("docs/READINESS.md claims too much: {}", wrong.join("; ")));
    }
    match low.is_empty() {
        true => Ok("every component at 8 or more".into()),
        false if full => Err(format!(
            "criterion 22 needs every component at 8 or more: {}",
            low.join(", ")
        )),
        false => Ok(format!("below 8: {}", low.join(", "))),
    }
}

/// The readiness file is current: its `MEASURE id=readiness` marker names (`ver=`) a commit at or after the last
/// one that changed `crates/`.
pub(crate) fn readiness_fresh(readiness: &str) -> Result<(), String> {
    let marker = readiness.lines().find(|l| l.contains("MEASURE id=readiness"));
    let ver = marker.and_then(|m| m.split_whitespace().find_map(|w| w.strip_prefix("ver=")));
    let ver = ver.ok_or("docs/READINESS.md has no `MEASURE id=readiness ... ver=<sha>` marker")?;
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root())
            .output();
        out.ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let last = git(&["log", "-1", "--format=%h", "--", "crates"]).ok_or("git log of crates/ failed")?;
    match git(&["merge-base", "--is-ancestor", &last, ver]) {
        Some(_) => Ok(()),
        None => Err(format!(
            "docs/READINESS.md is assessed at {ver}, not at or after {last}, the last change of crates/: re-assess it"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows of §15, criterion 21 withdrawn: the report shows each, run or not.
    const ROWS: &[&str] = &[
        "1", "2", "3", "4", "4a", "5a", "5b", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18",
        "19", "20", "22", "23", "24", "25", "26",
    ];

    #[test]
    fn every_row_of_section_15_is_in_the_report() {
        let known: Vec<&str> = (ALL.iter().chain(crate::scenarios::ALL).map(|s| s.criterion)).collect();
        for row in ROWS {
            assert!(known.contains(row), "criterion {row} has no check or scenario");
        }
        assert!(known.iter().all(|c| ROWS.contains(c)), "{known:?}");
    }

    #[test]
    fn readiness_may_not_claim_more_than_the_open_rows_allow() {
        let table =
            |score: &str| format!("| Component | Criteria | Score |\n|---|---|---|\n| CLI | 3, 17 | {score} |\n");
        let pass = [("3", "pass"), ("17", "pass"), ("17", "not run"), ("20", "pass")];
        assert_eq!(
            readiness_verdict(&table("8"), &pass[..3], false),
            Ok("every component at 8 or more".into())
        );
        // A row not run caps the score in a full run only; a FAIL caps it in either.
        assert!(
            readiness_verdict(&table("8"), &pass[..3], true)
                .unwrap_err()
                .contains("criterion 17 open")
        );
        let failed = [("3", "FAIL"), ("17", "pass")];
        assert!(
            readiness_verdict(&table("8"), &failed, false)
                .unwrap_err()
                .contains("at most 6")
        );
        assert_eq!(
            readiness_verdict(&table("6"), &failed, false),
            Ok("below 8: CLI 6".into())
        );
        // A full run needs 8 everywhere; a 10 needs criterion 20 green.
        let all = [("3", "pass"), ("17", "pass"), ("20", "pass")];
        assert!(
            readiness_verdict(&table("6"), &all, true)
                .unwrap_err()
                .contains("at 8 or more: CLI 6")
        );
        assert!(readiness_verdict(&table("10"), &all, true).is_ok());
        assert!(
            readiness_verdict(&table("10"), &all[..2], false)
                .unwrap_err()
                .contains("criterion 20")
        );
        assert!(readiness_verdict("| Component | Score |\n| CLI | 8 |\n", &all, false).is_err());
    }

    #[test]
    fn readiness_is_stale_before_the_last_change_of_the_code() {
        let marker = |ver: &str| format!("<!-- {} id=readiness date=2026-11-14 ver={ver} env=x -->\n", "MEASURE");
        assert_eq!(readiness_fresh(&marker("HEAD")), Ok(()));
        let first = std::process::Command::new("git")
            .args(["rev-list", "--max-parents=0", "HEAD"])
            .output()
            .unwrap();
        let first = String::from_utf8(first.stdout).unwrap();
        let first = first.lines().next().unwrap();
        assert!(readiness_fresh(&marker(first)).unwrap_err().contains("re-assess"));
        assert!(readiness_fresh("no marker").is_err());
    }
}
