//! An agent's prompt template lives in its directory, and the project overrides it with files of its own
//! (docs/design/config.md, criterion 33).

use super::config::{BASE, TEMPLATE, fails, load_with};
use crate::config::{Name, Prompt};
use serde_json::json;

const OWN: &str = "shop/.loop-manager/agents/implementer/prompt.template.md";
const CHECKS: &str = "shop/.loop-manager/agents/implementer/checks.md";

fn prompt(changes: &[(&str, &str)]) -> Prompt {
    let config = load_with(changes).unwrap();
    let name = Name {
        pack: "kit".into(),
        name: "implementer".into(),
    };
    config.projects[0].prompts[&name].clone()
}

#[test]
fn a_project_template_replaces_the_packs_and_a_file_of_the_project_fills_it() {
    let pack = prompt(&[]);
    assert!(pack.from.ends_with(TEMPLATE), "{}", pack.from.display());
    let own = prompt(&[
        (OWN, "Check {{ bead.title }}:\n{{ agent.checks }}"),
        (CHECKS, "- tests pass\n"),
    ]);
    assert!(own.from.ends_with(OWN), "{}", own.from.display());
    let text = own
        .template
        .render(&json!({ "bead": { "title": "T" }, "agent": own.files }))
        .unwrap();
    assert_eq!(text, "Check T:\n- tests pass\n");
    // The pack's file fills the template when the project has none; the project's is stronger.
    let packs = ("kit/agents/implementer/checks.md", "- the pack's checks\n");
    let used = |extra: &[(&str, &str)]| {
        let mut changes = vec![(TEMPLATE, "{{ agent.checks }}"), packs];
        changes.extend_from_slice(extra);
        prompt(&changes).files["checks"].clone()
    };
    assert_eq!(used(&[]), "- the pack's checks\n");
    assert_eq!(used(&[(CHECKS, "- the project's\n")]), "- the project's\n");
}

#[test]
fn a_missing_template_or_named_file_is_an_error_that_names_where_it_goes() {
    // An agent no step names needs no template.
    let idle = ("kit/agents/reviewer/agent.toml", "harness = \"claude\"\n");
    assert!(load_with(&[idle]).is_ok());
    let no_template = load_with(&[(TEMPLATE, "")]).unwrap_err();
    assert!(
        no_template.iter().any(|e| e.message.contains("is empty")),
        "{no_template:?}"
    );
    fails(
        &[(OWN, "{{ agent.checks }}")],
        "`{{ agent.checks }}`: put `checks.md` in `",
    );
    fails(&[(OWN, "{{ agent.checks }}")], "shop/.loop-manager/agents/implementer`");
    fails(&[(OWN, "{{ agent.a.b }}")], "one file name after `agent.`");
    fails(&[(OWN, "{{ agent }}")], "is not a value of the runs of `kit.implement`");
    fails(
        &[("shop/.loop-manager/agents/nobody/prompt.template.md", "x")],
        "no pack of project `shop` has an agent `nobody`",
    );
}

#[test]
fn an_agent_named_by_a_step_without_a_template_is_an_error_naming_its_path() {
    let reviewer = ("kit/agents/reviewer/agent.toml", "harness = \"claude\"\n");
    let review = (
        "kit/formulas/review.toml",
        "name = \"review\"\n[[step]]\nkind = \"ai\"\nagent = \"reviewer\"\n",
    );
    let errors = load_with(&[reviewer, review]).unwrap_err();
    let needle = "agent `kit.reviewer` has no prompt template: put it in `";
    let error = errors
        .iter()
        .find(|e| e.message.contains(needle))
        .unwrap_or_else(|| panic!("{errors:?}"));
    assert!(
        error.message.ends_with("kit/agents/reviewer/prompt.template.md`"),
        "{}",
        error.message
    );
    assert!(error.file.ends_with("kit/agents/reviewer/agent.toml"));
}

#[test]
fn a_template_fits_every_formula_whose_steps_name_its_agent() {
    let review = (
        "kit/formulas/review.toml",
        "name = \"review\"\n[[step]]\nkind = \"ai\"\nagent = \"implementer\"\n",
    );
    fails(
        &[review],
        "`{{ bead.title }}` is not a value of the runs of `kit.review`",
    );
    let plain = (TEMPLATE, "Implement {{ inputs.bead }} in {{ rig.path }}.");
    fails(
        &[review, plain],
        "`{{ inputs.bead }}` is not a value of the runs of `kit.review`",
    );
    assert!(load_with(&[review, (TEMPLATE, "Work in {{ rig.path }}.")]).is_ok());
}

#[test]
fn a_variant_runs_on_the_template_of_its_base_agent() {
    let project = format!(
        "{}[agents.quick]\nbase = \"kit.implementer\"\n",
        super::config::BASE[3].1
    );
    let config = load_with(&[("shop/loop-manager.toml", &project)]).unwrap();
    let prompt = config.prompt("shop", "quick").unwrap();
    assert!(prompt.from.ends_with(TEMPLATE), "{}", prompt.from.display());
    assert!(config.prompt("shop", "nobody").is_err());
}

#[test]
fn a_prompt_template_may_use_only_values_its_runs_have() {
    let template = TEMPLATE;
    fails(&[(template, "{{ inputs.nope }}")], "`{{ inputs.nope }}`");
    fails(&[(template, "{{ bead.nope }}")], "`{{ bead.nope }}`");
    fails(&[(template, "{{ run.nope }}")], "`{{ run.nope }}`");
    fails(&[(template, "{{ project.nope }}")], "`{{ project.nope }}`");
    fails(&[(template, "{{ secrets.token }}")], "starts with none of");
    fails(&[(template, "{{ inputs.bead")], "unclosed");
    let no_bead = BASE[8].1.replace(r#"type = "task""#, r#"type = "string""#);
    fails(
        &[("kit/formulas/implement.toml", no_bead.as_str())],
        "`{{ bead.title }}`",
    );
}

#[test]
fn a_prompt_shows_the_comments_and_the_parents_of_its_bead_and_no_field_below_them() {
    assert!(prompt(&[(OWN, "{{ bead.comments }}\n{{ bead.parents }}")]).shows_lineage());
    assert!(!prompt(&[]).shows_lineage());
    for path in ["bead.comment", "bead.comments.text", "bead.parents.title"] {
        let template = format!("{{{{ {path} }}}}");
        let error = format!("`{{{{ {path} }}}}` is not a value of the runs of `kit.implement`");
        fails(&[(OWN, template.as_str())], &error);
    }
    // A formula without a task input has no bead.
    let review = (
        "kit/formulas/review.toml",
        "name = \"review\"\n[[step]]\nkind = \"ai\"\nagent = \"implementer\"\n",
    );
    fails(
        &[review, (TEMPLATE, "{{ bead.comments }}")],
        "`{{ bead.comments }}` is not a value of the runs of `kit.review`",
    );
}

#[test]
fn last_comments_is_a_positive_count_on_an_ai_step() {
    use crate::config::StepKind;
    let implement = |keys: &str| {
        let text = format!(
            "name = \"implement\"\n[inputs]\nbead = {{ type = \"task\", required = true }}\n[[step]]\n{keys}\n"
        );
        ("kit/formulas/implement.toml", text)
    };
    let (file, text) = implement("kind = \"ai\"\nagent = \"implementer\"\nlast_comments = 2");
    let config = load_with(&[(file, text.as_str())]).unwrap();
    let (_, formula) = config.projects[0].formula("implement").unwrap();
    let StepKind::Ai { last_comments, .. } = &formula.steps[0].kind else {
        panic!("an ai step");
    };
    assert_eq!(last_comments.map(std::num::NonZeroUsize::get), Some(2));
    let (file, zero) = implement("kind = \"ai\"\nagent = \"implementer\"\nlast_comments = 0");
    fails(&[(file, zero.as_str())], "expected a nonzero usize");
    let (file, exec) = implement("kind = \"exec\"\ncommand = [\"true\"]\nlast_comments = 2");
    fails(&[(file, exec.as_str())], "unknown field `last_comments`");
}
