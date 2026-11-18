//! The project root: its name and path, and the rigs beside it (docs/design/config.md, decision 99).

use super::{
    config::{BASE, fails, load_for_adapters, load_with},
    config_launch::{ADAPTERS, HARNESSES},
};
use crate::config::{Name, Scope};

/// `shop/loop-manager.toml` of `BASE` with `extra` before its rigs.
fn project(extra: &str) -> String {
    BASE[3]
        .1
        .replacen("[rigs.backend]", &format!("{extra}[rigs.backend]"), 1)
}

#[test]
fn a_project_without_a_root_section_has_the_root_next_to_its_file() {
    let config = load_with(&[]).unwrap();
    let shop = &config.projects[0];
    assert_eq!(shop.root.0, "root");
    assert_eq!(shop.root.1.path, shop.dir);
    assert_eq!(shop.place("root").unwrap().path, shop.dir);
    assert_eq!(shop.places().map(|(n, _)| n).collect::<Vec<_>>(), ["root", "backend"]);
}

#[test]
fn the_root_takes_a_name_and_a_path() {
    let text = project("[root]\nname = \"mono\"\npath = \"../work\"\n");
    let config = load_with(&[("shop/loop-manager.toml", &text), ("work/.keep", "")]).unwrap();
    let shop = &config.projects[0];
    assert_eq!(shop.root.0, "mono");
    assert_eq!(shop.root.1.path, shop.dir.parent().unwrap().join("work"));
    assert!(
        shop.dir.ends_with("shop"),
        "the config stays where loop-manager.toml is"
    );
    assert!(shop.place("root").is_none());
    // The root takes the rules of a rig's name and path.
    fails(
        &[("shop/loop-manager.toml", &project("[root]\npath = \"../nowhere\"\n"))],
        "root `root`: ",
    );
    fails(
        &[("shop/loop-manager.toml", &project("[root]\nname = \"a b\"\n"))],
        "root name `a b`",
    );
    fails(
        &[("shop/loop-manager.toml", &project("[root]\nmodel = \"opus\"\n"))],
        "unknown field `model`",
    );
    // Two settings and no more (docs/design/config.md): text for an agent is a file of the agent
    // (docs/design/config.md).
    fails(
        &[(
            "shop/loop-manager.toml",
            &project("[root]\nvars = { checks = \"x\" }\n"),
        )],
        "unknown field `vars`",
    );
}

#[test]
fn a_rig_at_the_root_path_or_under_its_name_is_an_error() {
    let at_root = project("[rigs.top]\npath = \".\"\n");
    fails(&[("shop/loop-manager.toml", &at_root)], "rig `top`: ");
    fails(
        &[("shop/loop-manager.toml", &at_root)],
        "is the project root: drop the rig",
    );
    let named_root = project("[root]\nname = \"backend\"\n");
    fails(
        &[("shop/loop-manager.toml", &named_root)],
        "rig `backend`: the root has this name: rename the rig or set `[root] name`",
    );
}

#[test]
fn a_project_of_one_repository_checks_its_formulas_on_the_root() {
    // No rigs: the runs of the project's formulas still assemble, in the root, and their harness is the build's.
    let text = "packs = [\"../kit\"]\n[agents.implementer]\nharness = \"codex\"\n";
    let config = load_for_adapters(&[HARNESSES, ("shop/loop-manager.toml", text)], ADAPTERS).unwrap();
    assert!(config.projects[0].rigs.is_empty());
    assert!(config.harnesses.contains_key("codex"));
    let (agent, _) = config.for_run("shop", "root", "kit.implement", "1").unwrap();
    assert_eq!(agent, "kit.implementer");
}

#[test]
fn an_agent_of_scope_root_takes_no_rig_layer() {
    let agent = "kit/agents/implementer/agent.toml";
    let rig_wide = BASE[3]
        .1
        .replace("[rigs.backend.agents.implementer]", "[rigs.backend.settings]");
    let rig_agent = load_with(&[("shop/loop-manager.toml", &rig_wide)]).unwrap();
    let (_, e) = rig_agent.for_run("shop", "backend", "kit.implement", "1").unwrap();
    assert_eq!((e.scope, e.model.unwrap().value.as_str()), (Scope::Rig, "opus"));
    let text = format!(
        "scope = \"root\"\n{}",
        BASE.iter().find(|(p, _)| *p == agent).unwrap().1
    );
    let mut config = load_with(&[(agent, &text), ("shop/loop-manager.toml", &rig_wide)]).unwrap();
    config.projects[0].tracker.prefix = Some("PREFIX".into());
    let (_, on_rig) = config.for_run("shop", "backend", "kit.implement", "1").unwrap();
    let (_, in_root) = config.for_run("shop", "root", "kit.implement", "1").unwrap();
    assert_eq!(
        on_rig, in_root,
        "the rig's settings and prefix do not reach an agent of the root"
    );
    assert!(!on_rig.env.contains_key("PREFIX"));
    assert_eq!(on_rig.scope, Scope::Root);
    assert!(on_rig.explain().contains("scope = \"root\"  # agent"));
    let alone = config.effective("shop", Some("backend"), &implementer()).unwrap();
    assert_eq!(alone, config.effective("shop", None, &implementer()).unwrap());
    fails(&[(agent, "scope = \"repo\"\n")], "`scope` is `rig` or `root`");
}

/// A rig that edits an agent of the root, or maps it to a variant, is refused: neither would reach the agent.
#[test]
fn a_rig_neither_edits_nor_maps_an_agent_of_the_root() {
    let agent = "kit/agents/implementer/agent.toml";
    let text = format!(
        "scope = \"root\"\n{}",
        BASE.iter().find(|(p, _)| *p == agent).unwrap().1
    );
    let edit = load_with(&[(agent, &text)]).expect_err("the rig edits an agent of the root");
    assert!(
        edit.iter()
            .any(|e| e.message.contains("agent `implementer` works in the root")),
        "{edit:?}"
    );
    let mapped = BASE[3].1.replace(
        "[rigs.backend.agents.implementer]\nmodel = \"opus\"\n",
        "agents = { implementer = \"fast\" }\n[agents.fast]\nbase = \"kit.implementer\"\n",
    );
    let errors = load_with(&[(agent, &text), ("shop/loop-manager.toml", &mapped)]).expect_err("the rig maps it");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("agent `kit.implementer` works in the root")),
        "{errors:?}"
    );
    let variant = BASE[3]
        .1
        .replace("[rigs.backend.agents.implementer]\n", "[rigs.backend.agents.fast]\n")
        + "[agents.fast]\nbase = \"kit.implementer\"\n";
    let errors =
        load_with(&[(agent, &text), ("shop/loop-manager.toml", &variant)]).expect_err("the rig edits its variant");
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("agent `fast` works in the root")),
        "{errors:?}"
    );
}

fn implementer() -> Name {
    Name {
        pack: "kit".into(),
        name: "implementer".into(),
    }
}

/// A rig's beads carry the acronym of its name, or its `prefix` (docs/design/tracker-br.md); a bead of no rig prefix is
/// the root's.
#[test]
fn a_rig_takes_the_acronym_of_its_name_or_its_prefix_key() {
    let rigs = "[rigs.my-project]\npath = \"mine\"\n[rigs.web_api]\npath = \"web\"\nprefix = \"api\"\n";
    let files = [
        ("shop/loop-manager.toml", project(rigs)),
        ("shop/mine/.keep", String::new()),
        ("shop/web/.keep", String::new()),
    ];
    let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let config = load_with(&files).unwrap();
    let shop = &config.projects[0];
    let prefix = |rig: &str| shop.rigs[rig].prefix.clone();
    assert_eq!(
        (prefix("backend"), prefix("my-project")),
        (Some("b".into()), Some("mp".into()))
    );
    assert_eq!(prefix("web_api").as_deref(), Some("api"));
    assert_eq!(shop.root.1.prefix, None);
    let rig_of = |bead: &str| shop.rig_of(bead);
    assert_eq!(
        [rig_of("api-x1"), rig_of("api-x1.2"), rig_of("mp-q59")],
        [Some("web_api"), Some("web_api"), Some("my-project")]
    );
    // The prefix is all before the last `-`: a root bead of a database prefix `mp-shop` is not `my-project`'s.
    assert_eq!(
        [rig_of("root-79u.1"), rig_of("b"), rig_of("x-b-1"), rig_of("mp-shop-x1")],
        [None; 4]
    );
}

#[test]
fn two_rigs_with_one_prefix_or_a_prefix_br_rewrites_are_an_error() {
    let both = project("[rigs.billing]\npath = \"backend\"\n");
    fails(
        &[("shop/loop-manager.toml", &both)],
        "rigs `backend` and `billing` both give their beads the prefix `b`",
    );
    let upper = project("[rigs.web]\npath = \"backend\"\nprefix = \"API\"\n");
    fails(&[("shop/loop-manager.toml", &upper)], "rig `web`: prefix `API`");
}

#[test]
fn a_rig_takes_max_writers_and_the_root_does_not() {
    let rig = |value: &str| {
        BASE[3].1.replacen(
            "path = \"backend\"\n",
            &format!("path = \"backend\"\nmax_writers = {value}\n"),
            1,
        )
    };
    let config = load_with(&[("shop/loop-manager.toml", &rig("2"))]).unwrap();
    let shop = &config.projects[0];
    assert_eq!(shop.rigs["backend"].max_writers, Some(2));
    assert_eq!(shop.root.1.max_writers, None);
    assert_eq!(load_with(&[]).unwrap().projects[0].rigs["backend"].max_writers, None);
    fails(&[("shop/loop-manager.toml", &rig("0"))], "nonzero");
    fails(
        &[("shop/loop-manager.toml", &project("[root]\nmax_writers = 1\n"))],
        "unknown field `max_writers`",
    );
}
