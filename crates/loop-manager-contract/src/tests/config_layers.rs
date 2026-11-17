use super::config::{BASE, fails, load_with};
use crate::config::{Config, EffectiveConfig, Layer, Name, Watchdog};
use crate::driver::{Fingerprint, ProcessPart};
use serde_json::json;

fn implementer() -> Name {
    Name {
        pack: "kit".into(),
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
            "effort = \"low\"\n[agents.implementer]\ninherit_host = false",
        ),
        (
            "shop/loop-manager.toml",
            "[settings]\nenv = { LANG = \"en\", PROJECT = \"shop\" }",
        ),
    ]);
    let e = effective(&config, None);
    let model = e.model.as_ref().unwrap();
    assert_eq!((model.value.as_str(), &model.from), ("sonnet", &Layer::Harness));
    // The agent's `high` beats the orchestrator's `low`; an `[agents]` edit beats the agent.
    assert_eq!(e.effort.as_ref().unwrap().from, Layer::Agent);
    assert_eq!(e.inherit_host.as_ref().unwrap().from, Layer::OrchestratorAgent);
    assert_eq!(
        (e.env["LANG"].value.as_str(), &e.env["LANG"].from),
        ("en", &Layer::Project)
    );
    assert_eq!(e.env.len(), 2, "the session environment holds only what the layers say");
    // The rig edit of `BASE` applies only to runs on that rig.
    let on_rig = effective(&config, Some("backend"));
    assert_eq!(on_rig.model.as_ref().unwrap().value, "opus");
    assert_eq!(on_rig.model.as_ref().unwrap().from, Layer::RigAgent);
}

#[test]
fn the_tracker_variables_come_under_every_layer() {
    let mut config = with_lines(&[("shop/loop-manager.toml", "[settings]\nenv = { TRACKER = \"mine\" }")]);
    let shop = config.projects.iter_mut().find(|p| p.name == "shop").unwrap();
    for (name, value) in [("TRACKER", "theirs"), ("TRACKER_DIR", "/shop/.tracker")] {
        shop.tracker.env.insert(name.to_owned(), value.to_owned());
    }
    let e = effective(&config, None);
    let env = |name: &str| (e.env[name].value.as_str(), &e.env[name].from);
    assert_eq!(env("TRACKER_DIR"), ("/shop/.tracker", &Layer::Project));
    assert_eq!(env("TRACKER"), ("mine", &Layer::Project), "a layer may still set it");
    // A session on a rig files beads with the rig's prefix; in the root, or of an agent of the root, without.
    assert!(!e.env.contains_key("PREFIX"));
    let shop = config.projects.iter_mut().find(|p| p.name == "shop").unwrap();
    shop.tracker.prefix = Some("PREFIX".to_owned());
    let on_rig = effective(&config, Some("backend"));
    assert_eq!(
        (on_rig.env["PREFIX"].value.as_str(), &on_rig.env["PREFIX"].from),
        ("b", &Layer::Project)
    );
    assert!(!effective(&config, None).env.contains_key("PREFIX"));
}

#[test]
fn an_attached_addon_contributes_and_a_stronger_layer_can_switch_it_off() {
    let config = load_with(&[]).unwrap();
    let e = effective(&config, None);
    assert_eq!(e.addons[0].from, Layer::Agent);
    assert_eq!(e.skills["mail"].from, Layer::Addon("mail".into()));

    let config = with_lines(&[(
        "shop/loop-manager.toml",
        "[agents.implementer.skills.mail]\nenabled = false",
    )]);
    let e = effective(&config, None);
    assert!(e.skills.is_empty(), "a skill switched off by name is gone");
    assert_eq!(e.addons.len(), 1);

    let config = with_lines(&[(
        "shop/loop-manager.toml",
        "[agents.implementer]\naddons = [{ name = \"mail\", enabled = false }]",
    )]);
    let e = effective(&config, None);
    assert!(
        e.addons.is_empty() && e.skills.is_empty(),
        "a detached addon contributes nothing"
    );
}

#[test]
fn explain_names_the_layer_of_every_value_and_the_host_in_one_line() {
    let config = load_with(&[]).unwrap();
    let text = effective(&config, Some("backend")).explain();
    assert!(text.contains("model = \"opus\"  # rig [agents]\n"), "{text}");
    assert!(text.contains("env.LANG = \"C\"  # orchestrator\n"), "{text}");
    assert!(text.contains("skills.mail = "), "{text}");
    // The role prompt file is the agent layer's system prompt.
    assert!(text.contains("system_prompt = \"You implement.\"  # agent\n"), "{text}");
    assert!(
        text.ends_with("host: inherited by the harness, its content is not visible to loop-manager\n"),
        "{text}"
    );

    let config = with_lines(&[("shop/loop-manager.toml", "[settings]\ninherit_host = false")]);
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
        pack: "kit".into(),
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
        json!({ "layer": "addon", "name": "mail" })
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
fn an_attached_addon_puts_its_bin_ahead_of_path_and_its_prompt_after_the_system_prompt() {
    let mail = "addons/mail/addon.toml";
    let manifest = "name = \"mail\"\nversion = \"0.1.0\"\nprompt = \"prompt.md\"\n";
    let two = BASE[0]
        .1
        .replace("[settings]", "[[addon]]\npath = \"addons/post\"\n[settings]");
    let config = load_with(&[
        ("config.toml", two.as_str()),
        (mail, manifest),
        ("addons/mail/prompt.md", "Check the mail first.\n"),
        ("addons/mail/bin/mail-send", ""),
        ("addons/post/addon.toml", "name = \"post\"\nversion = \"0.1.0\"\n"),
        ("addons/post/bin/post-send", ""),
        (
            "kit/agents/implementer/agent.toml",
            "harness = \"claude\"\nenv = { PATH = \"/usr/bin\" }\n\
             addons = [{ name = \"mail\" }, { name = \"post\" }]\n",
        ),
    ])
    .unwrap();
    let e = effective(&config, None);
    let both = Layer::Merged(vec![Layer::Agent, Layer::Addon("mail".into())]);
    let prompt = e.system_prompt.as_ref().unwrap();
    assert_eq!(
        (prompt.value.as_str(), &prompt.from),
        ("You implement.\n\nCheck the mail first.", &both)
    );
    let path = &e.env["PATH"];
    let bin = |name: &str| {
        let addon = config.addons.iter().find(|p| p.name == name).unwrap();
        addon.root.join("bin").display().to_string()
    };
    assert_eq!(path.value, format!("{}:{}:/usr/bin", bin("mail"), bin("post")));
    let three = [Layer::Agent, Layer::Addon("mail".into()), Layer::Addon("post".into())];
    assert_eq!(path.from, Layer::Merged(three.to_vec()));
    assert!(
        e.explain().contains("first.\"  # agent + addon mail\n"),
        "{}",
        e.explain()
    );
}

#[test]
fn native_tables_merge_by_key_across_the_layers() {
    let config = with_lines(&[
        ("config.toml", "[settings.native.claude]\na = 1\nb = 2"),
        ("shop/loop-manager.toml", "[settings.native.claude]\nb = 3"),
    ]);
    let native = effective(&config, None).native.unwrap();
    assert_eq!(native.value, json!({ "a": 1, "b": 3 }));
    assert_eq!(native.from, Layer::Merged(vec![Layer::Orchestrator, Layer::Project]));
    let config = with_lines(&[("shop/loop-manager.toml", "[settings.native]\nclaude = \"plain\"")]);
    assert_eq!(
        effective(&config, None).native.unwrap().value,
        json!("plain"),
        "not a table: replaced"
    );
}

#[test]
fn a_project_builtin_table_replaces_the_orchestrators_for_its_runs() {
    let config = with_lines(&[
        ("config.toml", "[builtin.default-policy]\nrapid_s = 9\nrapid_n = 2"),
        ("shop/loop-manager.toml", "[builtin.default-policy]\nrapid_s = 5"),
    ]);
    let names = |b: &[crate::config::Bundled]| b.iter().map(|b| (b.name, b.params.to_string())).collect::<Vec<_>>();
    assert_eq!(
        names(&config.bundled),
        [("default-policy", r#"{"rapid_n":2,"rapid_s":9}"#.to_owned())]
    );
    assert_eq!(
        names(&config.projects[0].bundled),
        [("default-policy", r#"{"rapid_s":5}"#.to_owned())]
    );
}

#[test]
fn the_watchdog_deadlines_come_from_the_orchestrator_file() {
    let defaults = Watchdog {
        silence_ms: 900_000,
        tool_silence_ms: 2_400_000,
        background_silence_ms: 7_200_000,
        start_timeout_ms: 120_000,
        queue_timeout_ms: 604_800_000,
        park_timeout_ms: 604_800_000,
    };
    assert_eq!(load_with(BASE).unwrap().watchdog, defaults);
    let all = "[watchdog]\nsilence = \"1ms\"\ntool_silence = \"2s\"\nbackground_silence = \"3m\"\n\
        start_timeout = \"5s\"\nqueue_timeout = \"6m\"\npark_timeout = \"8760h\"";
    let expected = Watchdog {
        silence_ms: 1,
        tool_silence_ms: 2_000,
        background_silence_ms: 180_000,
        start_timeout_ms: 5_000,
        queue_timeout_ms: 360_000,
        park_timeout_ms: 31_536_000_000,
    };
    assert_eq!(with_lines(&[("config.toml", all)]).watchdog, expected);
    let one = |line: &str| format!("{}[watchdog]\n{line}\n", BASE[0].1);
    fails(&[("config.toml", &one("stall = \"1m\""))], "unknown key `stall`");
    fails(
        &[("config.toml", &one("silence = \"0s\""))],
        "`silence` is from 1ms to 8760h",
    );
    fails(
        &[("config.toml", &one("park_timeout = \"8761h\""))],
        "`park_timeout` is from 1ms",
    );
    fails(
        &[("config.toml", &one("silence = \"5 minutes\""))],
        "`silence`: `5 minutes`",
    );
}
