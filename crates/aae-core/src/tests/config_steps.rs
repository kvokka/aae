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
