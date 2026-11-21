//! `[[service]]` in the orchestrator config, a project and an addon (docs/design/config.md).

use super::config::{BASE, fails, load_with};
use crate::config::{Config, Health, Layer, Name, Service, ServiceScope};

/// `BASE` with `text` appended to each of the given files.
fn with(lines: &[(&str, &str)]) -> Result<Config, Vec<crate::config::ConfigError>> {
    let changed: Vec<(&str, String)> = (lines.iter())
        .map(|(path, text)| {
            (
                *path,
                format!("{}{text}\n", BASE.iter().find(|(p, _)| p == path).unwrap().1),
            )
        })
        .collect();
    load_with(&changed.iter().map(|(p, t)| (*p, t.as_str())).collect::<Vec<_>>())
}

fn agent(name: &str) -> Name {
    Name {
        pack: "kit".into(),
        name: name.into(),
    }
}

#[test]
fn services_layer_like_settings_and_guess_their_paths() {
    let config = with(&[
        (
            "config.toml",
            "[[service]]\nname = \"mail\"\ncommand = [\"mail-d\", \"${LOOP_MANAGER_SERVICE_PORT}\"]\nmcp = true\n\
             health = \"http\"\n[[service]]\nname = \"search\"\ncommand = [\"search-d\"]",
        ),
        (
            "shop/loop-manager.toml",
            "[[service]]\nname = \"mail\"\ncommand = [\"shop-mail\"]\nscope = \"project\"\nmcp = { url = \"/rpc\" }\n\
             health = { command = [\"probe\"] }",
        ),
        (
            "addons/mail/addon.toml",
            "[[service]]\nname = \"inbox\"\ncommand = [\"${LOOP_MANAGER_ADDON_DIR}/bin/inbox\"]\nhealth = { http = \"/up\" }\n\
             [[service]]\nname = \"sorter\"\ncommand = [\"sorter\"]\nenv = { RULES = \"${LOOP_MANAGER_ADDON_DIR}/rules\" }\n\
             health = { command = [\"${LOOP_MANAGER_ADDON_DIR}/bin/ok\"] }",
        ),
    ])
    .unwrap();
    let e = config.effective("shop", None, &agent("implementer")).unwrap();
    let mail = &e.services["mail"];
    let expected = Service {
        command: vec!["shop-mail".into()],
        env: Default::default(),
        scope: ServiceScope::Project,
        bind: String::new(),
        health: Health::Command(vec!["probe".into()]),
        mcp: Some("/rpc".into()),
    };
    assert_eq!((&mail.value, &mail.from), (&expected, &Layer::Project));
    let search = &e.services["search"].value;
    assert_eq!(
        (&search.health, &search.mcp, search.scope),
        (&Health::Port, &None, ServiceScope::Orchestrator)
    );
    let inbox = &e.services["inbox"];
    assert_eq!(inbox.from, Layer::Addon("mail".into()));
    assert!(
        inbox.value.command[0].ends_with("addons/mail/bin/inbox"),
        "{:?}",
        inbox.value.command
    );
    assert_eq!(inbox.value.health, Health::Http("/up".into()));
    // Its health check and environment reach its files too.
    let sorter = &e.services["sorter"].value;
    assert!(sorter.env["RULES"].ends_with("addons/mail/rules"), "{sorter:?}");
    let Health::Command(check) = &sorter.health else {
        panic!("{sorter:?}")
    };
    assert!(check[0].ends_with("addons/mail/bin/ok"), "{check:?}");
    assert!(e.explain().contains("service.inbox = "), "{}", e.explain());

    // The orchestrator's own: the guessed paths.
    let config = with(&[(
        "config.toml",
        "[[service]]\nname = \"mail\"\ncommand = [\"mail-d\"]\nmcp = true\nhealth = \"http\"",
    )])
    .unwrap();
    let e = config.effective("shop", None, &agent("implementer")).unwrap();
    let mail = &e.services["mail"].value;
    assert_eq!(
        (&mail.health, mail.mcp.as_deref()),
        (&Health::Http("/health".into()), Some("/mcp"))
    );
}

#[test]
fn a_bad_service_is_a_config_error() {
    let service = |rest: &str| format!("{}[[service]]\nname = \"s\"\n{rest}\n", BASE[0].1);
    for (rest, needle) in [
        (
            "command = [\"x\"]\nscope = \"session\"",
            "expected `orchestrator` or `project`",
        ),
        ("command = []", "service `s`: a command is empty"),
        (
            "command = [\"x\"]\nhealth = { command = [] }",
            "service `s`: a command is empty",
        ),
        (
            "command = [\"x\"]\nmcp = { url = \"mcp\" }",
            "service `s`: a path starts with `/`",
        ),
        (
            "command = [\"x\"]\nhealth = { http = \"up\" }",
            "service `s`: a path starts with `/`",
        ),
        ("command = [\"x\"]\nhealth = \"tcp\"", "`health` is"),
        ("command = [\"x\"]\nport = 1", "unknown field `port`"),
        ("command = [\"x\"]\nbind = \"0.0.0.0:0\"", "bind must be"),
        (
            "command = [\"x\"]\n[[service]]\nname = \"s\"\ncommand = [\"y\"]",
            "two services are named `s`",
        ),
    ] {
        fails(&[("config.toml", service(rest).as_str())], needle);
    }
}

#[test]
fn a_addon_names_its_own_files_in_every_value_of_its_agent_layer() {
    let config = with(&[(
        "addons/mail/addon.toml",
        "[agent.env]\nMAIL_HOME = \"${LOOP_MANAGER_ADDON_DIR}/data\"\n\
         [agent.mcp.m]\ncommand = [\"${LOOP_MANAGER_ADDON_DIR}/bin/m\"]\nenv = { X = \"${LOOP_MANAGER_ADDON_DIR}/x\" }\n\
         [agent.hooks.h]\nevent = \"Stop\"\ncommand = \"${LOOP_MANAGER_ADDON_DIR}/bin/h\"\n\
         [agent.native.claude]\nstatusLine = { command = \"${LOOP_MANAGER_ADDON_DIR}/bin/s\" }",
    )])
    .unwrap();
    let e = config.effective("shop", None, &agent("implementer")).unwrap();
    let dir = config
        .addons
        .iter()
        .find(|p| p.name == "mail")
        .unwrap()
        .root
        .display()
        .to_string();
    let mcp = &e.mcp["m"].value;
    let native = &e.native.as_ref().unwrap().value;
    let values = [
        e.env["MAIL_HOME"].value.as_str(),
        mcp.command.as_ref().unwrap()[0].as_str(),
        mcp.env["X"].as_str(),
        e.hooks["h"].value.command.as_deref().unwrap(),
        native["statusLine"]["command"].as_str().unwrap(),
    ];
    assert!(
        values.iter().all(|v| v.starts_with(&dir) && !v.contains('$')),
        "{values:?}"
    );
}
