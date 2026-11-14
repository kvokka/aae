use crate::config::{Config, ConfigError, Name, Source, Step, load};
use std::{fs, time::Duration};

/// A valid config: an orchestrator with a plugin, a project with a rig and a project trigger, and one pack.
pub(super) const BASE: &[(&str, &str)] = &[
    (
        "config.toml",
        r#"projects = ["shop"]
[harness.claude]
model = "sonnet"
[[plugin]]
path = "plugins/mail"
[settings]
env = { LANG = "C" }
"#,
    ),
    (
        "plugins/mail/plugin.toml",
        r#"name = "mail"
version = "0.1.0"
[agent.skills.mail]
path = "skills/mail"
[[hook]]
on = "run.finishing"
command = ["bin/check"]
timeout = "10s"
"#,
    ),
    ("plugins/mail/skills/mail/SKILL.md", "mail"),
    (
        "shop/aae.toml",
        r#"packs = ["../kit"]
[rigs.backend]
path = "backend"
vars = { test_cmd = "cargo test" }
triggers = ["nightly"]
[rigs.backend.overrides.implementer]
model = "opus"
[[trigger]]
name = "implement-next"
on = "bead.ready"
where = { "bead.labels" = "auto" }
run = "implement"
inputs = { bead = { next_ready = true, epic = "{{ event.bead.epic }}" } }
"#,
    ),
    ("shop/backend/.keep", ""),
    (
        "kit/pack.toml",
        r#"name = "kit"
version = "0.3.0"
requires_plugins = ["mail"]
min_aae = "0.1.0"
[[trigger]]
name = "nightly"
cron = "0 3 * * *"
run = "audit"
"#,
    ),
    (
        "kit/agents/implementer/agent.toml",
        r#"harness = "claude"
effort = "high"
plugins = [{ name = "mail" }]
"#,
    ),
    ("kit/agents/implementer/prompt.md", "You implement."),
    (
        "kit/formulas/implement.toml",
        r#"name = "implement"
[inputs]
bead = { type = "bead", required = true }
[step]
kind = "prompt"
agent = "implementer"
prompt = "implement.md"
timeout = "45m"
"#,
    ),
    (
        "kit/formulas/implement.md",
        "Implement {{ bead.title }} ({{ inputs.bead }}) in {{ rig.path }}, check with `{{ rig.test_cmd }}`.",
    ),
    (
        "kit/formulas/audit.toml",
        r#"name = "audit"
[step]
kind = "exec"
command = ["sh", "-c", "true"]
"#,
    ),
];

/// Loads `BASE` with `changes` written over it (same path replaces, a new path adds).
pub(super) fn load_with(changes: &[(&str, &str)]) -> Result<Config, Vec<ConfigError>> {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in BASE
        .iter()
        .filter(|(p, _)| !changes.iter().any(|(c, _)| c == p))
        .chain(changes)
    {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    load(&dir.path().join("config.toml"))
}

/// Asserts that loading fails and one error mentions `needle`.
pub(super) fn fails(changes: &[(&str, &str)], needle: &str) {
    let errors = load_with(changes).expect_err("the config must be rejected");
    assert!(
        errors.iter().any(|e| e.message.contains(needle)),
        "no error mentions `{needle}`: {errors:?}"
    );
}

fn name(pack: &str, name: &str) -> Name {
    Name {
        pack: pack.into(),
        name: name.into(),
    }
}

#[test]
fn a_valid_config_loads_with_every_reference_resolved() {
    let config = load_with(&[]).unwrap();
    assert_eq!(config.harnesses["claude"].model.as_deref(), Some("sonnet"));
    assert_eq!(config.settings.env["LANG"], "C");
    let mail = &config.plugins[0];
    assert_eq!(mail.hooks[0].timeout, Some(Duration::from_secs(10)));
    assert!(mail.agent.skills["mail"].path.as_ref().unwrap().is_absolute());
    let shop = &config.projects[0];
    assert_eq!(shop.name, "shop");
    let packs: Vec<&str> = shop.packs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(packs, ["shop", "kit"]);
    let rig = &shop.rigs["backend"];
    assert!(rig.path.ends_with("shop/backend"));
    assert_eq!(rig.triggers, [name("kit", "nightly")]);
    let project_trigger = &shop.packs[0].triggers[0];
    assert_eq!(project_trigger.run, name("kit", "implement"));
    assert!(matches!(&project_trigger.source, Source::Event { on, .. } if on == "bead.ready"));
    let kit = &shop.packs[1];
    assert_eq!(kit.version.as_deref(), Some("0.3.0"));
    assert!(matches!(&kit.triggers[0].source, Source::Cron(c) if *c == crate::cron::Cron::parse("0 3 * * *").unwrap()));
    let agent = &kit.agents["implementer"];
    assert_eq!(
        (agent.harness.as_str(), agent.prompt.as_deref()),
        ("claude", Some("You implement."))
    );
    assert_eq!(agent.settings.effort.as_deref(), Some("high"));
    match &kit.formulas["implement"].step {
        Step::Prompt { agent, timeout, .. } => {
            assert_eq!(*agent, name("kit", "implementer"));
            assert_eq!(*timeout, Some(Duration::from_secs(45 * 60)));
        }
        Step::Exec { .. } => panic!("implement is a prompt step"),
    }
    assert!(matches!(&kit.formulas["audit"].step, Step::Exec { command, timeout: None } if command.len() == 3));
}

const OTHER_PACK: (&str, &str) = ("other/pack.toml", "name = \"other\"\nversion = \"1.0.0\"\n");
const OTHER_FORMULA: (&str, &str) = (
    "other/formulas/implement.toml",
    "name = \"implement\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\n",
);

#[test]
fn a_name_shared_by_two_packs_must_be_qualified() {
    let two_packs = BASE[3]
        .1
        .replace(r#"packs = ["../kit"]"#, r#"packs = ["../kit", "../other"]"#);
    let shared = [("shop/aae.toml", two_packs.as_str()), OTHER_PACK, OTHER_FORMULA];
    fails(&shared, "write one of: kit.implement, other.implement");

    let qualified = two_packs.replace(r#"run = "implement""#, r#"run = "kit.implement""#);
    let config = load_with(&[("shop/aae.toml", qualified.as_str()), OTHER_PACK, OTHER_FORMULA]).unwrap();
    assert_eq!(config.projects[0].packs[0].triggers[0].run, name("kit", "implement"));
}

#[test]
fn a_bare_name_prefers_the_own_pack() {
    // The project root pack has its own `implementer`; the pack's formula still gets the pack's one.
    let own = ("shop/agents/implementer/agent.toml", "harness = \"claude\"\n");
    let config = load_with(&[own]).unwrap();
    match &config.projects[0].packs[1].formulas["implement"].step {
        Step::Prompt { agent, .. } => assert_eq!(*agent, name("kit", "implementer")),
        Step::Exec { .. } => panic!("implement is a prompt step"),
    }
}

#[test]
fn names_are_unique_within_a_pack_and_a_project() {
    fails(
        &[("kit/formulas/again.toml", BASE[10].1)],
        "two formulas of the pack are named `audit`",
    );
    let twice = BASE[3]
        .1
        .replace(r#"packs = ["../kit"]"#, r#"packs = ["../kit", "../kit"]"#);
    fails(&[("shop/aae.toml", twice.as_str())], "two packs are named `kit`");
    fails(
        &[(
            "kit/formulas/audit.toml",
            "name = \"a.b\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\n",
        )],
        "formula name `a.b`",
    );
}

#[test]
fn two_inputs_of_an_exec_step_may_not_share_a_variable() {
    let inputs = "[inputs]\nnote-text = { type = \"string\" }\nnote_text = { type = \"string\" }\n";
    let audit = format!("name = \"audit\"\n{inputs}[step]\nkind = \"exec\"\ncommand = [\"true\"]\n");
    fails(
        &[("kit/formulas/audit.toml", audit.as_str())],
        "inputs `note-text` and `note_text` are both `AAE_INPUT_NOTE_TEXT`",
    );
}

#[test]
fn a_prompt_template_may_use_only_values_its_runs_have() {
    let template = "kit/formulas/implement.md";
    fails(&[(template, "{{ inputs.nope }}")], "`{{ inputs.nope }}`");
    fails(&[(template, "{{ bead.nope }}")], "`{{ bead.nope }}`");
    fails(&[(template, "{{ run.nope }}")], "`{{ run.nope }}`");
    fails(&[(template, "{{ project.nope }}")], "`{{ project.nope }}`");
    fails(&[(template, "{{ secrets.token }}")], "starts with none of");
    fails(&[(template, "{{ inputs.bead")], "unclosed");
    let no_bead = BASE[8].1.replace(r#"type = "bead""#, r#"type = "string""#);
    fails(
        &[("kit/formulas/implement.toml", no_bead.as_str())],
        "`{{ bead.title }}`",
    );
}

#[test]
fn files_reject_unknown_keys_and_undeclared_names() {
    let agent = "kit/agents/implementer/agent.toml";
    fails(
        &[(agent, "harness = \"claude\"\nmodle = \"opus\"\n")],
        "unknown field `modle`",
    );
    // Plugin parameters live in the `[[plugin]]` declaration only: an attachment carries none.
    fails(
        &[(
            agent,
            "harness = \"claude\"\nplugins = [{ name = \"mail\", config = { level = 2 } }]\n",
        )],
        "unknown field `config`",
    );
    fails(
        &[(agent, "harness = \"codex\"\n")],
        "harness `codex` is not in the orchestrator config",
    );
    fails(&[(agent, "effort = \"high\"\n")], "`harness` is required");
    fails(
        &[(agent, "harness = \"claude\"\nmax_runs = 0\n")],
        "`max_runs` must be a positive",
    );
    fails(
        &[(agent, "harness = \"claude\"\nplugins = [{ name = \"chat\" }]\n")],
        "plugin `chat` is not declared",
    );
    fails(
        &[(agent, "harness = \"claude\"\n[native.codex]\nx = 1\n")],
        "`native.codex`",
    );
    fails(
        &[(
            agent,
            "harness = \"claude\"\n[mcp.web]\ncommand = [\"srv\"]\nurl = \"http://x\"\nscope = \"session\"\n",
        )],
        "with both it is a `[[service]]` with `mcp`",
    );
    fails(
        &[(agent, "harness = \"claude\"\n[hooks.lint]\nevent = \"Stop\"\n")],
        "hook `lint` needs",
    );
    fails(&[(agent, "harness = \"claude\"\nprompt = \"role.md\"\n")], "role.md");
    let unreadable_prompt = ("kit/agents/reviewer/prompt.md/.keep", "");
    fails(
        &[
            ("kit/agents/reviewer/agent.toml", "harness = \"claude\"\n"),
            unreadable_prompt,
        ],
        "prompt.md",
    );
    let slow_hook = BASE[1].1.replace("\"10s\"", "\"6m\"");
    fails(&[("plugins/mail/plugin.toml", slow_hook.as_str())], "ceiling is 300s");
    let pack = "kit/pack.toml";
    fails(
        &[(pack, "name = \"kit\"\nversion = \"1\"\nrequires_plugins = [\"chat\"]\n")],
        "required plugin `chat`",
    );
    fails(
        &[(pack, "name = \"kit\"\nversion = \"1\"\nmin_aae = \"99.0.0\"\n")],
        "needs aae 99.0.0",
    );
    let no_rig = BASE[3].1.replace("path = \"backend\"", "path = \"frontend\"");
    fails(&[("shop/aae.toml", no_rig.as_str())], "is not a directory");
    let bad_rig = BASE[3]
        .1
        .replace("backend", "back end")
        .replace("[rigs.back end", "[rigs.\"back end\"");
    fails(&[("shop/aae.toml", bad_rig.as_str())], "rig name `back end`");
    let bad_override = BASE[3].1.replace("overrides.implementer", "overrides.reviewer");
    fails(&[("shop/aae.toml", bad_override.as_str())], "no agent `reviewer`");
    let orchestrator_override = format!("{}[overrides.reviewer]\nmodel = \"opus\"\n", BASE[0].1);
    fails(
        &[("config.toml", orchestrator_override.as_str())],
        "no project has agent `reviewer`",
    );
}

#[test]
fn every_error_is_reported_with_its_file() {
    let errors = load_with(&[
        ("kit/agents/implementer/agent.toml", "harness = \"codex\"\n"),
        ("kit/formulas/implement.md", "{{ run.nope }}"),
    ])
    .unwrap_err();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].file.ends_with("kit/agents/implementer/agent.toml"));
    assert!(errors[1].file.ends_with("kit/formulas/implement.md"));
}

#[test]
fn the_adapter_settings_of_a_harness_pass_through_unread() {
    // `[harness.claude]` already opens the fixture: its keys go after its model. The adapter checks them.
    let text = BASE[0]
        .1
        .replace("\"sonnet\"\n", "\"sonnet\"\nmax_sessions = 3\nidle = \"x\"\n");
    let config = load_with(&[("config.toml", text.as_str())]).unwrap();
    let options = &config.harnesses["claude"].options;
    assert_eq!(
        (options["max_sessions"].as_integer(), options["idle"].as_str()),
        (Some(3), Some("x"))
    );
}

#[test]
fn the_api_listens_on_tcp_only_on_a_loopback_address() {
    let with = |addr: &str| {
        BASE[0]
            .1
            .replace("projects", &format!("api_tcp = \"{addr}\"\nprojects"))
    };
    let config = load_with(&[("config.toml", with("127.0.0.1:7171").as_str())]).unwrap();
    assert_eq!(config.api_tcp, Some("127.0.0.1:7171".parse().unwrap()));
    assert!(load_with(&[("config.toml", with("[::1]:7171").as_str())]).is_ok());
    fails(
        &[("config.toml", with("0.0.0.0:7171").as_str())],
        "`api_tcp` must be a loopback",
    );
    assert_eq!(load_with(BASE).unwrap().api_tcp, None);
}
