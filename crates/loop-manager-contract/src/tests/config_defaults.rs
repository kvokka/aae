//! What the orchestrator and the project config give every formula: `rounds` and values of inputs
//! (docs/design/config.md "Formula", decisions 129, 130).

use super::config::{BASE, fails, load_with};
use crate::config::{Config, Layer};
use serde_json::json;

const FLOW: &str = "kit/formulas/flow.toml";
const EXEC: &str = "[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n";
const FLOW_TEXT: &str = "name = \"flow\"\n[inputs]\nfocus = { type = \"string\", required = true }\n\
    n = { type = \"integer\" }\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n";

/// `BASE` with `top` at the top of the orchestrator file, `own` at the top of the project's, and `flow` as
/// `FLOW`.
fn files(top: &str, own: &str, flow: &str) -> Vec<(&'static str, String)> {
    let base = |path: &str| BASE.iter().find(|(p, _)| *p == path).unwrap().1;
    let top = base("config.toml").replacen("[harness.claude]", &format!("{top}[harness.claude]"), 1);
    let own = base("shop/loop-manager.toml").replacen("[rigs.backend]", &format!("{own}[rigs.backend]"), 1);
    vec![
        ("config.toml", top),
        ("shop/loop-manager.toml", own),
        (FLOW, flow.to_owned()),
    ]
}

fn refs<'a>(files: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    files.iter().map(|(p, t)| (*p, t.as_str())).collect()
}

fn load(top: &str, own: &str, flow: &str) -> Config {
    load_with(&refs(&files(top, own, flow))).unwrap()
}

fn rejected(top: &str, own: &str, needle: &str) {
    fails(&refs(&files(top, own, FLOW_TEXT)), needle);
}

/// `rounds`: the formula over the project over the orchestrator over 3.
#[test]
fn rounds_come_from_the_strongest_layer_that_gives_them() {
    let rounds = |top: &str, own: &str, head: &str| {
        let config = load(top, own, &format!("{head}{FLOW_TEXT}"));
        let r = &config.projects[0].packs[1].formulas["flow"].rounds;
        (r.value, r.from.clone())
    };
    assert_eq!(rounds("", "", ""), (3, Layer::Default));
    assert_eq!(rounds("rounds = 4\n", "", ""), (4, Layer::Orchestrator));
    assert_eq!(rounds("rounds = 4\n", "rounds = 5\n", ""), (5, Layer::Project));
    let own = (6, Layer::Formula("flow".into()));
    assert_eq!(rounds("rounds = 4\n", "rounds = 5\n", "rounds = 6\n"), own.clone());
    assert_eq!(rounds("", "", "rounds = 6\n"), own);
    rejected("rounds = 0\n", "", "nonzero");
}

/// `[inputs]`: the project's value over the orchestrator's, each for the formulas that declare the input; text of an
/// integer is one.
#[test]
fn input_values_come_from_the_project_over_the_orchestrator() {
    let top = "[inputs]\nfocus = \"top\"\nn = \"7\"\nelse = 1\n";
    let config = load(top, "", FLOW_TEXT);
    let formulas = &config.projects[0].packs[1].formulas;
    let given = |f: &str, input: &str| {
        formulas[f]
            .defaults
            .get(input)
            .map(|s| (s.value.clone(), s.from.clone()))
    };
    assert_eq!(given("flow", "focus"), Some((json!("top"), Layer::Orchestrator)));
    assert_eq!(given("flow", "n"), Some((json!(7), Layer::Orchestrator)));
    assert_eq!(given("flow", "else"), None, "an input the formula does not declare");
    assert!(
        formulas["audit"].defaults.is_empty(),
        "a formula without the input takes none"
    );
    assert!(
        !formulas["flow"].needs("focus"),
        "a required input with a value counts as given"
    );
    let config = load(top, "[inputs]\nfocus = \"own\"\n", FLOW_TEXT);
    let formulas = &config.projects[0].packs[1].formulas;
    let focus = &formulas["flow"].defaults["focus"];
    assert_eq!((&focus.value, &focus.from), (&json!("own"), &Layer::Project));
    assert_eq!(formulas["flow"].defaults["n"].from, Layer::Orchestrator);
}

/// A value for a `task` input or of the wrong type is an error of the file that gives it.
#[test]
fn a_value_for_a_task_or_of_the_wrong_type_is_an_error() {
    let task = "`inputs.bead` for formula `kit.implement`: a `task` input takes no value from the config";
    rejected("[inputs]\nbead = \"b-1\"\n", "", task);
    rejected(
        "",
        "[inputs]\nn = \"many\"\n",
        "`inputs.n` for formula `kit.flow`: must be an integer, not \"many\"",
    );
    rejected("[inputs]\nfocus = 3\n", "", "must be a string, not 3");
    let errors = load_with(&refs(&files("", "[inputs]\nn = true\n", FLOW_TEXT))).unwrap_err();
    assert!(errors[0].file.ends_with("shop/loop-manager.toml"), "{errors:?}");
}

/// A required input the config gives needs no value from a trigger, a `formula` step or a `drain` step.
#[test]
fn a_required_input_the_config_gives_needs_no_caller() {
    let trigger = "[[trigger]]\nname = \"t\"\ncron = \"0 3 * * *\"\nrun = \"flow\"\n";
    let outer = "name = \"outer\"\n[[step]]\nkind = \"formula\"\nformula = \"flow\"\n";
    let task = "[inputs]\nbead = { type = \"task\", required = true }\n";
    let each = format!("name = \"each\"\n{task}focus = {{ type = \"string\", required = true }}\n{EXEC}");
    let drainer = format!("name = \"drainer\"\n{task}[[step]]\nkind = \"drain\"\nformula = \"each\"\n");
    let mut changes = files("", "", FLOW_TEXT);
    changes[1].1 += trigger;
    changes.push(("kit/formulas/outer.toml", outer.to_owned()));
    changes.push(("kit/formulas/each.toml", each));
    changes.push(("kit/formulas/drainer.toml", drainer));
    fails(&refs(&changes), "required input `focus` of `kit.flow` is not given");
    fails(
        &refs(&changes),
        "needs input `focus`, which the step neither gives nor inherits",
    );
    fails(
        &refs(&changes),
        "requires input `focus` that a drain step does not give",
    );
    changes[0].1 = changes[0]
        .1
        .replacen("[harness.claude]", "[inputs]\nfocus = \"top\"\n[harness.claude]", 1);
    load_with(&refs(&changes)).unwrap();
}
