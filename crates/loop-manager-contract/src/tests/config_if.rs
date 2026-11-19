//! The `if` of a step (docs/design/config.md "Formula").

use super::config::{fails, load_with};
use serde_json::json;

const FLOW: &str = "kit/formulas/flow.toml";
const SUB: &str = "kit/formulas/sub.toml";
/// The agent of the `ai` steps of `FLOW`, with a template of its own: an agent's template fits every formula naming it.
const WORKER: (&str, &str) = ("kit/agents/worker/agent.toml", "harness = \"claude\"\n");
const WORKER_TEMPLATE: &str = "kit/agents/worker/prompt.template.md";

fn step(name: &str, kind: &str, keys: &str) -> String {
    format!("[[step]]\nname = \"{name}\"\nkind = \"{kind}\"\n{keys}\n")
}

#[test]
fn a_step_of_any_kind_takes_an_if_table() {
    let when = "if = { back_from = \"t\" }";
    let flow = [
        "name = \"flow\"\n[inputs]\nroot = { type = \"task\", required = true }\n".to_string(),
        step("a", "exec", "command = [\"true\"]"),
        step("t", "exec", "command = [\"true\"]\nback_to = \"a\""),
        step("w", "ai", &format!("agent = \"worker\"\n{when}")),
        step("e", "exec", &format!("command = [\"true\"]\n{when}")),
        step("d", "drain", &format!("formula = \"implement\"\n{when}")),
        step("f", "formula", &format!("formula = \"sub\"\n{when}")),
        step("g", "await", when),
    ]
    .concat();
    let sub = format!(
        "name = \"sub\"\ninternal = true\n{}",
        step("x", "exec", "command = [\"true\"]")
    );
    let config = load_with(&[(FLOW, &flow), (SUB, &sub), WORKER, (WORKER_TEMPLATE, "Work.")]).unwrap();
    let steps = &config.projects[0].packs[1].formulas["flow"].steps;
    assert!(steps[..2].iter().all(|s| s.condition.is_empty()), "{steps:?}");
    for s in &steps[2..] {
        assert!(s.condition.matches(&json!({ "back_from": ["t"] })), "{s:?}");
        assert!(!s.condition.matches(&json!({ "back_from": [] })), "{s:?}");
    }
    let kinds: Vec<&str> = steps[2..].iter().map(|s| s.kind.name()).collect();
    assert_eq!(kinds, ["ai", "exec", "drain", "formula", "await"]);
}

/// Every path of an `if` names what a run has before the step, or `back_from`, whose values name checks.
#[test]
fn the_paths_of_an_if_are_checked_at_load() {
    let flow = |when: &str| {
        [
            "name = \"flow\"\n[inputs]\ndeploy = { type = \"boolean\" }\n".to_string(),
            step("pre", "exec", "command = [\"true\"]"),
            step("t", "exec", "command = [\"true\"]\nback_to = \"pre\""),
            step(
                "cond",
                "exec",
                "command = [\"true\"]\nif = { \"inputs.deploy\" = true }",
            ),
            step("s", "exec", &format!("command = [\"true\"]\nif = {{ {when} }}")),
            step("later", "exec", "command = [\"true\"]"),
        ]
        .concat()
    };
    let no = |path: &str| format!("step `s`: `if`: a run has no `{path}`");
    for (when, error) in [
        ("\"inputs.nope\" = 1", no("inputs.nope")),
        ("\"steps.later.result\" = \"pass\"", no("steps.later.result")),
        ("\"steps.pre.nope\" = 1", no("steps.pre.nope")),
        ("remarks = \"x\"", no("remarks")),
        ("\"agent.x\" = \"y\"", no("agent.x")),
        ("\"bead.title\" = \"x\"", no("bead.title")),
        ("run = \"x\"", no("run")),
        ("\"nowhere.x\" = 1", no("nowhere.x")),
        ("\"back_from.x\" = \"t\"", no("back_from.x")),
        (
            "inputs.deploy = true",
            "step `s`: `if`: `inputs`: a matcher is a string".to_string(),
        ),
        (
            "back_from = \"nope\"",
            "step `s`: `if` waits for `back_from` \"nope\", which is no check of the formula".to_string(),
        ),
    ] {
        fails(&[(FLOW, &flow(when))], &error);
    }
    for when in [
        "\"inputs.deploy\" = true",
        "\"run.formula\" = \"flow\"",
        "\"rig.name\" = \"x\"",
        "\"project.name\" = \"shop\"",
        "\"steps.pre.data.exit_code\" = 0",
        "back_from = [\"t\"]",
        // A step with an `if` of its own: a skipped step has no outcome, so the entry does not match.
        "\"steps.cond.result\" = \"pass\"",
    ] {
        let loaded = load_with(&[(FLOW, &flow(when))]);
        assert!(loaded.is_ok(), "{when}: {loaded:?}");
    }
    // The comments and the parents of the bead are for a prompt only.
    let tasked = |when: &str| {
        let head = "name = \"flow\"\n[inputs]\nbead = { type = \"task\" }\n".to_string();
        head + &step("s", "exec", &format!("command = [\"true\"]\nif = {{ {when} }}"))
    };
    assert!(load_with(&[(FLOW, &tasked("\"bead.title\" = \"x\""))]).is_ok());
    fails(
        &[(FLOW, &tasked("\"bead.comments\" = \"x\""))],
        "`if`: a run has no `bead.comments`",
    );
}

/// A template or the `inputs` of a `formula` step need a value, which a step with an `if` may never leave.
#[test]
fn a_template_or_step_inputs_cannot_name_a_step_with_an_if() {
    let pre = |when: &str| step("pre", "exec", &format!("command = [\"true\"]\n{when}"));
    let when = "if = { \"inputs.deploy\" = true }";
    let head = "name = \"flow\"\n[inputs]\ndeploy = { type = \"boolean\" }\n";
    let ai = |when: &str| format!("{head}{}{}", pre(when), step("w", "ai", "agent = \"worker\""));
    let template = (WORKER_TEMPLATE, "Fix {{ steps.pre.summary }}.");
    fails(
        &[(FLOW, &ai(when)), WORKER, template],
        "`{{ steps.pre.summary }}` is not a value of the runs of `kit.flow`",
    );
    let control = load_with(&[(FLOW, &ai("")), WORKER, template]);
    assert!(control.is_ok(), "{control:?}");

    let sub = format!(
        "name = \"sub\"\ninternal = true\n[inputs]\nx = {{ type = \"string\", required = true }}\n{}",
        step("x", "exec", "command = [\"true\"]")
    );
    let given = "formula = \"sub\"\ninputs = { x = \"{{ steps.pre.summary }}\" }";
    let formula = |when: &str| format!("{head}{}{}", pre(when), step("inner", "formula", given));
    fails(&[(FLOW, &formula(when)), (SUB, &sub)], "names nothing a run has");
    let control = load_with(&[(FLOW, &formula("")), (SUB, &sub)]);
    assert!(control.is_ok(), "{control:?}");
}
