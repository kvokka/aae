//! An addon's parameters, its attachment on every layer and its declaration (docs/design/config.md "Addon").

use super::config::{BASE, fails, load_with};
use crate::config::{Checkouts, Config, GitSource, Layer, Name, load_for};
use serde_json::json;
use std::fs;

const MAIL: &str = "addons/mail/addon.toml";

/// `BASE` with `text` appended to each of the given files.
fn with(lines: &[(&str, &str)]) -> Vec<(&'static str, String)> {
    (lines.iter())
        .map(|(path, line)| {
            let (path, base) = BASE.iter().find(|(p, _)| p == path).unwrap();
            (*path, format!("{base}{line}\n"))
        })
        .collect()
}

fn load(changed: &[(&str, String)]) -> Config {
    let changes: Vec<(&str, &str)> = changed.iter().map(|(p, t)| (*p, t.as_str())).collect();
    load_with(&changes).unwrap()
}

fn implementer() -> Name {
    Name {
        pack: "kit".into(),
        name: "implementer".into(),
    }
}

/// The addon `mail` with four params and the agent side that names them.
const PARAMS: &str = "params = { level = 1, mode = \"p\", strict = false, keep = \"k\" }\n\
    [agent.env]\nMAIL = \"${params.mode}-${params.level}\"\nDATA = \"${AAE_ADDON_DATA}/box\"\n\
    [agent.native.claude]\nwindow = \"${params.level}\"\nnote = \"level ${params.level}\"\n\
    [[service]]\nname = \"inbox\"\ncommand = [\"inbox\", \"--strict=${params.strict}\"]";

/// Every layer gives a value of its own: each param comes from the strongest layer that names it.
fn layered() -> Config {
    let mut changed = with(&[
        (
            "config.toml",
            "addons = [{ name = \"mail\", params = { level = 2, mode = \"o\" } }]",
        ),
        (
            "shop/aae.toml",
            "[settings]\naddons = [{ name = \"mail\", params = { level = 3 } }]\n\
            [rigs.backend.settings]\naddons = [{ name = \"mail\", params = { mode = \"r\" } }]",
        ),
    ]);
    let edit = "[agents.implementer]\naddons = [{ name = \"mail\", params = { level = 6 } }]\n";
    changed[0].1 += edit;
    let mail = BASE.iter().find(|(p, _)| *p == MAIL).unwrap().1;
    // `params` is a top-level key: it goes before the first table.
    changed.push((
        MAIL,
        mail.replace("[agent.skills.mail]", &format!("{PARAMS}\n[agent.skills.mail]")),
    ));
    let agent = "kit/agents/implementer/agent.toml";
    let text = BASE.iter().find(|(p, _)| *p == agent).unwrap().1;
    changed.push((
        agent,
        text.replace("{ name = \"mail\" }", "{ name = \"mail\", params = { strict = true } }"),
    ));
    load(&changed)
}

#[test]
fn each_param_comes_from_the_strongest_layer_that_gives_it() {
    let mut config = layered();
    let on = |rig| {
        let e = config.effective("shop", rig, &implementer()).unwrap();
        let params = &e.addons[0].value.params;
        (params.iter())
            .map(|(k, s)| (k.clone(), s.value.clone(), s.from.clone()))
            .collect::<Vec<_>>()
    };
    let addon = Layer::Addon("mail".into());
    assert_eq!(
        on(Some("backend")),
        vec![
            ("keep".into(), json!("k"), addon.clone()),
            ("level".into(), json!(6), Layer::OrchestratorAgent),
            ("mode".into(), json!("r"), Layer::Rig),
            ("strict".into(), json!(true), Layer::Agent),
        ]
    );
    assert_eq!(on(None)[2], ("mode".into(), json!("o"), Layer::Orchestrator));

    // The values reach every string of the agent side, a `native` string that is one reference with its type.
    config.addon_data = Some("/state/addons".into());
    let e = config.effective("shop", Some("backend"), &implementer()).unwrap();
    assert_eq!(e.env["MAIL"].value, "r-6");
    assert_eq!(e.env["DATA"].value, "/state/addons/mail/box");
    let native = &e.native.as_ref().unwrap().value;
    assert_eq!((&native["window"], &native["note"]), (&json!(6), &json!("level 6")));
    assert_eq!(e.services["inbox"].value.command, ["inbox", "--strict=true"]);

    let explain = e.explain();
    for line in [
        "addons.mail.params.level = 6  # orchestrator [agents]",
        "addons.mail.params.mode = \"r\"  # rig",
        "addons.mail.params.keep = \"k\"  # addon mail",
    ] {
        assert!(explain.contains(line), "no `{line}` in:\n{explain}");
    }
}

#[test]
fn an_attachment_keeps_the_place_of_its_first_mention() {
    let news = "name = \"news\"\nversion = \"0.1.0\"\n";
    let mut changed = with(&[
        (
            "config.toml",
            "addons = [{ name = \"news\" }]\n[[addon]]\npath = \"addons/news\"",
        ),
        (
            "shop/aae.toml",
            "[settings]\naddons = [{ name = \"mail\" }, { name = \"news\", enabled = false }]",
        ),
    ]);
    changed.push(("addons/news/addon.toml", news.to_owned()));
    let config = load(&changed);
    let names = |c: &Config| {
        let e = c.effective("shop", None, &implementer()).unwrap();
        e.addons.iter().map(|p| p.value.name.clone()).collect::<Vec<_>>()
    };
    // `news` is off from the project on; the agent attaches `mail` again.
    assert_eq!(names(&config), ["mail"]);
    let agent = "kit/agents/implementer/agent.toml";
    let text = BASE.iter().find(|(p, _)| *p == agent).unwrap().1;
    changed.push((
        agent,
        text.replace("{ name = \"mail\" }", "{ name = \"mail\" }, { name = \"news\" }"),
    ));
    assert_eq!(names(&load(&changed)), ["news", "mail"]);
}

#[test]
fn a_param_is_declared_with_its_type() {
    let mail = BASE.iter().find(|(p, _)| *p == MAIL).unwrap().1;
    let declared = |extra: &str| mail.replace("[agent.skills.mail]", &format!("{extra}\n[agent.skills.mail]"));
    let agent = |params: &str| {
        let text = BASE
            .iter()
            .find(|(p, _)| *p == "kit/agents/implementer/agent.toml")
            .unwrap()
            .1;
        text.replace(
            "{ name = \"mail\" }",
            &format!("{{ name = \"mail\", params = {params} }}"),
        )
    };
    let level = declared("params = { level = 1 }");
    let at = "kit/agents/implementer/agent.toml";
    for (params, error) in [
        (
            "{ nope = 1 }",
            "addon `mail`: param `nope` is not in the addon's `[params]`",
        ),
        ("{ level = \"x\" }", "addon `mail`: param `level` is of type integer"),
        ("{ level = true }", "addon `mail`: param `level` is of type integer"),
        ("{ level = 1.5 }", "addon `mail`: param `level` is of type integer"),
    ] {
        fails(&[(MAIL, level.as_str()), (at, agent(params).as_str())], error);
    }
    for default in ["1.5", "[1]", "{ a = 1 }"] {
        let text = declared(&format!("params = {{ level = {default} }}"));
        fails(
            &[(MAIL, text.as_str())],
            "param `level`: a default is a string, an integer or a boolean",
        );
    }
    let unknown = declared("[agent.env]\nX = \"${params.nope}\"");
    fails(
        &[(MAIL, unknown.as_str())],
        "`${params.nope}`: no param `nope` in `[params]`",
    );
}

#[test]
fn a_addon_gives_its_agents_only_env_mcp_hooks_skills_and_native() {
    let mail = BASE.iter().find(|(p, _)| *p == MAIL).unwrap().1;
    for key in [
        "model = \"opus\"",
        "harness = \"claude\"",
        "addons = []",
        "system_prompt = \"x\"",
    ] {
        let text = mail.replace("[agent.skills.mail]", &format!("[agent]\n{key}\n[agent.skills.mail]"));
        let field = key.split(' ').next().unwrap();
        fails(&[(MAIL, text.as_str())], &format!("unknown field `{field}`"));
    }
}

#[test]
fn a_addon_for_other_harnesses_is_refused() {
    let mail = BASE.iter().find(|(p, _)| *p == MAIL).unwrap().1;
    let codex = mail.replace("version = \"0.1.0\"", "version = \"0.1.0\"\nharnesses = [\"codex\"]");
    fails(
        &[(MAIL, codex.as_str())],
        "addon `mail` is for codex, not for harness `claude`",
    );
    let both = mail.replace(
        "version = \"0.1.0\"",
        "version = \"0.1.0\"\nharnesses = [\"codex\", \"claude\"]",
    );
    load_with(&[(MAIL, both.as_str())]).unwrap();
}

#[test]
fn a_declaration_names_only_a_source() {
    let config = BASE.iter().find(|(p, _)| *p == "config.toml").unwrap().1;
    let declared = |extra: &str| config.replace("path = \"addons/mail\"", &format!("path = \"addons/mail\"\n{extra}"));
    let retired = declared("config = { level = 2 }");
    fails(
        &[("config.toml", retired.as_str())],
        "line 6, column 1: a `[[addon]]` has no `config`: its params are given where it is attached",
    );
    let dir = declared("dir = \"mail\"");
    fails(&[("config.toml", dir.as_str())], "`dir` is for an addon from git");
}

#[test]
fn a_addon_from_git_may_live_in_a_directory_of_the_repository() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let git = "https://example.com/addons.git";
    let declare = |sub: &str| format!("git = \"{git}\"\nrev = \"v1\"\ndir = \"{sub}\"");
    for (path, text) in BASE {
        let path = path
            .strip_prefix("addons/")
            .map_or_else(|| path.to_string(), |p| format!("dl/{p}"));
        let path = d.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let checkouts = || {
        let source = GitSource {
            git: git.to_owned(),
            rev: Some("v1".to_owned()),
        };
        Checkouts::from([(source, Ok(d.join("dl")))])
    };
    let file = d.join("config.toml");
    let config = fs::read_to_string(&file).unwrap();
    for (sub, error) in [
        ("mail", None),
        ("../dl/mail", Some("is not a path inside the repository")),
    ] {
        fs::write(&file, config.replace("path = \"addons/mail\"", &declare(sub))).unwrap();
        match (load_for(&file, &[], checkouts()), error) {
            (Ok(c), None) => assert_eq!(c.addons[0].root, d.join("dl/mail")),
            (Err(e), Some(error)) => assert!(e.iter().any(|e| e.message.contains(error)), "{e:?}"),
            (got, _) => panic!("dir `{sub}`: {got:?}"),
        }
    }
}
