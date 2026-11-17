//! The configs of the agents a session of a run may call, carried by the run's config (docs/design/config.md
//! "Subagents"), and the clashes of what the session and its targets share.

use super::{
    config::load_with,
    config_subagents::{FIT, ME, QA, base, me, one_error},
};
use crate::config::{EffectiveConfig, Name};

fn qa() -> Name {
    Name {
        pack: "kit".into(),
        name: "qa".into(),
    }
}

#[test]
fn a_run_carries_the_configs_of_the_agents_it_may_call() {
    let me_qa = me("subagents = [\"qa\"]");
    let qa_low = format!("{FIT}effort = \"low\"\n");
    let config = load_with(&[(ME, &me_qa), (QA, &qa_low)]).unwrap();
    let (_, run) = config.for_run("shop", "backend", "kit.implement", "1").unwrap();
    let keys: Vec<&String> = run.targets.keys().collect();
    // The caller has no `description`: no session calls it.
    assert_eq!(keys, ["kit.qa"]);
    // A target is alone at the run's place: the rig's edit of the caller does not reach it.
    assert_eq!(
        run.targets["kit.qa"],
        config.effective("shop", Some("backend"), &qa()).unwrap()
    );
    assert_eq!(run.model.as_ref().unwrap().value, "opus");
    assert_ne!(run.targets["kit.qa"].model, run.model);

    // A caller with a `description` is a target of its own, with the run's config, the step's layer in it.
    let me_described = me("subagents = [\"qa\"]\ndescription = \"Implements.\"");
    let formula = base("kit/formulas/implement.toml").replace("timeout", "effort = \"low\"\ntimeout");
    let formula = ("kit/formulas/implement.toml", formula.as_str());
    let config = load_with(&[(ME, &me_described), (QA, FIT), formula]).unwrap();
    let (_, run) = config.for_run("shop", "backend", "kit.implement", "1").unwrap();
    assert_eq!(run.effort.as_ref().unwrap().value, "low");
    let mut own = run.clone();
    own.targets.clear();
    assert_eq!(run.targets["kit.implementer"], own);
    assert_eq!(run.targets.len(), 2);

    // No list and no `description`: nothing to call.
    let config = load_with(&[]).unwrap();
    let (_, run) = config.for_run("shop", "backend", "kit.implement", "1").unwrap();
    assert!(run.targets.is_empty());
    // A snapshot taken before the field reads as none.
    let mut old = serde_json::to_value(&run).unwrap();
    old.as_object_mut().unwrap().remove("targets");
    let old: EffectiveConfig = serde_json::from_value(old).unwrap();
    assert_eq!(old, run);
}

#[test]
fn a_target_mcp_server_of_the_callers_name_is_the_callers() {
    let me_db = me("subagents = [\"qa\"]\n[mcp.db]\ncommand = [\"db\"]");
    let qa_db = |command: &str| format!("{FIT}[mcp.db]\ncommand = [\"{command}\"]\n");
    one_error(
        &[(ME, &me_db), (QA, &qa_db("other-db"))],
        "agent `kit.implementer` on rig `root`: subagent `kit.qa`: its `mcp.db` is not the caller's",
    );
    load_with(&[(ME, &me_db), (QA, &qa_db("db"))]).unwrap();
}

#[test]
fn a_skill_name_has_one_directory_in_a_session() {
    // The caller has `mail` of the plugin `mail`; a target with a `mail` of its own clashes.
    let me_list = me("subagents = [\"qa\", \"docs\"]");
    let skill = |name: &str| format!("{FIT}[skills.{name}]\npath = \"{name}\"\n");
    let qa_mail = skill("mail");
    let docs = ("kit/agents/docs/agent.toml", FIT);
    let qa_skill = ("kit/agents/qa/mail/SKILL.md", "mail");
    one_error(
        &[(ME, &me_list), (QA, &qa_mail), qa_skill, docs],
        "subagent `kit.qa`: its skill `mail` is not the one of the session",
    );
    // Two targets, each with a `notes` of its own.
    let (qa_notes, docs_notes) = (skill("notes"), skill("notes"));
    let files = [
        (ME, me_list.as_str()),
        (QA, &qa_notes),
        ("kit/agents/qa/notes/SKILL.md", "notes"),
        ("kit/agents/docs/agent.toml", &docs_notes),
        ("kit/agents/docs/notes/SKILL.md", "notes"),
    ];
    one_error(
        &files,
        "subagent `kit.qa`: its skill `notes` is not the one of the session",
    );
    // The same directory is no clash: both attach the plugin `mail`.
    let qa_plugin = format!("{FIT}plugins = [{{ name = \"mail\" }}]\n");
    load_with(&[(ME, &me_list), (QA, &qa_plugin), docs]).unwrap();
}
