//! The launch settings: harness and provider derived from each other, short model names, the pack and formula
//! layers and agent variants (docs/design/config.md).

use super::config::load_for_adapters;
use crate::{
    config::{Config, ConfigError, EffectiveConfig, Layer},
    driver::Adapter,
};

static CLAUDE: Adapter = Adapter {
    name: "claude",
    providers: &["anthropic"],
    models: &[],
    tested: [(0, 0, 0), (0, 0, 0)],
    new: |_| Err(String::new()),
    doctor: |_| Vec::new(),
};
static CODEX: Adapter = Adapter {
    name: "codex",
    providers: &["openai"],
    models: &[("luna", "gpt-6-luna")],
    tested: [(0, 0, 0), (0, 0, 0)],
    new: |_| Err(String::new()),
    doctor: |_| Vec::new(),
};
static ADAPTERS: &[&Adapter] = &[&CLAUDE, &CODEX];

const AGENT: &str = "kit/agents/implementer/agent.toml";
const HARNESSES: (&str, &str) = (
    "config.toml",
    "projects = [\"shop\"]\n[harness.claude]\nmodel = \"sonnet\"\n[harness.codex]\n[[plugin]]\npath = \"plugins/mail\"\n",
);

fn load(changes: &[(&str, &str)]) -> Result<Config, Vec<ConfigError>> {
    let mut all = vec![HARNESSES];
    all.extend_from_slice(changes);
    load_for_adapters(&all, ADAPTERS)
}

fn run(config: &Config) -> (String, EffectiveConfig) {
    config.for_run("shop", "backend", "kit.implement").unwrap()
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
fn harness_and_provider_follow_from_each_other() {
    let (_, e) = run(&load(&[]).unwrap());
    assert_eq!((e.harness.as_str(), &e.harness_from), ("claude", &Layer::Agent));
    assert_eq!(e.provider, Some(from(Layer::Derived, "anthropic")));

    let (_, e) = run(&load(&[(AGENT, "provider = \"openai\"\nplugins = [{ name = \"mail\" }]\n")]).unwrap());
    assert_eq!((e.harness.as_str(), &e.harness_from), ("codex", &Layer::Derived));
    assert_eq!(e.provider, Some(from(Layer::Agent, "openai")));
    let text = e.explain();
    assert!(
        text.contains("harness = \"codex\"  # derived\nprovider = \"openai\"  # agent\n"),
        "{text}"
    );
}

#[test]
fn an_override_changes_the_harness() {
    let project = "packs = [\"../kit\"]\n[rigs.backend]\npath = \"backend\"\n\
                   [rigs.backend.overrides.implementer]\nharness = \"codex\"\nmodel = \"luna\"\n";
    let (_, e) = run(&load(&[("shop/aae.toml", project)]).unwrap());
    assert_eq!((e.harness.as_str(), &e.harness_from), ("codex", &Layer::RigOverride));
    assert_eq!(e.provider, Some(from(Layer::Derived, "openai")));
    // The short name is the adapter's: the harness gets the full one, explain shows both.
    assert_eq!(e.model, Some(from(Layer::RigOverride, "gpt-6-luna")));
    assert_eq!(e.model_key().as_deref(), Some("model:openai/gpt-6-luna"));
    assert!(
        e.explain()
            .contains("model = \"gpt-6-luna\"  # rig override, given as \"luna\"\n"),
        "{}",
        e.explain()
    );
}

#[test]
fn launch_settings_that_do_not_go_together_are_load_errors() {
    rejected(
        &[(AGENT, "effort = \"high\"\n")],
        "neither `harness` nor `provider` is set",
    );
    rejected(
        &[(AGENT, "harness = \"claude\"\nprovider = \"openai\"\n")],
        "`provider = \"openai\"` (agent) is not one that harness `claude` works with: anthropic",
    );
    rejected(
        &[(AGENT, "provider = \"acme\"\n")],
        "provider `acme`: no harness of this build works with it",
    );
    let only_claude = (
        "config.toml",
        "projects = [\"shop\"]\n[harness.claude]\n[[plugin]]\npath = \"plugins/mail\"\n",
    );
    let errors = load(&[only_claude, (AGENT, "provider = \"openai\"\n")]).unwrap_err();
    let error = errors.iter().find(|e| {
        e.message
            .contains("harness `codex` (derived) is not in the orchestrator")
    });
    // Said of the project file, with the formula and rig whose runs it is about.
    let error = error.unwrap_or_else(|| panic!("{errors:?}"));
    assert!(error.file.ends_with("shop/aae.toml") && error.message.starts_with("formula `kit.implement` on rig"));
}

#[test]
fn the_pack_and_the_formula_step_are_layers() {
    let pack = "name = \"kit\"\nversion = \"0.3.0\"\nrequires_plugins = [\"mail\"]\n\
                [[trigger]]\nname = \"nightly\"\ncron = \"0 3 * * *\"\nrun = \"audit\"\n[settings]\nmodel = \"haiku\"\neffort = \"low\"\n";
    let formula = "name = \"implement\"\n[inputs]\nbead = { type = \"bead\", required = true }\n\
                   [step]\nkind = \"prompt\"\nagent = \"implementer\"\nprompt = \"implement.md\"\neffort = \"max\"\n";
    let config = load(&[("kit/pack.toml", pack), ("kit/formulas/implement.toml", formula)]).unwrap();
    let (agent, e) = run(&config);
    assert_eq!(agent, "kit.implementer");
    // The rig override of BASE is stronger than both.
    assert_eq!(e.model, Some(from(Layer::RigOverride, "opus")));
    assert_eq!(e.effort, Some(from(Layer::Formula("kit.implement".into()), "max")));
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
    let exec = "name = \"audit\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\nmodel = \"opus\"\n";
    rejected(&[("kit/formulas/audit.toml", exec)], "unknown field `model`");
}

fn run_agent() -> crate::config::Name {
    crate::config::Name {
        pack: "kit".into(),
        name: "implementer".into(),
    }
}

const VARIANT: &str = "packs = [\"../kit\"]\n\
    [agents.fast]\nbase = \"kit.implementer\"\nprovider = \"openai\"\nmodel = \"luna\"\n\
    [overrides.fast]\neffort = \"low\"\n\
    [rigs.backend]\npath = \"backend\"\nagents = { implementer = \"fast\" }\n\
    [rigs.backend.overrides.implementer]\nmodel = \"opus\"\n";

#[test]
fn a_rig_runs_a_variant_in_place_of_the_agent() {
    let config = load(&[("shop/aae.toml", VARIANT)]).unwrap();
    let (agent, e) = run(&config);
    assert_eq!(agent, "fast");
    // The role is the base agent's; the variant's settings over the agent's; overrides of the base do not apply.
    assert_eq!(
        e.system_prompt.as_ref().map(|s| (s.value.as_str(), &s.from)),
        Some(("You implement.", &Layer::Agent))
    );
    assert_eq!(e.provider, Some(from(Layer::Variant("fast".into()), "openai")));
    // The agent's harness does not work with the variant's provider, which is stronger: the harness follows from it.
    assert_eq!((e.harness.as_str(), &e.harness_from), ("codex", &Layer::Derived));
    assert_eq!(e.model, Some(from(Layer::Variant("fast".into()), "gpt-6-luna")));
    assert_eq!(e.effort, Some(from(Layer::ProjectOverride, "low")));
    assert_eq!(config.max_runs("shop", "fast"), None);
}

#[test]
fn variant_errors() {
    let bad =
        |text: &str, needle: &str| rejected(&[("shop/aae.toml", &VARIANT.replace("[overrides.fast]", text))], needle);
    bad(
        "[agents.slow]\nbase = \"kit.nobody\"\n[overrides.fast]",
        "`agents.slow`: `base`: no agent `kit.nobody`",
    );
    bad(
        "[agents.slow]\nbase = \"fast\"\n[overrides.fast]",
        "`fast` is a variant: a variant is of a pack agent",
    );
    bad(
        "[agents.implementer]\nbase = \"kit.implementer\"\n[overrides.fast]",
        "the name is that of agent `kit.implementer`",
    );
    let unmapped = VARIANT.replace("implementer = \"fast\"", "implementer = \"gone\"");
    rejected(
        &[("shop/aae.toml", &unmapped)],
        "rig `backend`: `agents`: no agent variant `gone`",
    );
}
