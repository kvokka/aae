//! Values of `env`: a literal, `${NAME}` for a variable of the daemon, `$$` for a `$`; and the layers of `[exec]`
//! (docs/design/config.md "Environment values", decision 137).

use super::config::{BASE, load_with};
use crate::config::{Config, ExecEnv, Layer, Name, StepKind, env_literal, env_value};

const FLOW: &str = "kit/formulas/flow.toml";

fn var(name: &str) -> Option<String> {
    match name {
        "X" => Some("x".into()),
        "Y_2" => Some("y".into()),
        _ => None,
    }
}

/// `BASE` with `lines` appended to each of the given files; a file `BASE` lacks starts empty.
fn appended(lines: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
    let base = |path: &str| BASE.iter().find(|(p, _)| *p == path).map_or("", |(_, t)| t);
    lines
        .iter()
        .map(|(path, line)| (*path, format!("{}{line}\n", base(path))))
        .collect()
}

fn load(lines: &[(&'static str, &str)]) -> Result<Config, Vec<crate::config::ConfigError>> {
    let changed = appended(lines);
    let changes: Vec<(&str, &str)> = changed.iter().map(|(p, t)| (*p, t.as_str())).collect();
    load_with(&changes)
}

fn flow(step: &str) -> (&'static str, String) {
    (
        FLOW,
        format!("name = \"flow\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n{step}"),
    )
}

fn exec_env(config: &Config, formula: &str) -> ExecEnv {
    let formula = (config.projects[0].packs.iter())
        .find_map(|p| p.formulas.get(formula))
        .unwrap();
    match &formula.steps[0].kind {
        StepKind::Exec { env, .. } => env.clone(),
        _ => panic!("an exec step"),
    }
}

/// `inherit_env`, and each key of `env`, with the layer that gave it.
type Layers<'a> = (Option<(bool, Layer)>, Vec<(&'a str, &'a str, Layer)>);

fn layers(env: &ExecEnv) -> Layers<'_> {
    let inherit = env.inherit_env.as_ref().map(|s| (s.value, s.from.clone()));
    let env = env
        .env
        .iter()
        .map(|(k, s)| (k.as_str(), s.value.as_str(), s.from.clone()));
    (inherit, env.collect())
}

#[test]
fn a_reference_takes_the_variable_and_two_dollars_are_one() {
    for (text, value) in [
        ("plain", "plain"),
        ("${X}", "x"),
        ("a${X}b${Y_2}", "axby"),
        ("$$5", "$5"),
        ("$${X}", "${X}"),
        ("$$$$", "$$"),
        ("", ""),
    ] {
        assert_eq!(env_value(text, var).as_deref(), Ok(value), "{text}");
    }
}

#[test]
fn a_dollar_that_is_no_reference_is_an_error() {
    for text in [
        "$", "a$b", "$HOME", "${", "${X", "${}", "${1X}", "${A-B}", "${A:-b}", "$$$",
    ] {
        let error = env_value(text, |_| Some(String::new())).unwrap_err();
        assert!(error.contains("a `$` is written `$$`"), "{text}: {error}");
    }
}

#[test]
fn a_missing_variable_is_named_and_the_value_is_not_quoted() {
    let error = env_value("secret-${X}-${NOPE}", var).unwrap_err();
    assert_eq!(error, "the daemon has no variable `NOPE`");
    assert!(!error.contains("secret"), "{error}");
}

#[test]
fn a_literal_comes_back_as_it_was() {
    for text in ["$", "${X}", "$$", "a$b", "plain", "/tmp/pl$ug"] {
        assert_eq!(env_value(&env_literal(text), |_| None).as_deref(), Ok(text));
    }
}

#[test]
fn exec_layers_merge_env_by_key_and_take_the_strongest_inherit_env() {
    let top = "[exec]\ninherit_env = true\nenv = { A = \"top\", B = \"top\", C = \"top\" }";
    let own = "[exec]\nenv = { B = \"own\", C = \"own\" }";
    let (step_path, step) = flow("env = { C = \"${X}\" }\n");
    let config = load(&[("config.toml", top), ("shop/aae.toml", own), (step_path, &step)]).unwrap();
    let formula = || Layer::Formula("flow".into());
    let (o, p) = (Layer::Orchestrator, Layer::Project);
    assert_eq!(
        layers(&exec_env(&config, "flow")),
        (
            Some((true, o.clone())),
            vec![
                ("A", "top", o.clone()),
                ("B", "own", p.clone()),
                ("C", "${X}", formula())
            ]
        )
    );
    // A step that sets nothing gets the project over the orchestrator, and so does a check script.
    let under = (
        Some((true, o.clone())),
        vec![
            ("A", "top", o.clone()),
            ("B", "own", p.clone()),
            ("C", "own", p.clone()),
        ],
    );
    assert_eq!(layers(&exec_env(&config, "audit")), under);
    assert_eq!(layers(&config.projects[0].exec), under);

    let own = "[exec]\ninherit_env = false";
    let config = load(&[("config.toml", top), ("shop/aae.toml", own), (step_path, &step)]).unwrap();
    assert_eq!(layers(&exec_env(&config, "flow")).0, Some((false, p.clone())));
    assert_eq!(layers(&config.projects[0].exec).0, Some((false, p)));
    let (_, step) = flow("inherit_env = true\n");
    let config = load(&[("shop/aae.toml", own), (step_path, &step)]).unwrap();
    assert_eq!(layers(&exec_env(&config, "flow")), (Some((true, formula())), vec![]));
    let config = load(&[]).unwrap();
    assert_eq!(exec_env(&config, "audit"), ExecEnv::default(), "nobody said: no layer");
}

#[test]
fn a_malformed_env_value_is_a_load_error_of_its_file_and_key() {
    let rejected = |path: &'static str, line: &str, needle: &str| {
        let errors = load(&[(path, line)]).expect_err(path);
        assert!(
            (errors.iter()).any(|e| e.file.ends_with(path) && e.message.contains(needle)),
            "no error of {path} mentions `{needle}`: {errors:?}"
        );
    };
    let form = "a `$` is written `$$`, a variable of the daemon `${NAME}`";
    rejected(
        "shop/aae.toml",
        "[settings]\nenv = { BAD = \"$HOME\" }",
        &format!("`env.BAD`: {form}"),
    );
    rejected(
        "kit/agents/implementer/agent.toml",
        "env = { BAD = \"$HOME\" }",
        "`env.BAD`: a `$`",
    );
    rejected(
        "config.toml",
        "[exec]\nenv = { BAD = \"${A-B}\" }",
        "`exec.env.BAD`: a `$`",
    );
    rejected(
        "shop/aae.toml",
        "[exec]\nenv = { BAD = \"$\" }",
        "`exec.env.BAD`: a `$`",
    );
    let (path, step) = flow("env = { BAD = \"${\" }\n");
    rejected(path, &step, "`env.BAD`: a `$`");
    rejected("config.toml", "[exec]\nshell = true", "unknown field `shell`");
}

#[test]
fn the_directory_of_a_plugin_with_a_dollar_stays_itself_in_env() {
    let plugin = "name = \"mail\"\nversion = \"0.1.0\"\n[agent]\nenv = { D = \"${AAE_PLUGIN_DIR}/x\" }\n\
        [agent.skills.mail]\npath = \"skills/mail\"\n[agent.hooks.h]\nevent = \"Stop\"\n\
        command = \"${AAE_PLUGIN_DIR}/h\"\n";
    let top = BASE[0].1.replace("plugins/mail", "plugins/ma$il");
    let config = load_with(&[
        ("config.toml", &top),
        ("plugins/ma$il/plugin.toml", plugin),
        ("plugins/ma$il/skills/mail/SKILL.md", "mail"),
    ])
    .unwrap();
    let dir = config.plugins[0].root.to_string_lossy().into_owned();
    assert!(dir.ends_with("ma$il"), "{dir}");
    let implementer = Name {
        pack: "kit".into(),
        name: "implementer".into(),
    };
    let e = config.effective("shop", None, &implementer).unwrap();
    assert_eq!(env_value(&e.env["D"].value, |_| None), Ok(format!("{dir}/x")));
    let hook = e.hooks["h"].value.command.as_deref();
    assert_eq!(
        hook,
        Some(format!("{dir}/h").as_str()),
        "a hook command is no env value"
    );
}

#[test]
fn explain_shows_the_reference() {
    let own = "[settings]\nenv = { GH_TOKEN = \"${OTHER}\" }\n[exec]\ninherit_env = true\nenv = { T = \"${X}\" }";
    let config = load(&[("shop/aae.toml", own), ("config.toml", "[exec]\nenv = { U = \"$$1\" }")]).unwrap();
    let implementer = Name {
        pack: "kit".into(),
        name: "implementer".into(),
    };
    let text = config.effective("shop", None, &implementer).unwrap().explain();
    assert!(text.contains("env.GH_TOKEN = \"${OTHER}\"  # project\n"), "{text}");
    assert_eq!(
        config.projects[0].exec.explain(),
        "exec.inherit_env = true  # project\nexec.env.T = \"${X}\"  # project\nexec.env.U = \"$$1\"  # orchestrator\n"
    );
}

#[test]
fn a_tracker_directory_with_a_dollar_stays_itself_in_env() {
    let mut config = load(&[]).unwrap();
    let dir = "/srv/sh$op/.beads".to_owned();
    (config.projects[0].tracker.env).insert("BEADS_DIR".into(), dir.clone());
    let implementer = Name {
        pack: "kit".into(),
        name: "implementer".into(),
    };
    let e = config.effective("shop", None, &implementer).unwrap();
    assert_eq!(env_value(&e.env["BEADS_DIR"].value, |_| None), Ok(dir.clone()));
    let (_, exec) = config.projects[0].tracker_env(None).next().unwrap();
    assert_eq!(env_value(&exec, |_| None), Ok(dir));
}

/// The `bin/` of a plugin and the daemon's own `PATH`, both with a `$`, reach `PATH` as they are. The daemon's
/// `PATH` is the test's: the test runs itself again with a `$` in it.
#[test]
fn a_dollar_in_a_plugin_bin_and_the_daemons_path_stays_itself() {
    const CHILD: &str = "AAE_TEST_DOLLAR_PATH";
    let name = "a_dollar_in_a_plugin_bin_and_the_daemons_path_stays_itself";
    if std::env::var_os(CHILD).is_none() {
        let path = format!("/no/su$ch:{}", std::env::var("PATH").unwrap_or_default());
        let test = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test, "--test-threads=1"])
            .env(CHILD, "1")
            .env("PATH", path)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let top = BASE[0].1.replace("plugins/mail", "plugins/ma$il");
    let config = load_with(&[
        ("config.toml", &top),
        ("plugins/ma$il/plugin.toml", BASE[1].1),
        ("plugins/ma$il/skills/mail/SKILL.md", "mail"),
        ("plugins/ma$il/bin/tool", ""),
    ])
    .unwrap();
    let bin = config.plugins[0].bin.clone().unwrap();
    let implementer = Name {
        pack: "kit".into(),
        name: "implementer".into(),
    };
    let e = config.effective("shop", None, &implementer).unwrap();
    let daemon = std::env::var("PATH").unwrap();
    assert!(daemon.starts_with("/no/su$ch:"), "{daemon}");
    let path = env_value(&e.env["PATH"].value, |_| None);
    assert_eq!(path, Ok(format!("{}:{daemon}", bin.display())));
}
