//! The launch settings: the harness of the strongest layer, short model names, the pack and formula
//! layers and agent variants (docs/design/config.md).

use super::config::load_for_adapters;
use crate::{
    config::{Config, ConfigError, EffectiveConfig, Layer},
    driver::Adapter,
};

static CLAUDE: Adapter = Adapter {
    name: "claude",
    models: &[],
    tested: [(0, 0, 0), (0, 0, 0)],
    new: |_| Err(String::new()),
    doctor: |_| Vec::new(),
};
static CODEX: Adapter = Adapter {
    name: "codex",
    models: &[("luna", "gpt-6-luna")],
    tested: [(0, 0, 0), (0, 0, 0)],
    new: |_| Err(String::new()),
    doctor: |_| Vec::new(),
};
pub(super) static ADAPTERS: &[&Adapter] = &[&CLAUDE, &CODEX];

const AGENT: &str = "kit/agents/implementer/agent.toml";
pub(super) const HARNESSES: (&str, &str) = (
    "config.toml",
    "projects = [\"shop\"]\n[harness.claude]\nmodel = \"sonnet\"\n[[plugin]]\npath = \"plugins/mail\"\n",
);

fn load(changes: &[(&str, &str)]) -> Result<Config, Vec<ConfigError>> {
    let mut all = vec![HARNESSES];
    all.extend_from_slice(changes);
    load_for_adapters(&all, ADAPTERS)
}

fn run(config: &Config) -> (String, EffectiveConfig) {
    config.for_run("shop", "backend", "kit.implement", "1").unwrap()
}

fn rejected(changes: &[(&str, &str)], needle: &str) {
    let errors = load(changes).expect_err("the config must be rejected");
    assert!(
        errors.iter().any(|e| e.message.contains(needle)),
        "no error mentions `{needle}`: {errors:?}"
    );
}

fn from(layer: Layer, value: &str) -> crate::config::Sourced<String> {
    crate::config::Sourced {
        value: value.to_owned(),
        from: layer,
    }
}

#[test]
fn the_harness_is_the_strongest_layers() {
    let (_, e) = run(&load(&[]).unwrap());
    assert_eq!((e.harness.as_str(), &e.harness_from), ("claude", &Layer::Agent));
    assert_eq!(e.model_key().as_deref(), Some("model:claude/opus"));
    // There is no `provider` setting: the harness's own settings say how it reaches its model (spec §6.2).
    rejected(
        &[(AGENT, "harness = \"claude\"\nprovider = \"openai\"\n")],
        "unknown field `provider`",
    );
}

#[test]
fn an_agents_table_changes_the_harness() {
    let project = "packs = [\"../kit\"]\n[rigs.backend]\npath = \"backend\"\n\
                   [rigs.backend.agents.implementer]\nharness = \"codex\"\nmodel = \"luna\"\n";
    let (_, e) = run(&load(&[("shop/aae.toml", project)]).unwrap());
    assert_eq!((e.harness.as_str(), &e.harness_from), ("codex", &Layer::RigAgent));
    // The short name is the adapter's: the harness gets the full one, explain shows both.
    assert_eq!(e.model, Some(from(Layer::RigAgent, "gpt-6-luna")));
    assert_eq!(e.model_key().as_deref(), Some("model:codex/gpt-6-luna"));
    assert!(
        e.explain()
            .contains("model = \"gpt-6-luna\"  # rig [agents], given as \"luna\"\n"),
        "{}",
        e.explain()
    );
}

#[test]
fn a_harness_no_layer_names_is_an_error_of_the_formula() {
    let errors = load(&[(AGENT, "effort = \"high\"\n")]).unwrap_err();
    let error = (errors.iter()).find(|e| e.message.contains("no layer sets `harness`"));
    // Said of the project file, with the formula and rig whose runs it is about.
    let error = error.unwrap_or_else(|| panic!("{errors:?}"));
    assert!(error.file.ends_with("shop/aae.toml") && error.message.starts_with("formula `kit.implement` on rig"));
}

#[test]
fn the_pack_and_the_formula_step_are_layers() {
    let pack = "name = \"kit\"\nversion = \"0.3.0\"\nrequires_plugins = [\"mail\"]\n\
                [[trigger]]\nname = \"nightly\"\ncron = \"0 3 * * *\"\nrun = \"audit\"\n[settings]\nmodel = \"haiku\"\neffort = \"low\"\n";
    let formula = "name = \"implement\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
                   [[step]]\nkind = \"ai\"\nagent = \"implementer\"\nprompt = \"implement.md\"\neffort = \"max\"\n";
    let config = load(&[("kit/pack.toml", pack), ("kit/formulas/implement.toml", formula)]).unwrap();
    let (agent, e) = run(&config);
    assert_eq!(agent, "kit.implementer");
    // The rig edit of BASE is stronger than both.
    assert_eq!(e.model, Some(from(Layer::RigAgent, "opus")));
    assert_eq!(
        e.effort,
        Some(from(Layer::Formula("kit.implement step 1".into()), "max"))
    );
    let project = "packs = [\"../kit\"]\n[rigs.backend]\npath = \"backend\"\n";
    let config = load(&[("kit/pack.toml", pack), ("shop/aae.toml", project)]).unwrap();
    let (_, e) = run(&config);
    assert_eq!(e.model, Some(from(Layer::Pack("kit".into()), "haiku")));
    assert!(
        e.explain().contains("effort = \"low\"  # pack kit\n"),
        "{}",
        e.explain()
    );
    // The agent alone has no formula.
    assert_eq!(
        config.effective("shop", None, &run_agent()).unwrap().effort,
        Some(from(Layer::Agent, "high"))
    );

    rejected(
        &[("kit/pack.toml", &format!("{pack}system_prompt = \"x\"\n"))],
        "unknown field `system_prompt`",
    );
    let exec = "name = \"audit\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\nmodel = \"opus\"\n";
    rejected(&[("kit/formulas/audit.toml", exec)], "unknown field `model`");
}

fn run_agent() -> crate::config::Name {
    crate::config::Name {
        pack: "kit".into(),
        name: "implementer".into(),
    }
}

const VARIANT: &str = "packs = [\"../kit\"]\n\
    [agents.fast]\nbase = \"kit.implementer\"\nharness = \"codex\"\nmodel = \"luna\"\n\
    [agents.implementer]\nmodel = \"opus\"\n\
    [rigs.backend]\npath = \"backend\"\nagents = { implementer = \"fast\", fast = { effort = \"low\" } }\n";

#[test]
fn a_rig_runs_a_variant_in_place_of_the_agent() {
    let config = load(&[("shop/aae.toml", VARIANT)]).unwrap();
    let (agent, e) = run(&config);
    assert_eq!(agent, "fast");
    // The role is the base agent's; the variant's settings over the agent's; edits of the base do not apply.
    assert_eq!(
        e.system_prompt.as_ref().map(|s| (s.value.as_str(), &s.from)),
        Some(("You implement.", &Layer::Agent))
    );
    assert_eq!(
        (e.harness.as_str(), &e.harness_from),
        ("codex", &Layer::Variant("fast".into()))
    );
    assert_eq!(e.model, Some(from(Layer::Variant("fast".into()), "gpt-6-luna")));
    assert_eq!(e.effort, Some(from(Layer::RigAgent, "low")));
    assert_eq!(config.max_runs("shop", "fast"), None);
}

#[test]
fn variant_errors() {
    let bad =
        |text: &str, needle: &str| rejected(&[("shop/aae.toml", &VARIANT.replace("[agents.fast]", text))], needle);
    bad(
        "[agents.slow]\nbase = \"kit.nobody\"\n[agents.fast]",
        "`agents.slow`: `base`: no agent `kit.nobody`",
    );
    bad(
        "[agents.slow]\nbase = \"fast\"\n[agents.fast]",
        "`fast` is a variant: a variant is of a pack agent",
    );
    // A table with `base` declares a variant, which takes no pack agent's name.
    let clash = VARIANT.replace("model = \"opus\"", "base = \"kit.implementer\"");
    rejected(
        &[("shop/aae.toml", &clash)],
        "the name is that of agent `kit.implementer`",
    );
    // A table without `base` edits an agent: a name no agent has is an error that says how to declare a variant.
    bad(
        "[agents.slow]\nmodel = \"opus\"\n[agents.fast]",
        "`agents.slow`: no agent `slow`",
    );
    bad(
        "[agents.slow]\nmodel = \"opus\"\n[agents.fast]",
        "a table of a new name is a variant and needs `base`",
    );
    bad(
        "[agents.slow]\nmax_runs = 2\n[agents.fast]",
        "`agents.slow`: `max_runs` is the agent's own",
    );
    let unmapped = VARIANT.replace("implementer = \"fast\"", "implementer = \"gone\"");
    rejected(
        &[("shop/aae.toml", &unmapped)],
        "rig `backend`: `agents`: no agent variant `gone`",
    );
}

#[test]
fn a_harness_of_the_build_needs_no_section() {
    // `HARNESSES` has no `[harness.codex]`: runs on codex assemble, and codex gets its defaults (spec §6.1).
    let project = "packs = [\"../kit\"]\n[settings.native.codex]\napproval_policy = \"never\"\n\
                   [rigs.backend]\npath = \"backend\"\n[agents.implementer]\nharness = \"codex\"\n";
    let config = load(&[("shop/aae.toml", project)]).unwrap();
    let (_, e) = run(&config);
    assert_eq!((e.harness.as_str(), &e.harness_from), ("codex", &Layer::ProjectAgent));
    assert_eq!(e.native.unwrap().value["approval_policy"], "never");
    assert!(config.harnesses.contains_key("codex"));
    // A harness neither of the build nor with a section stays unknown.
    rejected(
        &[(AGENT, "harness = \"opencode\"\n")],
        "harness `opencode` is neither of this build",
    );
}

#[test]
fn a_retired_key_is_one_line_that_says_where_it_went() {
    let rig = "[rigs.backend]\npath = \"backend\"\n[rigs.backend.overrides.implementer]\nmodel = \"opus\"\n";
    let step =
        "name = \"implement\"\n[[step]]\nkind = \"prompt\"\nagent = \"implementer\"\nprompt = \"implement.md\"\n";
    for (file, text, hint) in [
        (
            "config.toml",
            "[settings]\npermission_mode = \"never\"\n",
            "its own key in `[settings.native.<harness>]`",
        ),
        (
            AGENT,
            "harness = \"claude\"\npermission_mode = \"plan\"\n",
            "`permission_mode` is native to a harness",
        ),
        (
            "shop/aae.toml",
            "[overrides.implementer]\nmodel = \"opus\"\n",
            "`[overrides.<agent>]` is `[agents.<agent>]` now",
        ),
        ("shop/aae.toml", rig, "`[overrides.<agent>]` is `[agents.<agent>]` now"),
        ("kit/formulas/implement.toml", step, "step kind `prompt` is `ai` now"),
    ] {
        let errors = load(&[(file, text)]).expect_err(text);
        let error = (errors.iter().find(|e| e.message.contains(hint))).unwrap_or_else(|| panic!("{errors:?}"));
        assert!(!error.message.contains('\n'), "one line: {}", error.message);
    }
}
