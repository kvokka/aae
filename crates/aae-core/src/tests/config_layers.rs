use super::config::{BASE, load_with};
use crate::config::{Config, EffectiveConfig, Layer, Name};
use crate::driver::{Fingerprint, ProcessPart};
use serde_json::json;

fn implementer() -> Name {
    Name {
        pack: "gascity".into(),
        name: "implementer".into(),
    }
}

/// `BASE` with one more line appended to each of the given files.
fn with_lines(lines: &[(&str, &str)]) -> Config {
    let changed: Vec<(&str, String)> = lines
        .iter()
        .map(|(path, line)| {
            let base = BASE.iter().find(|(p, _)| p == path).unwrap().1;
            (*path, format!("{base}{line}\n"))
        })
        .collect();
    let changes: Vec<(&str, &str)> = changed.iter().map(|(p, t)| (*p, t.as_str())).collect();
    load_with(&changes).unwrap()
}

fn effective(config: &Config, rig: Option<&str>) -> EffectiveConfig {
    config.effective("shop", rig, &implementer()).unwrap()
}

#[test]
fn a_stronger_layer_wins_and_every_value_keeps_its_layer() {
    let config = with_lines(&[
        (
            "config.toml",
            "effort = \"low\"\n[overrides.implementer]\npermission_mode = \"plan\"",
        ),
        (
            "shop/aae.toml",
            "[settings]\nenv = { LANG = \"en\", PROJECT = \"shop\" }",
        ),
    ]);
    let e = effective(&config, None);
    let model = e.model.as_ref().unwrap();
    assert_eq!((model.value.as_str(), &model.from), ("sonnet", &Layer::Harness));
    // The agent's `high` beats the orchestrator's `low`; an override beats the agent.
    assert_eq!(e.effort.as_ref().unwrap().from, Layer::Agent);
    assert_eq!(e.permission_mode.as_ref().unwrap().from, Layer::OrchestratorOverride);
    assert_eq!(
        (e.env["LANG"].value.as_str(), &e.env["LANG"].from),
        ("en", &Layer::Project)
    );
    assert_eq!(e.env.len(), 2, "the session environment holds only what the layers say");
    // The rig override of `BASE` applies only to runs on that rig.
    let on_rig = effective(&config, Some("backend"));
    assert_eq!(on_rig.model.as_ref().unwrap().value, "opus");
    assert_eq!(on_rig.model.as_ref().unwrap().from, Layer::RigOverride);
}

#[test]
fn the_tracker_variables_come_under_every_layer() {
    let mut config = with_lines(&[("shop/aae.toml", "[settings]\nenv = { TRACKER = \"mine\" }")]);
    let shop = config.projects.iter_mut().find(|p| p.name == "shop").unwrap();
    for (name, value) in [("TRACKER", "theirs"), ("TRACKER_DIR", "/shop/.tracker")] {
        shop.tracker.env.insert(name.to_owned(), value.to_owned());
    }
    let e = effective(&config, None);
    let env = |name: &str| (e.env[name].value.as_str(), &e.env[name].from);
    assert_eq!(env("TRACKER_DIR"), ("/shop/.tracker", &Layer::Project));
    assert_eq!(env("TRACKER"), ("mine", &Layer::Project), "a layer may still set it");
}

#[test]
fn an_attached_plugin_contributes_and_a_stronger_layer_can_switch_it_off() {
    let config = load_with(&[]).unwrap();
    let e = effective(&config, None);
    assert_eq!(e.plugins[0].from, Layer::Agent);
    assert_eq!(e.skills["mail"].from, Layer::Plugin("mail".into()));

    let config = with_lines(&[("shop/aae.toml", "[overrides.implementer.skills.mail]\nenabled = false")]);
    let e = effective(&config, None);
    assert!(e.skills.is_empty(), "a skill switched off by name is gone");
    assert_eq!(e.plugins.len(), 1);

    let config = with_lines(&[(
        "shop/aae.toml",
        "[overrides.implementer]\nplugins = [{ name = \"mail\", enabled = false }]",
    )]);
    let e = effective(&config, None);
    assert!(
        e.plugins.is_empty() && e.skills.is_empty(),
        "a detached plugin contributes nothing"
    );
}

#[test]
fn explain_names_the_layer_of_every_value_and_the_host_in_one_line() {
    let config = load_with(&[]).unwrap();
    let text = effective(&config, Some("backend")).explain();
    assert!(text.contains("model = \"opus\"  # rig override\n"), "{text}");
    assert!(text.contains("env.LANG = \"C\"  # orchestrator\n"), "{text}");
    assert!(text.contains("skills.mail = "), "{text}");
    assert!(
        text.ends_with("host: inherited by the harness, its content is not visible to aae\n"),
        "{text}"
    );

    let config = with_lines(&[("shop/aae.toml", "[settings]\ninherit_host = false")]);
    let text = effective(&config, None).explain();
    assert!(
        text.contains("inherit_host = false  # project\n") && text.ends_with("host: off\n"),
        "{text}"
    );
}

#[test]
fn unknown_project_rig_or_agent_is_an_error() {
    let config = load_with(&[]).unwrap();
    assert!(config.effective("nope", None, &implementer()).is_err());
    assert!(config.effective("shop", Some("frontend"), &implementer()).is_err());
    let nobody = Name {
        pack: "gascity".into(),
        name: "reviewer".into(),
    };
    assert!(config.effective("shop", None, &nobody).is_err());
}

#[test]
fn the_effective_config_serializes_as_the_run_snapshot() {
    let e = effective(&load_with(&[]).unwrap(), None);
    let snapshot = serde_json::to_value(&e).unwrap();
    assert_eq!(
        snapshot["model"],
        json!({ "value": "sonnet", "from": { "layer": "harness" } })
    );
    assert_eq!(
        snapshot["skills"]["mail"]["from"],
        json!({ "layer": "plugin", "name": "mail" })
    );
}

#[test]
fn a_fingerprint_depends_on_the_process_part_not_on_key_order() {
    let a = ProcessPart(serde_json::from_str(r#"{"model":"opus","env":{"A":"1","B":"2"}}"#).unwrap());
    let b = ProcessPart(serde_json::from_str(r#"{"env":{"B":"2","A":"1"},"model":"opus"}"#).unwrap());
    assert_eq!(Fingerprint::of("claude", &a), Fingerprint::of("claude", &b));
    assert_ne!(Fingerprint::of("claude", &a), Fingerprint::of("codex", &a));
    let c = ProcessPart(json!({ "model": "sonnet", "env": { "A": "1", "B": "2" } }));
    assert_ne!(Fingerprint::of("claude", &a), Fingerprint::of("claude", &c));
    assert_eq!(Fingerprint::of("claude", &a).0.len(), 64);
}

#[test]
fn an_attached_plugin_puts_its_bin_ahead_of_path_and_its_prompt_after_the_system_prompt() {
    let mail = "plugins/mail/plugin.toml";
    let manifest = "name = \"mail\"\nversion = \"0.1.0\"\nprompt = \"prompt.md\"\n";
    let two = BASE[0]
        .1
        .replace("[settings]", "[[plugin]]\npath = \"plugins/post\"\n[settings]");
    let config = load_with(&[
        ("config.toml", two.as_str()),
        (mail, manifest),
        ("plugins/mail/prompt.md", "Check the mail first.\n"),
        ("plugins/mail/bin/mail-send", ""),
        ("plugins/post/plugin.toml", "name = \"post\"\nversion = \"0.1.0\"\n"),
        ("plugins/post/bin/post-send", ""),
        (
            "gascity/agents/implementer/agent.toml",
            "harness = \"claude\"\nsystem_prompt = \"Be brief.\"\nenv = { PATH = \"/usr/bin\" }\n\
             plugins = [{ name = \"mail\" }, { name = \"post\" }]\n",
        ),
    ])
    .unwrap();
    let e = effective(&config, None);
    let both = Layer::Merged(vec![Layer::Agent, Layer::Plugin("mail".into())]);
    let prompt = e.system_prompt.as_ref().unwrap();
    assert_eq!(
        (prompt.value.as_str(), &prompt.from),
        ("Be brief.\n\nCheck the mail first.", &both)
    );
    let path = &e.env["PATH"];
    let bin = |name: &str| {
        let plugin = config.plugins.iter().find(|p| p.name == name).unwrap();
        plugin.root.join("bin").display().to_string()
    };
    assert_eq!(path.value, format!("{}:{}:/usr/bin", bin("mail"), bin("post")));
    let three = [Layer::Agent, Layer::Plugin("mail".into()), Layer::Plugin("post".into())];
    assert_eq!(path.from, Layer::Merged(three.to_vec()));
    assert!(
        e.explain().contains("first.\"  # agent + plugin mail\n"),
        "{}",
        e.explain()
    );
}

#[test]
fn native_tables_merge_by_key_across_the_layers() {
    let config = with_lines(&[
        ("config.toml", "[settings.native.claude]\na = 1\nb = 2"),
        ("shop/aae.toml", "[settings.native.claude]\nb = 3"),
    ]);
    let native = effective(&config, None).native.unwrap();
    assert_eq!(native.value, json!({ "a": 1, "b": 3 }));
    assert_eq!(native.from, Layer::Merged(vec![Layer::Orchestrator, Layer::Project]));
    let config = with_lines(&[("shop/aae.toml", "[settings.native]\nclaude = \"plain\"")]);
    assert_eq!(
        effective(&config, None).native.unwrap().value,
        json!("plain"),
        "not a table: replaced"
    );
}

#[test]
fn a_project_builtin_table_replaces_the_orchestrators_for_its_runs() {
    let config = with_lines(&[
        ("config.toml", "[builtin.worktree]\nagents = [\"implementer\"]"),
        (
            "shop/aae.toml",
            "[builtin.worktree]\nenabled = false\n[builtin.default-policy]\nrapid_s = 5",
        ),
    ]);
    let names = |b: &[crate::config::Bundled]| b.iter().map(|b| (b.name, b.params.to_string())).collect::<Vec<_>>();
    assert_eq!(
        names(&config.bundled),
        [
            ("default-policy", "{}".into()),
            ("worktree", r#"{"agents":["implementer"]}"#.into())
        ]
    );
    assert_eq!(
        names(&config.projects[0].bundled),
        [("default-policy", r#"{"rapid_s":5}"#.to_owned())]
    );
}
