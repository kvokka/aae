//! The steps of a formula file (spec §9.1, docs/design/config.md "Formula").

use super::config::{fails, load_with};

const FLOW: &str = "kit/formulas/flow.toml";

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
        "name = \"flow\"\n{}[[step]]\nname = \"work\"\nkind = \"prompt\"\nagent = \"implementer\"\n\
         prompt = \"flow.md\"\n{}",
        exec(Some("pre")),
        exec(Some("post"))
    );
    let with = |text: &'static str| load_with(&[(FLOW, &flow), ("kit/formulas/flow.md", text)]);
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
    let bead = "root = { type = \"bead\", required = true }";
    let config = load_with(&[(FLOW, &ship(bead, "implement"))]).unwrap();
    let step = &config.projects[0].packs[1].formulas["flow"].steps[0];
    let crate::config::StepKind::Drain { formula, timeout: None } = &step.kind else {
        panic!("{step:?}");
    };
    assert_eq!(
        (formula.to_string().as_str(), step.kind.name()),
        ("kit.implement", "drain")
    );
    fails(&[(FLOW, &ship(bead, "nothing"))], "step `all`: no formula `nothing`");
    fails(
        &[(FLOW, &ship(bead, "audit"))],
        "`kit.audit` needs exactly one input of type `bead`",
    );
    let two = "name = \"two\"\n[inputs]\nbead = { type = \"bead\" }\nwhy = { type = \"string\", required = true }\n\
               [[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n";
    fails(
        &[(FLOW, &ship(bead, "two")), ("kit/formulas/two.toml", two)],
        "`kit.two` requires input `why`",
    );
    fails(
        &[(FLOW, &ship("root = { type = \"bead\" }", "implement"))],
        "a formula with a `drain` step needs a required input of type `bead`",
    );
}

/// A check names an earlier step to go back to; the formula bounds the rounds (spec §9.1).
#[test]
fn a_check_goes_back_to_an_earlier_step() {
    let back = |to: &str| format!("back_to = \"{to}\"\n");
    let flow = |rounds: &str, to: &str| {
        format!(
            "name = \"flow\"\n{rounds}{}[[step]]\nname = \"work\"\nkind = \"prompt\"\nagent = \"implementer\"\n\
             prompt = \"flow.md\"\n{}{}",
            exec(Some("plan")),
            exec(Some("check")) + &back(to),
            exec(Some("post"))
        )
    };
    let text = ("kit/formulas/flow.md", "Remarks: {{ remarks }}");
    let config = load_with(&[(FLOW, &flow("", "plan")), text]).unwrap();
    let formula = &config.projects[0].packs[1].formulas["flow"];
    assert_eq!(formula.rounds, 3, "the default");
    assert_eq!(formula.steps[2].back_to.as_deref(), Some("plan"));
    let config = load_with(&[(FLOW, &flow("rounds = 5\n", "work")), text]).unwrap();
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
        fails(&[(FLOW, &flow(rounds, to)), text], error);
    }
    let drain = "name = \"flow\"\n[inputs]\nb = { type = \"bead\", required = true }\n[[step]]\nname = \"a\"\n\
                 kind = \"drain\"\nformula = \"implement\"\nback_to = \"a\"\n";
    fails(&[(FLOW, drain)], "unknown field `back_to`");
}
