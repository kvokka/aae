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
                39,
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
        criterion: "20",
        title: "demo 2: demo 1 on openai models, then a rumdl check and a qa agent in a browser send the work back to \
            the planner until both pass, in at most three rounds, unattended",
        launch: Launch::Request,
        run: |env| Box::pin(demo(env, "2-multirepo-qa")),
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
    Scenario {
        criterion: "27",
        title: "a harness of the build needs no section; `[agents.<name>]` edits an agent or declares a variant and \
            explain names its layer; a retired key is one line that says where it went; the root takes a name and \
            a path, root and next to aae.toml without them, need not be a git repository, and runs the project \
            triggers without a rig",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p aae-core -p aae-testkit -E test(/a_harness_of_the_build_needs_no_section|a_retired_key_is_one_line\
                 |variant_errors|an_agents_table_changes_the_harness|a_rig_runs_a_variant_in_place_of_the_agent\
                 |a_project_without_a_root_section|the_root_takes_a_name_and_a_path|a_rig_at_the_root_path\
                 |a_root_with_a_name_and_a_path_is_where_its_runs_work|a_run_without_a_rig_works_in_the_root_that_is_no_git_repository\
                 |a_worktree_in_a_directory_without_git_fails_that_run|a_project_trigger_without_a_rig_runs_in_the_root/)",
                12,
            ))
        },
    },
    Scenario {
        criterion: "32",
        title: "an ai step of an agent of scope root works in the root, of scope rig in the rig, and no rig edits it; \
            a project of one repository runs without rigs, its beads in the root",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p aae-core -p aae-testkit -E test(/an_agent_of_scope_root_takes_no_rig_layer\
                 |a_project_of_one_repository_checks_its_formulas_on_the_root|a_run_without_a_rig_works_in_the_root\
                 |a_root_agent_works_in_the_root_whatever_the_rig|a_bead_of_the_root_runs_in_the_root\
                 |a_rig_neither_edits_nor_maps_an_agent_of_the_root/)",
                6,
            ))
        },
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
    for demo in ["1-planner-executor", "2-multirepo-qa"] {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows of §15, criterion 21 withdrawn: the report shows each, run or not.
    const ROWS: &[&str] = &[
        "1", "2", "3", "4", "4a", "5a", "5b", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18",
        "19", "20", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32", "33", "34",
    ];

    #[test]
    fn every_row_of_section_15_is_in_the_report() {
        let checked = ALL.iter().chain(crate::scenarios::ALL).map(|s| s.criterion);
        let known: Vec<&str> = checked.chain(crate::readiness::OPEN.iter().map(|(c, _)| *c)).collect();
        for row in ROWS {
            assert!(known.contains(row), "criterion {row} has no check or scenario");
        }
        assert!(known.iter().all(|c| ROWS.contains(c)), "{known:?}");
    }
}
