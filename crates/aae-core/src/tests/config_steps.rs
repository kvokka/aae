//! The steps of a formula file (spec §9.1, docs/design/config.md "Formula").

use super::config::{fails, load_with};

const FLOW: &str = "kit/formulas/flow.toml";
/// The agent of the `ai` step of `FLOW`, with a template of its own: an agent's template fits every formula naming it.
const WORKER: (&str, &str) = ("kit/agents/worker/agent.toml", "harness = \"claude\"\n");
const WORKER_TEMPLATE: &str = "kit/agents/worker/prompt.template.md";

fn exec(name: Option<&str>) -> String {
    let name = name.map(|n| format!("name = \"{n}\"\n")).unwrap_or_default();
    format!("[[step]]\n{name}kind = \"exec\"\ncommand = [\"true\"]\n")
}

#[test]
fn steps_are_named_by_their_position_unless_named() {
    let flow = format!("name = \"flow\"\n{}{}{}", exec(None), exec(Some("check")), exec(None));
    let config = load_with(&[(FLOW, &flow)]).unwrap();
    let formula = &config.projects[0].packs[1].formulas["flow"];
    let names: Vec<&str> = formula.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["1", "check", "3"]);
    assert_eq!(formula.next("check").map(|s| s.name.as_str()), Some("3"));
    assert!(formula.next("3").is_none());
}

#[test]
fn bad_steps_are_errors() {
    let flow = format!(
        "name = \"flow\"\n{}[[step]]\nname = \"work\"\nkind = \"ai\"\nagent = \"worker\"\n{}",
        exec(Some("pre")),
        exec(Some("post"))
    );
    let with = |text: &'static str| load_with(&[(FLOW, &flow), WORKER, (WORKER_TEMPLATE, text)]);
    let refused = |text: &'static str| {
        with(text)
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("is not a value"))
    };
    assert!(refused("{{ steps.post.result }}"), "a later step");
    assert!(refused("{{ steps.pre.result.x }}"), "below a result");
    assert!(with("{{ steps.pre.data.stdout }} {{ steps.pre.summary }}").is_ok());
    for (flow, error) in [
        (
            format!("name = \"flow\"\n{}{}", exec(Some("a")), exec(Some("a"))),
            "two steps are named `a`",
        ),
        (
            format!("name = \"flow\"\n{}{}", exec(Some("a-b")), exec(Some("a_b"))),
            "step `a-b` and step `a_b` are both `AAE_STEP_A_B_*`",
        ),
        (
            format!("name = \"flow\"\n{}", exec(Some("a.b"))),
            "step name `a.b` must be non-empty",
        ),
        (
            "name = \"flow\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\n".to_owned(),
            "expected a sequence",
        ),
        ("name = \"flow\"\nstep = []\n".to_owned(), "at least one `[[step]]`"),
    ] {
        fails(&[(FLOW, &flow)], error);
    }
    let twice = format!("name = \"flow\"\n{}{}", exec(Some("a")), exec(Some("a")));
    let errors = load_with(&[(FLOW, &twice)]).unwrap_err();
    assert_eq!(errors.len(), 1, "one error for one cause: {errors:?}");
}

/// A drain step names the formula of its runs: one that takes the bead and needs nothing else; its own formula
/// takes the bead the beads are under.
#[test]
fn a_drain_step_names_a_formula_that_takes_one_bead() {
    let ship = |inputs: &str, each: &str| {
        format!(
            "name = \"flow\"\n[inputs]\n{inputs}\n[[step]]\nname = \"all\"\nkind = \"drain\"\nformula = \"{each}\"\n"
        )
    };
    let bead = "root = { type = \"task\", required = true }";
    let config = load_with(&[(FLOW, &ship(bead, "implement"))]).unwrap();
    let step = &config.projects[0].packs[1].formulas["flow"].steps[0];
    let crate::config::StepKind::Drain {
        formula, timeout: None, ..
    } = &step.kind
    else {
        panic!("{step:?}");
    };
    assert_eq!(
        (formula.to_string().as_str(), step.kind.name()),
        ("kit.implement", "drain")
    );
    fails(&[(FLOW, &ship(bead, "nothing"))], "step `all`: no formula `nothing`");
    fails(
        &[(FLOW, &ship(bead, "audit"))],
        "`kit.audit` needs exactly one input of type `task`",
    );
    let two = "name = \"two\"\n[inputs]\nbead = { type = \"task\" }\nwhy = { type = \"string\", required = true }\n\
               [[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n";
    fails(
        &[(FLOW, &ship(bead, "two")), ("kit/formulas/two.toml", two)],
        "`kit.two` requires input `why`",
    );
    fails(
        &[(FLOW, &ship("root = { type = \"task\" }", "implement"))],
        "a formula with a `drain` step needs a required input of type `task`",
    );
    // No input takes a bead: a formula takes a task (spec §9.1).
    fails(
        &[(FLOW, &ship("root = { type = \"bead\" }", "implement"))],
        "input type `bead` is `task` now",
    );
}

/// A drain step keeps `pool` beads in work, 1 unless set, and stops on a failed bead as `on_item_failure` says,
/// `continue` unless set (spec §9.1); a bad value is one error, placed at the step's header.
#[test]
fn a_drain_step_takes_pool_and_on_item_failure() {
    use crate::config::{OnItemFailure, StepKind};
    let ship = |keys: &str| {
        format!(
            "name = \"flow\"\n[inputs]\nroot = {{ type = \"task\", required = true }}\n\
             [[step]]\nname = \"all\"\nkind = \"drain\"\nformula = \"implement\"\n{keys}"
        )
    };
    let drain = |keys: &str| {
        let config = load_with(&[(FLOW, &ship(keys))]).unwrap();
        match &config.projects[0].packs[1].formulas["flow"].steps[0].kind {
            StepKind::Drain {
                pool, on_item_failure, ..
            } => (*pool, *on_item_failure),
            kind => panic!("{kind:?}"),
        }
    };
    assert_eq!(drain(""), (1, OnItemFailure::Continue));
    assert_eq!(
        drain("pool = 3\non_item_failure = \"hard_stop\"\n"),
        (3, OnItemFailure::HardStop)
    );
    assert_eq!(drain("on_item_failure = \"soft_stop\"\n").1, OnItemFailure::SoftStop);
    for (keys, needle) in [
        ("pool = 0\n", "expected a nonzero u32"),
        ("on_item_failure = \"stop\"\n", "unknown variant `stop`"),
    ] {
        let errors = load_with(&[(FLOW, &ship(keys))]).expect_err(keys);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].message.contains(needle), "{errors:?}");
    }
    let exec = "name = \"flow\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\npool = 2\n";
    fails(&[(FLOW, exec)], "unknown field `pool`");
}

/// A check names an earlier step to go back to; the formula bounds the rounds (spec §9.1).
#[test]
fn a_check_goes_back_to_an_earlier_step() {
    let back = |to: &str| format!("back_to = \"{to}\"\n");
    let flow = |rounds: &str, to: &str| {
        format!(
            "name = \"flow\"\n{rounds}{}[[step]]\nname = \"work\"\nkind = \"ai\"\nagent = \"worker\"\n{}{}",
            exec(Some("plan")),
            exec(Some("check")) + &back(to),
            exec(Some("post"))
        )
    };
    let text = (WORKER_TEMPLATE, "Remarks: {{ remarks }}");
    let config = load_with(&[(FLOW, &flow("", "plan")), WORKER, text]).unwrap();
    let formula = &config.projects[0].packs[1].formulas["flow"];
    assert_eq!(formula.rounds, 3, "the default");
    assert_eq!(formula.steps[2].back_to.as_deref(), Some("plan"));
    let config = load_with(&[(FLOW, &flow("rounds = 5\n", "work")), WORKER, text]).unwrap();
    assert_eq!(config.projects[0].packs[1].formulas["flow"].rounds, 5);
    for (to, rounds, error) in [
        (
            "post",
            "",
            "step `check` goes back to `post`, which is no step before it",
        ),
        (
            "check",
            "",
            "step `check` goes back to `check`, which is no step before it",
        ),
        ("plan", "rounds = 0\n", "nonzero"),
    ] {
        fails(&[(FLOW, &flow(rounds, to)), WORKER, text], error);
    }
    let drain = "name = \"flow\"\n[inputs]\nb = { type = \"task\", required = true }\n[[step]]\nname = \"a\"\n\
                 kind = \"drain\"\nformula = \"implement\"\nback_to = \"a\"\n";
    fails(&[(FLOW, drain)], "unknown field `back_to`");
}

/// [`load_with`] with the files `executable` given the execute bit.
fn load_executable(
    changes: &[(&str, &str)],
    executable: &[&str],
) -> Result<crate::config::Config, Vec<crate::config::ConfigError>> {
    use std::{fs, os::unix::fs::PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let base = super::config::BASE
        .iter()
        .filter(|(p, _)| !changes.iter().any(|(c, _)| c == p));
    for (path, text) in base.chain(changes) {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    for path in executable {
        fs::set_permissions(dir.path().join(path), fs::Permissions::from_mode(0o755)).unwrap();
    }
    crate::config::load_for(&dir.path().join("config.toml"), &[], Default::default())
}

const SCRIPT: (&str, &str) = ("kit/formulas/check.sh", "#!/bin/sh\nexit 0\n");

/// Any step may run more than once and have a script say whether it passed; the path is from the formula file.
#[test]
fn a_step_of_any_kind_takes_max_attempts_and_a_check_script() {
    let flow = "name = \"flow\"\n[inputs]\nroot = { type = \"task\", required = true }\n\
                [[step]]\nname = \"work\"\nkind = \"ai\"\nagent = \"worker\"\nmax_attempts = 3\n\
                step_check_script_path = \"check.sh\"\n\
                [[step]]\nname = \"build\"\nkind = \"exec\"\ncommand = [\"true\"]\nstep_check_script_path = \"\"\n\
                [[step]]\nname = \"test\"\nkind = \"exec\"\ncommand = [\"true\"]\nstep_check_script_path = \"/bin/sh\"\n\
                [[step]]\nname = \"all\"\nkind = \"drain\"\nformula = \"implement\"\nmax_attempts = 2\n\
                step_check_script_path = \"../scripts/check\"\n";
    let changes = [
        (FLOW, flow),
        WORKER,
        (WORKER_TEMPLATE, "Work."),
        SCRIPT,
        ("kit/scripts/check", SCRIPT.1),
    ];
    let config = load_executable(&changes, &[SCRIPT.0, "kit/scripts/check"]).unwrap();
    let steps = &config.projects[0].packs[1].formulas["flow"].steps;
    let got: Vec<(u32, Option<&std::path::Path>)> =
        steps.iter().map(|s| (s.max_attempts, s.check.as_deref())).collect();
    assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), [3, 1, 1, 2]);
    assert!(
        got[0].1.is_some_and(|p| p.is_absolute() && p.ends_with(SCRIPT.0)),
        "{got:?}"
    );
    assert_eq!(got[1].1, None, "an empty path is no script");
    assert_eq!(
        got[2].1,
        Some(std::path::Path::new("/bin/sh")),
        "an absolute path stays"
    );
    assert!(got[3].1.is_some_and(|p| p.ends_with("kit/scripts/check")), "{got:?}");
}

/// A zero `max_attempts` or a check script that is no executable file is one error.
#[test]
fn a_bad_max_attempts_or_check_script_is_one_line() {
    let flow = |key: &str| format!("name = \"flow\"\n{}{key}\n", exec(Some("build")));
    let errors = |key: &str, executable: &[&str]| {
        let errors = load_executable(&[(FLOW, &flow(key)), SCRIPT], executable).unwrap_err();
        assert_eq!(errors.len(), 1, "one error for one cause: {errors:?}");
        errors[0].message.clone()
    };
    let zero = errors("max_attempts = 0", &[SCRIPT.0]);
    // The parser places a bad key of a tagged table at the table's header.
    assert!(zero.contains("nonzero") && zero.contains("line 2"), "{zero}");
    assert_eq!(
        errors("max_attempts = 21", &[SCRIPT.0]),
        "step `build`: `max_attempts` is at most 20"
    );
    load_executable(&[(FLOW, &flow("max_attempts = 20")), SCRIPT], &[SCRIPT.0]).unwrap();
    for (script, executable) in [("none.sh", &[SCRIPT.0][..]), ("check.sh", &[])] {
        let message = errors(&format!("step_check_script_path = \"{script}\""), executable);
        assert!(
            message.starts_with("step `build`: the check script `/")
                && message.ends_with(&format!("kit/formulas/{script}` is not an executable file"))
                && !message.contains('\n'),
            "{message}"
        );
    }
}

/// A formula step names a formula of the project whose required inputs it gives or inherits; a formula that runs
/// itself through such steps and a trigger on an internal formula are errors (spec §9.1).
#[test]
fn a_formula_step_names_a_formula_whose_inputs_it_has() {
    const SUB: &str = "kit/formulas/sub.toml";
    let sub = |inputs: &str, steps: &str| format!("name = \"sub\"\ninternal = true\n[inputs]\n{inputs}\n{steps}");
    let flow = |inputs: &str, given: &str| {
        format!(
            "name = \"flow\"\n[inputs]\n{inputs}\n[[step]]\nname = \"inner\"\nkind = \"formula\"\nformula = \"sub\"\n\
             inputs = {{ {given} }}\n"
        )
    };
    let goal = "goal = { type = \"string\", required = true }";
    let config = load_with(&[(FLOW, &flow(goal, "")), (SUB, &sub(goal, &exec(None)))]).unwrap();
    let pack = &config.projects[0].packs[1];
    assert!(pack.formulas["sub"].internal);
    let step = &pack.formulas["flow"].steps[0];
    let crate::config::StepKind::Formula { formula, .. } = &step.kind else {
        panic!("{step:?}");
    };
    assert_eq!((formula.to_string().as_str(), step.kind.name()), ("kit.sub", "formula"));
    let given = load_with(&[
        (FLOW, &flow("", "goal = \"{{ run.id }}\"")),
        (SUB, &sub(goal, &exec(None))),
    ]);
    assert!(given.is_ok(), "{given:?}");
    fails(
        &[(FLOW, &flow("", "")), (SUB, &sub(goal, &exec(None)))],
        "`kit.sub` needs input `goal`, which the step neither gives nor inherits",
    );
    fails(
        &[(FLOW, &flow(goal, "why = \"x\"")), (SUB, &sub(goal, &exec(None)))],
        "`kit.sub` has no input `why`",
    );
    fails(
        &[
            (FLOW, &flow(goal, "goal = \"{{ inputs.nope }}\"")),
            (SUB, &sub(goal, &exec(None))),
        ],
        "names nothing a run has",
    );
    // A task is inherited as any input: the child works on the bead its parent holds (spec §9.1).
    let task = "goal = { type = \"task\", required = true }";
    let inherited = load_with(&[(FLOW, &flow(task, "")), (SUB, &sub(task, &exec(None)))]);
    assert!(inherited.is_ok(), "{inherited:?}");
    let back = "[[step]]\nkind = \"formula\"\nformula = \"flow\"\n";
    fails(
        &[(FLOW, &flow(goal, "")), (SUB, &sub(goal, back))],
        "runs itself through its `formula` steps",
    );
    let trigger = (
        "kit/pack.toml",
        "name = \"kit\"\nversion = \"0.3.0\"\nrequires_plugins = [\"mail\"]\n[[trigger]]\nname = \"nightly\"\n\
         cron = \"0 3 * * *\"\nrun = \"sub\"\ninputs = { goal = \"g\" }\n",
    );
    fails(
        &[(FLOW, &flow(goal, "")), (SUB, &sub(goal, &exec(None))), trigger],
        "formula `kit.sub` is internal",
    );
}
