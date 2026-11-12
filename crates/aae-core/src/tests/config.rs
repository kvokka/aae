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
        r#"packs = ["../gascity"]
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
        "gascity/pack.toml",
        r#"name = "gascity"
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
        "gascity/agents/implementer/agent.toml",
        r#"harness = "claude"
effort = "high"
plugins = [{ name = "mail" }]
"#,
    ),
    ("gascity/agents/implementer/prompt.md", "You implement."),
    (
        "gascity/formulas/implement.toml",
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
        "gascity/formulas/implement.md",
        "Implement {{ bead.title }} ({{ inputs.bead }}) in {{ rig.path }}, check with `{{ rig.test_cmd }}`.",
    ),
    (
        "gascity/formulas/audit.toml",
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
fn fails(changes: &[(&str, &str)], needle: &str) {
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
    assert_eq!(packs, ["shop", "gascity"]);
    let rig = &shop.rigs["backend"];
    assert!(rig.path.ends_with("shop/backend"));
    assert_eq!(rig.triggers, [name("gascity", "nightly")]);
    let project_trigger = &shop.packs[0].triggers[0];
    assert_eq!(project_trigger.run, name("gascity", "implement"));
    assert!(matches!(&project_trigger.source, Source::Event { on, .. } if on == "bead.ready"));
    let gascity = &shop.packs[1];
    assert_eq!(gascity.version.as_deref(), Some("0.3.0"));
    assert!(
        matches!(&gascity.triggers[0].source, Source::Cron(c) if *c == crate::cron::Cron::parse("0 3 * * *").unwrap())
    );
    let agent = &gascity.agents["implementer"];
    assert_eq!(
        (agent.harness.as_str(), agent.prompt.as_deref()),
        ("claude", Some("You implement."))
    );
    assert_eq!(agent.settings.effort.as_deref(), Some("high"));
    match &gascity.formulas["implement"].step {
        Step::Prompt { agent, timeout, .. } => {
            assert_eq!(*agent, name("gascity", "implementer"));
            assert_eq!(*timeout, Some(Duration::from_secs(45 * 60)));
        }
        Step::Exec { .. } => panic!("implement is a prompt step"),
    }
    assert!(matches!(&gascity.formulas["audit"].step, Step::Exec { command, timeout: None } if command.len() == 3));
}

#[test]
fn no_orchestrator_file_is_an_empty_config() {
    let dir = tempfile::tempdir().unwrap();
    let config = load(&dir.path().join("config.toml")).unwrap();
    assert!(config.projects.is_empty() && config.harnesses.is_empty());
    let on: Vec<_> = config.bundled.iter().map(|b| b.name).collect();
    assert_eq!(on, ["default-policy"], "worktree acts only where it is turned on");
}

#[test]
fn builtin_tables_turn_bundled_plugins_on_and_off() {
    let config = |text: &str| load_with(&[("config.toml", text)]);
    let on = config("[builtin.default-policy]\nenabled = false\n[builtin.worktree]\nagents = [\"dev\"]\n").unwrap();
    let on: Vec<_> = on.bundled.iter().map(|b| (b.name, b.params.to_string())).collect();
    assert_eq!(on, [("worktree", r#"{"agents":["dev"]}"#.to_owned())]);
    fails(
        &[("config.toml", "[builtin.policy]\n")],
        "`policy` is not a bundled plugin",
    );
    fails(
        &[("config.toml", "[builtin.worktree]\nenabled = 1\n")],
        "`enabled` is a boolean",
    );
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
        .replace(r#"packs = ["../gascity"]"#, r#"packs = ["../gascity", "../other"]"#);
    let shared = [("shop/aae.toml", two_packs.as_str()), OTHER_PACK, OTHER_FORMULA];
    fails(&shared, "write one of: gascity.implement, other.implement");

    let qualified = two_packs.replace(r#"run = "implement""#, r#"run = "gascity.implement""#);
    let config = load_with(&[("shop/aae.toml", qualified.as_str()), OTHER_PACK, OTHER_FORMULA]).unwrap();
    assert_eq!(
        config.projects[0].packs[0].triggers[0].run,
        name("gascity", "implement")
    );
}

#[test]
fn a_bare_name_prefers_the_own_pack() {
    // The project root pack has its own `implementer`; the pack's formula still gets the pack's one.
    let own = ("shop/agents/implementer/agent.toml", "harness = \"claude\"\n");
    let config = load_with(&[own]).unwrap();
    match &config.projects[0].packs[1].formulas["implement"].step {
        Step::Prompt { agent, .. } => assert_eq!(*agent, name("gascity", "implementer")),
        Step::Exec { .. } => panic!("implement is a prompt step"),
    }
}

#[test]
fn names_are_unique_within_a_pack_and_a_project() {
    fails(
        &[("gascity/formulas/again.toml", BASE[10].1)],
        "two formulas of the pack are named `audit`",
    );
    let twice = BASE[3]
        .1
        .replace(r#"packs = ["../gascity"]"#, r#"packs = ["../gascity", "../gascity"]"#);
    fails(&[("shop/aae.toml", twice.as_str())], "two packs are named `gascity`");
    fails(
        &[(
            "gascity/formulas/audit.toml",
            "name = \"a.b\"\n[step]\nkind = \"exec\"\ncommand = [\"true\"]\n",
        )],
        "formula name `a.b`",
    );
}

#[test]
fn a_prompt_template_may_use_only_values_its_runs_have() {
    let template = "gascity/formulas/implement.md";
    fails(&[(template, "{{ inputs.nope }}")], "`{{ inputs.nope }}`");
    fails(&[(template, "{{ bead.nope }}")], "`{{ bead.nope }}`");
    fails(&[(template, "{{ run.nope }}")], "`{{ run.nope }}`");
    fails(&[(template, "{{ project.nope }}")], "`{{ project.nope }}`");
    fails(&[(template, "{{ secrets.token }}")], "starts with none of");
    fails(&[(template, "{{ inputs.bead")], "unclosed");
    let no_bead = BASE[8].1.replace(r#"type = "bead""#, r#"type = "string""#);
    fails(
        &[("gascity/formulas/implement.toml", no_bead.as_str())],
        "`{{ bead.title }}`",
    );
}

/// The project file with its trigger replaced by `trigger`.
fn project_trigger(trigger: &str) -> String {
    let start = BASE[3].1.find("[[trigger]]").unwrap();
    format!("{}[[trigger]]\n{trigger}\n", &BASE[3].1[..start])
}

#[test]
fn triggers_are_checked_against_events_and_formulas() {
    let cases = [
        (
            "name = \"t\"\non = \"bead.ready\"\nwhere = { \"bead.nope\" = 1 }\nrun = \"audit\"",
            "has no `bead.nope`",
        ),
        (
            "name = \"t\"\non = \"run.passed\"\nwhere = { run = \"r1\" }\nrun = \"audit\"",
            "has no `run`",
        ),
        (
            "name = \"t\"\non = \"bead.gone\"\nrun = \"audit\"",
            "unknown event type `bead.gone`",
        ),
        (
            "name = \"t\"\nevery = \"1h\"\nrun = \"implement\"\ninputs = { bead = \"{{ event.bead.id }}\" }",
            "no event",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\nrun = \"implement\"\ninputs = { bead = \"{{ event.bead.nope }}\" }",
            "no `bead.nope`",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\nrun = \"implement\"\ninputs = { bead = \"x\", extra = 1 }",
            "no input `extra`",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\nrun = \"implement\"",
            "required input `bead`",
        ),
        (
            "name = \"t\"\non = \"bead.ready\"\ncron = \"0 3 * * *\"\nrun = \"audit\"",
            "exactly one of",
        ),
        ("name = \"t\"\ncron = \"0 3 * *\"\nrun = \"audit\"", "five cron fields"),
        ("name = \"t\"\nevery = \"3d\"\nrun = \"audit\"", "unit is one of"),
        (
            "name = \"t\"\ncron = \"0 3 * * *\"\nwhere = { project = \"x\" }\nrun = \"audit\"",
            "`where` needs `on`",
        ),
        (
            "name = \"t\"\ncron = \"0 3 * * *\"\nrun = \"audit\"\nrig = \"frontend\"",
            "no rig `frontend`",
        ),
        (
            "name = \"t\"\ncron = \"0 3 * * *\"\nrun = \"nothing\"",
            "no formula `nothing`",
        ),
    ];
    for (trigger, needle) in cases {
        fails(&[("shop/aae.toml", project_trigger(trigger).as_str())], needle);
    }
    let with_rig = BASE[5]
        .1
        .replace("run = \"audit\"", "run = \"audit\"\nrig = \"backend\"");
    fails(
        &[("gascity/pack.toml", with_rig.as_str())],
        "gets its rig from the project",
    );
}

#[test]
fn files_reject_unknown_keys_and_undeclared_names() {
    let agent = "gascity/agents/implementer/agent.toml";
    fails(
        &[(agent, "harness = \"claude\"\nmodle = \"opus\"\n")],
        "unknown field `modle`",
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
            "harness = \"claude\"\n[mcp.web]\ncommand = [\"srv\"]\nurl = \"http://x\"\n",
        )],
        "mcp `web` needs exactly one",
    );
    fails(
        &[(agent, "harness = \"claude\"\n[hooks.lint]\nevent = \"Stop\"\n")],
        "hook `lint` needs",
    );
    fails(&[(agent, "harness = \"claude\"\nprompt = \"role.md\"\n")], "role.md");
    let unreadable_prompt = ("gascity/agents/reviewer/prompt.md/.keep", "");
    fails(
        &[
            ("gascity/agents/reviewer/agent.toml", "harness = \"claude\"\n"),
            unreadable_prompt,
        ],
        "prompt.md",
    );
    let slow_hook = BASE[1].1.replace("\"10s\"", "\"6m\"");
    fails(&[("plugins/mail/plugin.toml", slow_hook.as_str())], "ceiling is 300s");
    let pack = "gascity/pack.toml";
    fails(
        &[(
            pack,
            "name = \"gascity\"\nversion = \"1\"\nrequires_plugins = [\"chat\"]\n",
        )],
        "required plugin `chat`",
    );
    fails(
        &[(pack, "name = \"gascity\"\nversion = \"1\"\nmin_aae = \"99.0.0\"\n")],
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
        ("gascity/agents/implementer/agent.toml", "harness = \"codex\"\n"),
        ("gascity/formulas/implement.md", "{{ run.nope }}"),
    ])
    .unwrap_err();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].file.ends_with("gascity/agents/implementer/agent.toml"));
    assert!(errors[1].file.ends_with("gascity/formulas/implement.md"));
}
