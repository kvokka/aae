use crate::config::{Config, ConfigError, Name, Source, StepKind};
use std::{fs, time::Duration};

/// A valid config: an orchestrator with an addon, a project with a rig and a project trigger, and one pack.
/// The prompt template of the `implementer` of `BASE`.
pub(super) const TEMPLATE: &str = "kit/agents/implementer/prompt.template.md";

pub(super) const BASE: &[(&str, &str)] = &[
    (
        "config.toml",
        r#"projects = ["shop"]
[harness.claude]
model = "sonnet"
[[addon]]
path = "addons/mail"
[settings]
env = { LANG = "C" }
"#,
    ),
    (
        "addons/mail/addon.toml",
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
    ("addons/mail/skills/mail/SKILL.md", "mail"),
    (
        "shop/loop-manager.toml",
        r#"packs = ["../kit"]
[rigs.backend]
path = "backend"
vars = { test_cmd = "cargo test" }
triggers = ["nightly"]
[rigs.backend.agents.implementer]
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
requires_addons = ["mail"]
min_loop_manager = "0.0.1"
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
addons = [{ name = "mail" }]
"#,
    ),
    ("kit/agents/implementer/prompt.md", "You implement."),
    (
        "kit/formulas/implement.toml",
        r#"name = "implement"
[inputs]
bead = { type = "task", required = true }
[[step]]
kind = "ai"
agent = "implementer"
timeout = "45m"
"#,
    ),
    (
        "kit/agents/implementer/prompt.template.md",
        "Implement {{ bead.title }} ({{ inputs.bead }}) in {{ rig.path }}, check with `{{ rig.test_cmd }}`.",
    ),
    (
        "kit/formulas/audit.toml",
        r#"name = "audit"
[[step]]
kind = "exec"
command = ["sh", "-c", "true"]
"#,
    ),
];

/// Loads `BASE` with `changes` written over it (same path replaces, a new path adds).
pub(super) fn load_with(changes: &[(&str, &str)]) -> Result<Config, Vec<ConfigError>> {
    load_for_adapters(changes, &[])
}

/// [`load_with`] for a build with `adapters`.
pub(super) fn load_for_adapters(
    changes: &[(&str, &str)],
    adapters: &'static [&'static crate::driver::Adapter],
) -> Result<Config, Vec<ConfigError>> {
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
    crate::config::load_for(&dir.path().join("config.toml"), adapters, Default::default())
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
    let mail = &config.addons[0];
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
        (agent.harness.as_deref(), agent.settings.system_prompt.as_deref()),
        (Some("claude"), Some("You implement."))
    );
    assert_eq!(agent.settings.effort.as_deref(), Some("high"));
    match &kit.formulas["implement"].steps[0].kind {
        StepKind::Ai { agent, timeout, .. } => {
            assert_eq!(*agent, name("kit", "implementer"));
            assert_eq!(*timeout, Some(Duration::from_secs(45 * 60)));
        }
        _ => panic!("implement is an ai step"),
    }
    assert!(
        matches!(&kit.formulas["audit"].steps[0].kind, StepKind::Exec { command, timeout: None, .. } if command.len() == 3)
    );
}

#[test]
fn a_rig_path_has_no_dot_and_no_dot_dot() {
    let project = "packs = [\"../kit\"]\n[root]\npath = \"./x/..\"\n[rigs.below]\npath = \"./x/./y/..\"\n\
                   [rigs.beside]\npath = \"../x\"\nprefix = \"bs\"\n";
    // The files make the directories the rigs name.
    let config = load_with(&[("shop/loop-manager.toml", project), ("shop/x/y/f", ""), ("x/f", "")]).unwrap();
    let shop = &config.projects[0];
    let path = |rig: &str| {
        shop.place(rig)
            .unwrap()
            .path
            .strip_prefix(shop.dir.parent().unwrap())
            .unwrap()
            .to_owned()
    };
    let paths = ["root", "below", "beside"].map(path);
    assert_eq!(paths.each_ref().map(|p| p.to_str().unwrap()), ["shop", "shop/x", "x"]);
}

const OTHER_PACK: (&str, &str) = ("other/pack.toml", "name = \"other\"\nversion = \"1.0.0\"\n");
const OTHER_FORMULA: (&str, &str) = (
    "other/formulas/implement.toml",
    "name = \"implement\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n",
);

#[test]
fn a_name_shared_by_two_packs_must_be_qualified() {
    let two_packs = BASE[3]
        .1
        .replace(r#"packs = ["../kit"]"#, r#"packs = ["../kit", "../other"]"#);
    let shared = [
        ("shop/loop-manager.toml", two_packs.as_str()),
        OTHER_PACK,
        OTHER_FORMULA,
    ];
    fails(&shared, "write one of: kit.implement, other.implement");

    let qualified = two_packs.replace(r#"run = "implement""#, r#"run = "kit.implement""#);
    let config = load_with(&[
        ("shop/loop-manager.toml", qualified.as_str()),
        OTHER_PACK,
        OTHER_FORMULA,
    ])
    .unwrap();
    assert_eq!(config.projects[0].packs[0].triggers[0].run, name("kit", "implement"));
}

#[test]
fn a_bare_name_prefers_the_own_pack() {
    // The project root pack has its own `implementer`; the pack's formula still gets the pack's one.
    let own = ("shop/agents/implementer/agent.toml", "harness = \"claude\"\n");
    let config = load_with(&[own]).unwrap();
    match &config.projects[0].packs[1].formulas["implement"].steps[0].kind {
        StepKind::Ai { agent, .. } => assert_eq!(*agent, name("kit", "implementer")),
        _ => panic!("implement is an ai step"),
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
    fails(
        &[("shop/loop-manager.toml", twice.as_str())],
        "two packs are named `kit`",
    );
    fails(
        &[(
            "kit/formulas/audit.toml",
            "name = \"a.b\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n",
        )],
        "formula name `a.b`",
    );
}

#[test]
fn two_inputs_of_an_exec_step_may_not_share_a_variable() {
    let inputs = "[inputs]\nnote-text = { type = \"string\" }\nnote_text = { type = \"string\" }\n";
    let audit = format!("name = \"audit\"\n{inputs}[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n");
    fails(
        &[("kit/formulas/audit.toml", audit.as_str())],
        "inputs `note-text` and `note_text` are both `LOOP_MANAGER_INPUT_NOTE_TEXT`",
    );
}

#[test]
fn files_reject_unknown_keys_and_undeclared_names() {
    let agent = "kit/agents/implementer/agent.toml";
    fails(
        &[(agent, "harness = \"claude\"\nmodle = \"opus\"\n")],
        "unknown field `modle`",
    );
    fails(
        &[(agent, "harness = \"codex\"\n")],
        "harness `codex` is neither of this build nor in the orchestrator config",
    );
    fails(&[(agent, "effort = \"high\"\n")], "no layer sets `harness`");
    fails(
        &[(agent, "harness = \"claude\"\nmax_runs = 0\n")],
        "`max_runs` must be a positive",
    );
    fails(
        &[(agent, "harness = \"claude\"\naddons = [{ name = \"chat\" }]\n")],
        "addon `chat` is not declared",
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
        "a server the daemon runs is a `[[service]]` with `mcp`",
    );
    fails(
        &[(agent, "harness = \"claude\"\n[hooks.lint]\nevent = \"Stop\"\n")],
        "hook `lint` needs",
    );
    fails(&[(agent, "harness = \"claude\"\nprompt = \"role.md\"\n")], "role.md");
    fails(
        &[
            (agent, "harness = \"claude\"\nsystem_prompt = \"x\"\n"),
            ("kit/agents/reviewer/prompt.md", "role"),
        ],
        "`system_prompt` and a role prompt file are both set",
    );
    // A prompt is never empty: a role prompt or a formula template of blanks is a load error naming the file.
    fails(
        &[("kit/agents/implementer/prompt.md", " \n\t\n")],
        "implementer/prompt.md` is empty",
    );
    fails(&[(TEMPLATE, "\n  \n")], "prompt.template.md` is empty");
    let unreadable_prompt = ("kit/agents/reviewer/prompt.md/.keep", "");
    fails(
        &[
            ("kit/agents/reviewer/agent.toml", "harness = \"claude\"\n"),
            unreadable_prompt,
        ],
        "prompt.md",
    );
    let slow_hook = BASE[1].1.replace("\"10s\"", "\"6m\"");
    fails(&[("addons/mail/addon.toml", slow_hook.as_str())], "ceiling is 300s");
    let pack = "kit/pack.toml";
    fails(
        &[(pack, "name = \"kit\"\nversion = \"1\"\nrequires_addons = [\"chat\"]\n")],
        "required addon `chat`",
    );
    fails(
        &[(pack, "name = \"kit\"\nversion = \"1\"\nmin_loop_manager = \"99.0.0\"\n")],
        "needs loop-manager 99.0.0",
    );
    let no_rig = BASE[3].1.replace("path = \"backend\"", "path = \"frontend\"");
    fails(&[("shop/loop-manager.toml", no_rig.as_str())], "is not a directory");
    let bad_rig = BASE[3]
        .1
        .replace("backend", "back end")
        .replace("[rigs.back end", "[rigs.\"back end\"");
    fails(&[("shop/loop-manager.toml", bad_rig.as_str())], "rig name `back end`");
    let bad_override = BASE[3].1.replace("agents.implementer", "agents.reviewer");
    fails(
        &[("shop/loop-manager.toml", bad_override.as_str())],
        "no agent `reviewer`",
    );
    let orchestrator_override = format!("{}[agents.reviewer]\nmodel = \"opus\"\n", BASE[0].1);
    fails(
        &[("config.toml", orchestrator_override.as_str())],
        "no project has agent `reviewer`",
    );
}

#[test]
fn every_error_is_reported_with_its_file() {
    let errors = load_with(&[
        ("kit/agents/implementer/agent.toml", "harness = \"codex\"\n"),
        (TEMPLATE, "{{ run.nope }}"),
    ])
    .unwrap_err();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].file.ends_with("kit/agents/implementer/agent.toml"));
    assert!(errors[1].file.ends_with(TEMPLATE));
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
