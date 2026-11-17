//! `subagents` and `description` of an agent: the list resolved on every place, and the targets checked against the
//! caller at load (docs/design/config.md "Subagents").

use super::{
    config::{BASE, load_for_adapters, load_with},
    config_launch::ADAPTERS,
};
use crate::config::{Config, ConfigError, Layer, Name};

pub(super) const ME: &str = "kit/agents/implementer/agent.toml";
pub(super) const QA: &str = "kit/agents/qa/agent.toml";
pub(super) const FIT: &str = "harness = \"claude\"\ndescription = \"Checks a change.\"\n";

/// The `agent.toml` of BASE's `implementer` with `extra` after it.
pub(super) fn me(extra: &str) -> String {
    format!("harness = \"claude\"\neffort = \"high\"\naddons = [{{ name = \"mail\" }}]\n{extra}\n")
}

pub(super) fn base(path: &str) -> &'static str {
    BASE.iter().find(|(p, _)| *p == path).unwrap().1
}

/// BASE's `shop/aae.toml` with `extra` after its first line, a second pack `other` with agents `qa` and `docs`.
fn shop(extra: &str) -> String {
    base("shop/aae.toml").replacen(
        "packs = [\"../kit\"]\n",
        &format!("packs = [\"../kit\", \"../other\"]\n{extra}\n"),
        1,
    )
}

const OTHER: [(&str, &str); 3] = [
    ("other/pack.toml", "name = \"other\"\nversion = \"0.1.0\"\n"),
    ("other/agents/qa/agent.toml", FIT),
    ("other/agents/docs/agent.toml", FIT),
];

/// The resolved list of `implementer` alone at `rig`, with its layer.
fn list(config: &Config, rig: Option<&str>) -> Option<(Vec<String>, Layer)> {
    let me = Name {
        pack: "kit".into(),
        name: "implementer".into(),
    };
    let e = config.effective("shop", rig, &me).unwrap();
    e.subagents.map(|s| (s.value, s.from))
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

fn errors(changes: &[(&str, &str)]) -> Vec<ConfigError> {
    load_with(changes).expect_err("the config must be rejected")
}

/// Loading fails with exactly one error that contains `needle`.
pub(super) fn one_error(changes: &[(&str, &str)], needle: &str) {
    let errors = errors(changes);
    let found: Vec<_> = errors.iter().filter(|e| e.message.contains(needle)).collect();
    assert_eq!(found.len(), 1, "`{needle}` in {errors:?}");
}

#[test]
fn no_list_means_only_the_agent_itself() {
    let config = load_with(&[]).unwrap();
    assert_eq!(list(&config, None), None);
    let explained = config
        .effective(
            "shop",
            None,
            &Name {
                pack: "kit".into(),
                name: "implementer".into(),
            },
        )
        .unwrap()
        .explain();
    assert!(!explained.contains("subagents"), "{explained}");
    let config = load_with(&[(ME, &me("subagents = []"))]).unwrap();
    assert_eq!(list(&config, None), Some((strings(&["kit.implementer"]), Layer::Agent)));
}

#[test]
fn a_name_is_bare_or_qualified() {
    let shop = shop("");
    let mut changes = vec![(ME, me("subagents = [\"qa\", \"other.docs\"]")), (QA, FIT.into())];
    changes.push(("shop/aae.toml", shop));
    let changes: Vec<(&str, &str)> = (changes.iter().map(|(p, t)| (*p, t.as_str()))).chain(OTHER).collect();
    let config = load_with(&changes).unwrap();
    // The bare `qa` is the caller's own pack's, though `other` has one too.
    let want = strings(&["kit.implementer", "kit.qa", "other.docs"]);
    assert_eq!(list(&config, None), Some((want, Layer::Agent)));
}

#[test]
fn a_pattern_matches_agents_and_variants() {
    let variant = "[agents.qa-fast]\nbase = \"kit.qa\"\nmodel = \"haiku\"";
    let shop = shop(variant);
    let with = |patterns: &str| {
        let mut changes = vec![(ME, me(&format!("subagents = {patterns}"))), (QA, FIT.into())];
        changes.push(("kit/agents/qa-deep/agent.toml", FIT.into()));
        changes.push(("shop/aae.toml", shop.clone()));
        let changes: Vec<(&str, &str)> = (changes.iter().map(|(p, t)| (*p, t.as_str()))).chain(OTHER).collect();
        list(&load_with(&changes).unwrap(), None).unwrap().0
    };
    let me = "kit.implementer";
    assert_eq!(
        with("[\"qa*\"]"),
        strings(&[me, "kit.qa", "kit.qa-deep", "other.qa", "qa-fast"])
    );
    // With a pack, only that pack's agents, no variant.
    assert_eq!(with("[\"kit.q*\"]"), strings(&[me, "kit.qa", "kit.qa-deep"]));
    assert_eq!(with("[\"*-fast\", \"q*p\"]"), strings(&[me, "kit.qa-deep", "qa-fast"]));
    assert_eq!(with("[\"*.d*\"]"), strings(&[me, "other.docs"]));
    // A pattern that matches nothing is no error.
    assert_eq!(with("[\"zz-*\"]"), strings(&[me]));
}

#[test]
fn the_strongest_layer_replaces_the_list() {
    let docs = ("kit/agents/docs/agent.toml", FIT);
    let shop = shop("[agents.implementer]\nsubagents = [\"docs\"]").replace("\"opus\"\n", "\"opus\"\nsubagents = []\n");
    let me_qa = me("subagents = [\"qa\"]");
    let changes = [(ME, me_qa.as_str()), (QA, FIT), docs, ("shop/aae.toml", shop.as_str())];
    let changes: Vec<(&str, &str)> = changes.into_iter().chain(OTHER).collect();
    let config = load_with(&changes).unwrap();
    let want = strings(&["kit.docs", "kit.implementer"]);
    assert_eq!(list(&config, None), Some((want, Layer::ProjectAgent)));
    let on_rig = Some((strings(&["kit.implementer"]), Layer::RigAgent));
    assert_eq!(list(&config, Some("backend")), on_rig);
}

#[test]
fn a_variant_has_a_list_of_its_own() {
    let variant = "[agents.impl-fast]\nbase = \"kit.implementer\"\nsubagents = [\"qa\"]";
    let shop = shop(&format!(
        "{variant}\n[rigs.backend.agents]\nimplementer = \"impl-fast\""
    ))
    .replace("[rigs.backend.agents.implementer]\nmodel = \"opus\"\n", "");
    let me_docs = me("subagents = [\"docs\"]");
    let changes = [
        (ME, me_docs.as_str()),
        (QA, FIT),
        ("kit/agents/docs/agent.toml", FIT),
        ("shop/aae.toml", &shop),
    ];
    let changes: Vec<(&str, &str)> = changes.into_iter().chain(OTHER).collect();
    let config = load_with(&changes).unwrap();
    let (agent, e) = config.for_run("shop", "backend", "kit.implement", "1").unwrap();
    assert_eq!(agent, "impl-fast");
    let s = e.subagents.unwrap();
    assert_eq!(
        (s.value, s.from),
        (strings(&["impl-fast", "kit.qa"]), Layer::Variant("impl-fast".into()))
    );
}

#[test]
fn a_rig_puts_its_variant_in_place_of_a_named_agent() {
    let variant =
        "[agents.qa-opus]\nbase = \"kit.qa\"\nmodel = \"opus\"\n[agents.qa-low]\nbase = \"kit.qa\"\neffort = \"low\"";
    let shop = shop(variant).replace(
        "[rigs.backend.agents.implementer]",
        "[rigs.backend.agents]\n\"kit.qa\" = \"qa-opus\"\n[rigs.backend.agents.implementer]",
    );
    let me_qa = me("subagents = [\"qa\", \"qa-low\"]");
    let changes = [(ME, me_qa.as_str()), (QA, FIT), ("shop/aae.toml", &shop)];
    let changes: Vec<(&str, &str)> = changes.into_iter().chain(OTHER).collect();
    let config = load_with(&changes).unwrap();
    let on = |rig| list(&config, rig).unwrap().0;
    assert_eq!(on(Some("backend")), strings(&["kit.implementer", "qa-low", "qa-opus"]));
    assert_eq!(on(None), strings(&["kit.implementer", "kit.qa", "qa-low"]));
    // An agent a pattern matches is mapped as well.
    let me_qa = me("subagents = [\"kit.qa*\"]");
    let changes = [(ME, me_qa.as_str()), (QA, FIT), ("shop/aae.toml", &shop)];
    let changes: Vec<(&str, &str)> = changes.into_iter().chain(OTHER).collect();
    let config = load_with(&changes).unwrap();
    let on = |rig| list(&config, rig).unwrap().0;
    assert_eq!(on(Some("backend")), strings(&["kit.implementer", "qa-opus"]));
    assert_eq!(on(None), strings(&["kit.implementer", "kit.qa"]));
}

#[test]
fn a_pattern_skips_an_orchestrator_variant_of_another_project() {
    let config = base("config.toml").replace("[\"shop\"]", "[\"shop\", \"farm\"]")
        + "[agents.tester-fast]\nbase = \"lab.tester\"\neffort = \"low\"\n";
    let me_all = me("subagents = [\"*\"]");
    let changes = [
        ("config.toml", config.as_str()),
        (ME, &me_all),
        ("farm/aae.toml", "packs = [\"../lab\"]\n"),
        ("lab/pack.toml", "name = \"lab\"\nversion = \"0.1.0\"\n"),
        ("lab/agents/tester/agent.toml", FIT),
    ];
    let config = load_with(&changes).unwrap();
    assert_eq!(list(&config, None), Some((strings(&["kit.implementer"]), Layer::Agent)));
}

#[test]
fn explain_prints_the_resolved_list() {
    let shop = shop("[agents.\"kit.qa\"]\ndescription = \"Checks a change, edited.\"");
    let me_qa = me("subagents = [\"kit.qa\"]\ndescription = \"Implements.\"");
    let changes = [(ME, me_qa.as_str()), (QA, FIT), ("shop/aae.toml", &shop)];
    let changes: Vec<(&str, &str)> = changes.into_iter().chain(OTHER).collect();
    let config = load_with(&changes).unwrap();
    let of = |name: &str| {
        config
            .effective(
                "shop",
                None,
                &Name {
                    pack: "kit".into(),
                    name: name.into(),
                },
            )
            .unwrap()
            .explain()
    };
    let text = of("implementer");
    assert!(
        text.contains("subagents = [\"kit.implementer\",\"kit.qa\"]  # agent\n"),
        "{text}"
    );
    assert!(text.contains("description = \"Implements.\"  # agent\n"), "{text}");
    let text = of("qa");
    assert!(
        text.contains("description = \"Checks a change, edited.\"  # project [agents]\n"),
        "{text}"
    );
}

#[test]
fn a_name_nobody_has_is_an_error() {
    let me_nobody = me("subagents = [\"nobody\"]");
    // Said once, of the place it is found on first, not once per place.
    one_error(
        &[(ME, &me_nobody)],
        "agent `kit.implementer` on rig `root`: `subagents`: no agent `nobody`",
    );
    let shop = shop("");
    let me_qa = me("subagents = [\"other.qa\", \"kit.docs\"]");
    let changes = [(ME, me_qa.as_str()), (QA, FIT), ("shop/aae.toml", &shop)];
    let changes: Vec<(&str, &str)> = changes.into_iter().chain(OTHER).collect();
    one_error(&changes, "`subagents`: no agent `kit.docs`");
}

#[test]
fn a_target_of_another_harness_is_an_error() {
    let me_qa = me("subagents = [\"qa\"]");
    let codex = FIT.replace("claude", "codex");
    let harnesses = format!("{}[harness.codex]\n", base("config.toml"));
    let harnesses = ("config.toml", harnesses.as_str());
    one_error(
        &[harnesses, (ME, &me_qa), (QA, &codex)],
        "agent `kit.implementer` on rig `root`: subagent `kit.qa`: it runs on harness `codex`, the caller on `claude`",
    );
    // A step of a formula that runs the caller on another harness than its targets'.
    let formula = base("kit/formulas/implement.toml").replace("timeout", "harness = \"codex\"\ntimeout");
    one_error(
        &[
            harnesses,
            (ME, &me_qa),
            (QA, FIT),
            ("kit/formulas/implement.toml", &formula),
        ],
        "formula `kit.implement` on rig `backend`: subagent `kit.qa`: it runs on harness `claude`, the caller on `codex`",
    );
}

#[test]
fn a_target_of_another_scope_or_host_is_an_error() {
    let me_qa = me("subagents = [\"qa\"]");
    let root = format!("{FIT}scope = \"root\"\n");
    one_error(
        &[(ME, &me_qa), (QA, &root)],
        "subagent `kit.qa`: its `scope` is not the caller's",
    );
    let off = format!("{FIT}inherit_host = false\n");
    one_error(
        &[(ME, &me_qa), (QA, &off)],
        "subagent `kit.qa`: its `inherit_host` is not the caller's",
    );
    let me_off = me("subagents = [\"qa\"]\ninherit_host = false");
    load_with(&[(ME, &me_off), (QA, &off)]).unwrap();
}

#[test]
fn an_env_key_with_two_values_is_an_error() {
    let me_qa = me("subagents = [\"qa\"]");
    let ru = format!("{FIT}env = {{ LANG = \"ru\" }}\n");
    one_error(
        &[(ME, &me_qa), (QA, &ru)],
        "subagent `kit.qa`: its `env.LANG` is not the caller's",
    );
    // A key of the target alone is no clash.
    let own = format!("{FIT}env = {{ QA_ONLY = \"1\" }}\n");
    load_with(&[(ME, &me_qa), (QA, &own)]).unwrap();
}

#[test]
fn a_target_addon_for_another_harness_is_an_error() {
    let mail =
        base("addons/mail/addon.toml").replace("version = \"0.1.0\"", "version = \"0.1.0\"\nharnesses = [\"claude\"]");
    let codex_mail = mail.replace("[\"claude\"]", "[\"codex\"]");
    // The caller drops `mail`; its target attaches it, and `mail` is not for claude.
    let caller = "harness = \"claude\"\nsubagents = [\"qa\"]\n";
    let qa = format!("{FIT}addons = [{{ name = \"mail\" }}]\n");
    one_error(
        &[(ME, caller), (QA, &qa), ("addons/mail/addon.toml", &codex_mail)],
        "subagent `kit.qa`: addon `mail` is for codex, not for harness `claude`",
    );
    load_with(&[(ME, caller), (QA, &qa), ("addons/mail/addon.toml", &mail)]).unwrap();
}

#[test]
fn a_built_in_agent_type_is_no_name_of_a_target() {
    let me_list = me("subagents = [\"Explore\", \"worker\"]");
    let explore = ("kit/agents/Explore/agent.toml", FIT);
    let worker = ("kit/agents/worker/agent.toml", FIT);
    let errors = load_for_adapters(&[(ME, &me_list), explore, worker], ADAPTERS).expect_err("rejected");
    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    // `worker` is built into the other harness only.
    assert_eq!(
        messages,
        [
            "agent `kit.implementer` on rig `root`: subagent `kit.Explore`: `Explore` is a built-in agent type of harness `claude`"
        ]
    );
}

#[test]
fn a_target_needs_a_description() {
    let me_qa = me("subagents = [\"qa\"]");
    let bare = "harness = \"claude\"\n";
    one_error(
        &[(ME, &me_qa), (QA, bare)],
        "subagent `kit.qa`: it has no `description`",
    );
    // An edit gives it one; the caller needs none.
    let shop = base("shop/aae.toml").replacen("\n", "\n[agents.qa]\ndescription = \"Checks.\"\n", 1);
    load_with(&[(ME, &me_qa), (QA, bare), ("shop/aae.toml", &shop)]).unwrap();
}

#[test]
fn the_keys_belong_to_an_agent() {
    let shop = base("shop/aae.toml").replacen("\n", "\n[settings]\nsubagents = [\"*\"]\n", 1);
    let found = errors(&[("shop/aae.toml", &shop)]);
    assert!(
        found.iter().any(|e| e.message.contains("unknown field `subagents`")),
        "{found:?}"
    );
    let me_one = me("subagents = \"qa\"");
    let found = errors(&[(ME, &me_one)]);
    assert!(
        found
            .iter()
            .any(|e| e.message.contains("`subagents` must be an array of strings")),
        "{found:?}"
    );
}
