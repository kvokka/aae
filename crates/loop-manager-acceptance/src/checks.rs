//! The rows of docs/acceptance.md that live in cargo builds, cargo tests and the CI scripts (decision 44): each runs
//! its commands in the workspace, with the target directory of the spec, and passes when all of them succeed. They
//! run one after another once the scenarios are done (see `main`).

use crate::{Env, Launch, Scenario};
use std::{env::consts, path::PathBuf, sync::Arc};

/// The four release targets of docs/guide/install.md.
const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

const KILLED_IN_A_COMMAND: &str = "-p loop-manager -E test(/killed_in_a_command_is_resumed/)";
pub(crate) const ALL: &[Scenario] = &[
    Scenario {
        criterion: "1",
        title: "the release build for this host's target runs without extra runtimes",
        launch: Launch::Request,
        run: |env| Box::pin(release(env, &[])),
    },
    Scenario {
        criterion: "1",
        title: "one binary builds for each of the four targets",
        launch: Launch::Release,
        run: |env| Box::pin(release(env, &TARGETS)),
    },
    Scenario {
        criterion: "9",
        title: "kill -9 of claude and of the codex app-server mid-tool is typed, resumed and reaches its outcome",
        launch: Launch::Ci,
        run: |env| Box::pin(tests(env, KILLED_IN_A_COMMAND, 2)),
    },
    Scenario {
        criterion: "35",
        title: "the chaos stand: random steps on 1000 new seeds keep the invariants of the core",
        launch: Launch::Ci,
        run: |env| Box::pin(tests(env, "-p loop-manager-testkit --test chaos", 10)),
    },
    Scenario {
        criterion: "16",
        title: "the claim holds through the death of a claude session and its resume; the session has the tracker's path",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager -E test(a_claude_killed_in_a_command_is_resumed_and_its_run_passes)",
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
                "-p loop-manager-testkit -E test(a_schedule_fires_on_the_managed_clock_and_once_after_downtime)",
                1,
            ))
        },
    },
    Scenario {
        criterion: "11",
        title: "the contract tests of claude and codex, codex's model list, and of the br tracker pass",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-claude -p loop-manager-codex -p loop-manager-br -E binary(contract)|test(/^contract::/)|test(/^models::/)",
                41,
            ))
        },
    },
    Scenario {
        criterion: "17",
        title: "a CLI command per API operation, the committed OpenAPI matches the code, its path parameters are the \
            names in its paths, real replies fit it, MCP serves its five tools",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-front -p loop-manager -E binary(openapi)|test(/^replies::/)|test(both_epochs_serve_the_same_five_tools)\
                 |test(a_codex_thread_that_calls_the_finisher_passes_its_run)",
                8,
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
        run: |env| Box::pin(demo(env, "1-planner-executor", 1)),
    },
    Scenario {
        criterion: "20",
        title: "demo 2: demo 1 on openai models, then a rumdl check and a qa agent in a browser send the work back to \
            the planner until both pass, in at most three rounds, unattended, two runs in a row",
        launch: Launch::Request,
        run: |env| Box::pin(demo(env, "2-multirepo-qa", 2)),
    },
    Scenario {
        criterion: "20",
        title: "demo 3: demo 1 with pull requests on GitHub, reviewed there by an agent in place of a human, the work \
            sent back once and the pull requests merged, unattended; needs gh logged in or GITHUB_TOKEN",
        launch: Launch::Request,
        run: |env| Box::pin(demo(env, "3-review-gate", 1)),
    },
    Scenario {
        criterion: "20",
        title: "demo 4: an agent on claude reads past the soft limit of the context-budget addon, wraps up and leaves \
            a summary comment on the bead of its run, unattended",
        launch: Launch::Request,
        run: |env| Box::pin(demo(env, "4-context-budget", 1)),
    },
    Scenario {
        criterion: "22",
        title: "line budgets and MEASURE markers (scripts/budget.sh)",
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
            a path, root and next to loop-manager.toml without them, need not be a git repository, and runs the project \
            triggers without a rig",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-contract -p loop-manager-core -p loop-manager-testkit -E test(/a_harness_of_the_build_needs_no_section|a_retired_key_is_one_line\
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
            a project of one repository runs without rigs, its beads in the root; a rig's beads carry its prefix, the \
            acronym of its name or its `prefix` key, and `drain` takes the rig of a child from it",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-contract -p loop-manager-testkit -p loop-manager-br -E test(/an_agent_of_scope_root_takes_no_rig_layer\
                 |a_project_of_one_repository_checks_its_formulas_on_the_root|a_run_without_a_rig_works_in_the_root\
                 |a_root_agent_works_in_the_root_whatever_the_rig|a_bead_of_the_root_runs_in_the_root\
                 |a_rig_neither_edits_nor_maps_an_agent_of_the_root|a_rig_takes_the_acronym_of_its_name_or_its_prefix_key\
                 |two_rigs_with_one_prefix_or_a_prefix_br_rewrites_are_an_error|the_tracker_variables_come_under_every_layer\
                 |a_bead_filed_with_a_rig_prefix_under_a_root_keeps_it_and_parent_loses_it\
                 |a_drain_gives_every_ready_bead_under_the_root_a_run\
                 |a_start_without_a_rig_takes_it_from_the_prefix_of_the_bead_id_or_runs_in_the_root/)",
                12,
            ))
        },
    },
    Scenario {
        criterion: "28",
        title: "a task: a text files one root bead, also across a crash, an id of a bead takes that bead up and files \
            none, on the real br and a real daemon too; a formula declares no input of type `bead`",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-contract -p loop-manager-testkit -p loop-manager-br -p loop-manager -E test(/a_text_task_becomes_one_root_bead\
                 |a_root_bead_created_before_a_crash_is_the_one_the_run_gets|a_blank_task_or_another_form_is_refused\
                 |a_task_naming_a_bead_takes_it_up_and_files_no_second_bead|a_drain_step_names_a_formula_that_takes_one_bead\
                 |a_task_naming_a_bead_gives_it_and_a_text_files_a_root_of_the_database_prefix\
                 |a_task_takes_up_the_bead_it_names_and_a_text_files_one_root/)",
                7,
            ))
        },
    },
    Scenario {
        criterion: "33",
        title: "an agent's prompt template is its `prompt.template.md`; the project's `.loop-manager/agents/<agent>/` replaces it \
            and fills it with its files; a missing template or file is a config error naming the path",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-contract -p loop-manager-testkit -p loop-manager -E test(/a_project_template_replaces_the_packs\
                 |a_missing_template_or_named_file_is_an_error|an_agent_named_by_a_step_without_a_template\
                 |a_template_fits_every_formula_whose_steps_name_its_agent|a_retired_key_is_one_line\
                 |a_run_gets_the_project_template_of_its_agent|the_config_is_checked_and_explained_without_a_daemon\
                 |a_variant_runs_on_the_template_of_its_base_agent/)",
                8,
            ))
        },
    },
    Scenario {
        criterion: "29",
        title: "a step runs again up to `max_attempts` while its check script fails, the script's output its remark in \
            the same session; exit 75 fails it at once; a crash or a restart keeps the count; a round starts it again; a timeout, a signal or a lost answer of the script \
            does not pass, a script that cannot start fails the run",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-contract -p loop-manager-testkit -E test(/a_step_runs_again_until_its_check_script_passes\
                 |past_max_attempts_the_step_fails|exit_75_of_the_check_script_fails_the_step_at_once\
                 |a_step_that_fails_on_its_own_runs_again_without_its_script|a_crash_between_attempts_keeps_the_count\
                 |a_check_script_cut_by_a_restart_runs_again_and_counts_once\
                 |a_check_script_gets_the_step_and_its_output_is_the_remark\
                 |a_repeat_stays_in_its_round_and_a_round_starts_the_count_again\
                 |a_step_of_any_kind_takes_max_attempts_and_a_check_script|a_bad_max_attempts_or_check_script_is_one_line\
                 |a_check_script_past_the_step_timeout_does_not_pass_it|a_check_script_killed_by_a_signal_does_not_pass_it\
                 |a_lost_check_script_is_a_timeout_at_the_deadline_of_finishing|a_check_script_that_cannot_start_fails_the_run\
                 |exit_75_of_the_check_script_of_a_check_step_fails_the_run_with_no_return/)",
                15,
            ))
        },
    },
    Scenario {
        criterion: "30",
        title: "a drain step keeps at most `pool` beads in work, 1 unless set; `on_item_failure` stops it softly or \
                hard and fails it, a restart giving no new bead, a repeat giving them again",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-contract -p loop-manager-testkit -E test(/a_drain_step_takes_pool_and_on_item_failure\
                 |without_pool_one_bead_is_in_work_at_a_time|pool_two_keeps_two_of_five_in_work\
                 |soft_stop_gives_no_new_bead|hard_stop_cancels_the_started_beads|a_restart_mid_stop_gives_no_new_bead\
                 |a_repeat_after_a_stop_is_a_new_entry/)",
                7,
            ))
        },
    },
    Scenario {
        criterion: "31",
        title: "a formula step runs its formula as a child run with the inputs it declares and the step's, the \
                task among them on the bead the parent holds, its outcome the step's, cancelled with its parent and \
                not doubled by a restart; an internal formula \
                starts only from a step; the config refuses a step short of an input, one the formula lacks, a \
                formula that runs itself and a trigger on an internal formula",
        launch: Launch::Ci,
        run: |env| {
            Box::pin(tests(
                env,
                "-p loop-manager-contract -p loop-manager-testkit -E test(/^formula_step::|a_formula_step_names_a_formula_whose_inputs_it_has/)",
                14,
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

/// The release build for this host's target and `targets` besides, then the host's binary runs.
async fn release(env: Arc<Env>, targets: &[&str]) -> String {
    let os = if consts::OS == "macos" {
        "apple-darwin"
    } else {
        "unknown-linux-musl"
    };
    let host = format!("{}-{os}", consts::ARCH);
    let mut built: Vec<&str> = targets.to_vec();
    if !built.contains(&host.as_str()) {
        built.push(&host);
    }
    for target in &built {
        let mut args = Vec::from_iter("zigbuild -q --release --locked -p loop-manager --target".split(' '));
        args.push(target);
        command(&env, "cargo", &args).await;
    }
    let binary = env.target.join(&host).join("release/loop-manager");
    let version = command(&env, &binary.to_string_lossy(), &["--version"]).await;
    format!(
        "built for {}; the {host} build runs: {}",
        built.join(", "),
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
    let bin = env.loop_manager.to_string_lossy().into_owned();
    let demos = std::fs::read_dir(root().join("demos")).unwrap();
    for config in demos
        .map(|d| d.unwrap().path().join("root/config.toml"))
        .filter(|c| c.exists())
    {
        command(&env, &bin, &["config", "check", "--config", &config.to_string_lossy()]).await;
    }
    "`loop-manager config check` of demos/<demo>/root/config.toml is clean for each demo".into()
}

/// One demo on real models (docs/acceptance.md "Demo"), `times` runs in a row; its `run.sh` leaves the result in the
/// demo's directory, ignored by git, and each run starts from what `git clean` leaves, as the demo's README says.
/// What the agents may have left running is neither looked for nor killed.
async fn demo(env: Arc<Env>, name: &str, times: usize) -> String {
    let var = format!("LOOP_MANAGER={}", env.loop_manager.display());
    let dir = format!("demos/{name}");
    let mut runs = Vec::new();
    for _ in 0..times {
        command(&env, "git", &["clean", "-ffdXq", "--", &dir]).await;
        let out = command(&env, "env", &[&var, &format!("{dir}/run.sh")]).await;
        runs.push(line_with(&out, name));
    }
    runs.join("; ")
}

async fn budget(env: Arc<Env>) -> String {
    let out = command(&env, "scripts/budget.sh", &[]).await;
    line_with(&out, "budget: total")
}

async fn no_stubs(env: Arc<Env>) -> String {
    command(
        &env,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "-q",
            "--",
            "-D",
            "warnings",
        ],
    )
    .await;
    // Not `cargo machete`: under `cargo run` it inherits `CARGO_PKG_NAME`, takes "machete" for a path and fails.
    command(&env, "cargo-machete", &[]).await;
    command(&env, "scripts/unused-pub.sh", &[]).await;
    "clippy (todo, unimplemented, unreachable_pub denied), cargo machete and scripts/unused-pub.sh are clean".into()
}

#[cfg(test)]
mod tests;
