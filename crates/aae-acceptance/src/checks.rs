//! The rows of spec §15 that live in cargo builds, cargo tests and the CI scripts (spec §2.1 item 7): each runs its
//! commands in the workspace, with the target directory of the spec, and passes when all of them succeed. They run
//! one after another once the scenarios are done (see `main`).

use crate::{Env, Launch, Scenario};
use std::{env::consts, path::PathBuf, sync::Arc};

/// Rows with nothing to run yet: the report shows them open, and an open row fails a whole run of the spec.
pub(crate) const OPEN: &[(&str, &str, &str)] = &[(
    "20",
    "demos 1 and 2 pass from launch to a green smoke test without intervention",
    "no scenario: the demos/ of §15 are not built yet",
)];

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
        launch: Launch::Ci,
        run: |env| Box::pin(release(env)),
    },
    Scenario {
        criterion: "9",
        title: "kill -9 of claude and of the codex app-server mid-tool is typed, resumed and reaches its outcome",
        launch: Launch::Ci,
        run: |env| Box::pin(tests(env, "-p aae -E test(/killed_in_a_command_is_resumed/)")),
    },
    Scenario {
        criterion: "11",
        title: "the contract tests of claude and codex (§7.8) and of the br tracker (§10.2) pass",
        launch: Launch::Ci,
        run: |env| Box::pin(tests(env, "-p aae-claude -p aae-codex -p aae-br -E binary(contract)")),
    },
    Scenario {
        criterion: "22",
        title: "line budgets, MEASURE markers and a readiness file whose scores the open rows allow",
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

/// The nextest run of `filter` (space-separated arguments); its summary.
async fn tests(env: Arc<Env>, filter: &str) -> String {
    let mut args = vec!["nextest", "run", "--locked", "--no-fail-fast"];
    args.extend(filter.split(' '));
    line_with(&command(&env, "cargo", &args).await, "Summary")
}

async fn budget(env: Arc<Env>) -> String {
    let out = command(&env, "scripts/budget.sh", &[]).await;
    format!(
        "{}; readiness checked against the open rows",
        line_with(&out, "budget: total")
    )
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

/// Criterion 22 against the rest of the report (spec §14, §2.1 item 16): a score of 8 needs the component's rows
/// green and 10 needs criterion 20, while `budget.sh` holds every score at 8 or more. So any open row but 20 makes
/// the readiness file claim too much, and so does a 10 while 20 is open.
pub(crate) fn readiness_verdict(readiness: &str, open: &[&str]) -> Result<(), String> {
    let below_ten: Vec<&str> = open.iter().copied().filter(|c| *c != "20").collect();
    if !below_ten.is_empty() {
        return Err(format!(
            "docs/READINESS.md scores every component 8 or more while these criteria are open: {}",
            below_ten.join(", ")
        ));
    }
    let ten = readiness.lines().filter(|l| l.starts_with("| ")).find(|l| {
        let score = l.split('|').nth(2).unwrap_or_default().trim();
        score.parse::<f64>().is_ok_and(|s| s >= 10.0)
    });
    match ten {
        Some(row) if open.contains(&"20") => Err(format!("a score of 10 while criterion 20 is open: {row}")),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows of §15, criterion 21 withdrawn: the report shows each, run or not.
    const ROWS: &[&str] = &[
        "1", "2", "3", "4", "4a", "5a", "5b", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18",
        "19", "20", "22", "23",
    ];

    #[test]
    fn every_row_of_section_15_is_in_the_report() {
        let known: Vec<&str> = (ALL.iter().chain(crate::scenarios::ALL).map(|s| s.criterion))
            .chain(OPEN.iter().map(|(c, ..)| *c))
            .collect();
        for row in ROWS {
            assert!(
                known.contains(row),
                "criterion {row} has no check, scenario or open entry"
            );
        }
        assert!(known.iter().all(|c| ROWS.contains(c)), "{known:?}");
    }

    #[test]
    fn readiness_may_not_claim_more_than_the_open_rows_allow() {
        let eights = "| Component | Score | Missing |\n|---|---|---|\n| CLI | 8 | demos |\n";
        let ten = "| Component | Score | Missing |\n|---|---|---|\n| CLI | 10 | nothing |\n";
        assert_eq!(readiness_verdict(eights, &[]), Ok(()));
        assert_eq!(readiness_verdict(eights, &["20"]), Ok(()));
        assert_eq!(readiness_verdict(ten, &[]), Ok(()));
        assert!(readiness_verdict(ten, &["20"]).is_err());
        assert!(
            readiness_verdict(eights, &["9", "20"])
                .unwrap_err()
                .ends_with("open: 9")
        );
    }
}
