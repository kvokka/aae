//! The scenarios of the spec, one file per criterion of spec §15, and what several of them share.

mod c02_prompt;
mod c03_exec;
mod c04_finisher;
mod c05a_chain;
mod c05b_schedule;
mod c06_hop;
mod c07_idle;
mod c08_crash;
mod c16_beads;
mod c18_journal;
mod c19_gates;

use crate::{Launch, Scenario, daemon::write};
use std::{path::Path, process::Command};

/// Kept apart from the marker text, so the marker check of CI does not take the template for a marker.
pub(crate) const MARK: &str = "MEASURE";

pub(crate) const ALL: &[Scenario] = &[
    Scenario {
        criterion: "2",
        title: "a prompt step passes on the cheapest real claude model by the finisher",
        launch: Launch::Request,
        run: |env| Box::pin(c02_prompt::run(env)),
    },
    Scenario {
        criterion: "3",
        title: "an exec step passes; exit 75 fails the attempt, not the formula",
        launch: Launch::Ci,
        run: |env| Box::pin(c03_exec::run(env)),
    },
    Scenario {
        criterion: "4",
        title: "no pass without the finisher; a stale finisher is rejected; a remark passes on the second go",
        launch: Launch::Ci,
        run: |env| Box::pin(c04_finisher::run(env)),
    },
    Scenario {
        criterion: "5a",
        title: "an exec planner creates three beads of an epic; a subscription closes each; one correlation",
        launch: Launch::Ci,
        run: |env| Box::pin(c05a_chain::run(env)),
    },
    Scenario {
        criterion: "5b",
        title: "a schedule fires on time; the firings missed while the daemon is down fire once",
        launch: Launch::Ci,
        run: |env| Box::pin(c05b_schedule::run(env)),
    },
    Scenario {
        criterion: "6",
        title: "30 hops from run.passed of A to the harness launch of B by a subscription: p50 <= 1 s, p90 <= 3 s",
        launch: Launch::Ci,
        run: |env| Box::pin(c06_hop::run(env)),
    },
    Scenario {
        criterion: "7",
        title: "the idle daemon with one project: RSS <= 50 MB, CPU <= 1 % over 10 minutes",
        launch: Launch::Ci,
        run: |env| Box::pin(c07_idle::run(env)),
    },
    Scenario {
        criterion: "8",
        title: "20 x kill -9 of the daemon mid-step: up within 30 s, each run ends once, no firing lost or doubled",
        launch: Launch::Ci,
        run: |env| Box::pin(c08_crash::run(env)),
    },
    Scenario {
        criterion: "16",
        title: "a bead: claim held through a crash, returned on failure, closed on pass, one run per bead",
        launch: Launch::Ci,
        run: |env| Box::pin(c16_beads::run(env)),
    },
    Scenario {
        criterion: "18",
        title: "the journal rotates at 10 MiB x 5; a reader continues by seq; a rotated-away seq is a gap",
        launch: Launch::Ci,
        run: |env| Box::pin(c18_journal::run(env)),
    },
    Scenario {
        criterion: "19",
        title: "operator gates stop dispatch, survive a restart and stay until opened by hand",
        launch: Launch::Ci,
        run: |env| Box::pin(c19_gates::run(env)),
    },
];

/// Writes an exec formula `name` of `command` (TOML array items) into project `shop` under `root`.
pub(crate) fn exec_formula(root: &Path, name: &str, inputs: &str, command: &str) {
    let text = format!(
        "name = \"{name}\"\n[inputs]\n{inputs}\n[step]\nkind = \"exec\"\ncommand = [{command}]\ntimeout = \"30s\"\n"
    );
    write(root.join(format!("config/aae/shop/formulas/{name}.toml")), text);
}

/// Agent `name` of project `shop` on the real `claude` against a fake Anthropic API at `base_url`, without the host
/// layer, and the prompt formula `name` that runs it; `[harness.claude]` in the orchestrator config.
pub(crate) fn claude_agent(root: &Path, name: &str, base_url: &str) {
    write(
        root.join("config/aae/config.toml"),
        "projects = [\"shop\"]\n[harness.claude]\n",
    );
    write(
        root.join(format!("config/aae/shop/agents/{name}/agent.toml")),
        format!(
            "harness = \"claude\"\nmodel = \"claude-haiku-5-5\"\npermission_mode = \"bypassPermissions\"\n\
             inherit_host = false\n[env]\nANTHROPIC_BASE_URL = \"{base_url}\"\nANTHROPIC_API_KEY = \"fake\"\n\
             CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = \"1\"\nCLAUDE_CODE_MAX_RETRIES = \"0\"\n"
        ),
    );
    write(
        root.join(format!("config/aae/shop/formulas/{name}.toml")),
        format!("name = \"{name}\"\n[step]\nkind = \"prompt\"\nagent = \"{name}\"\nprompt = \"{name}.md\"\n"),
    );
    write(
        root.join(format!("config/aae/shop/formulas/{name}.md")),
        "Do the work on {{ rig.name }}.",
    );
}

/// `br <args>` as `alice` on the tracker of project `shop` under `root`; its trimmed stdout.
pub(crate) fn br(root: &Path, args: &[&str]) -> String {
    let project = root.join("config/aae/shop");
    let out = Command::new("br")
        .args(args)
        .args(["--actor", "alice"])
        .env("BEADS_DIR", project.join(".beads"))
        .env("RUST_LOG", "error")
        .current_dir(&project)
        .output()
        .expect("br on PATH");
    assert!(
        out.status.success(),
        "br {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A git repository with a br tracker of prefix `t` in project `shop` under `root`.
pub(crate) fn tracker(root: &Path) {
    let project = root.join("config/aae/shop");
    let git = Command::new("git").args(["init", "-q"]).current_dir(&project).status();
    assert!(git.is_ok_and(|s| s.success()), "git init");
    br(root, &["init", "--prefix", "t", "-q"]);
}
