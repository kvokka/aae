//! What the loader refuses because nothing would read it or it would read wrong (docs/design/config.md).

use super::config::{BASE, fails, load_with};

const AGENT: &str = "kit/agents/reviewer/agent.toml";

#[test]
fn mcp_keys_of_the_other_kind_of_server_are_errors() {
    let agent = |mcp: &str| format!("harness = \"claude\"\n[mcp.m]\n{mcp}\n");
    for (mcp, error) in [
        (
            "url = \"http://x\"\nenv = { A = \"1\" }",
            "mcp `m` has `env` or `scope`",
        ),
        (
            "url = \"http://x\"\nscope = \"session\"",
            "mcp `m` has `env` or `scope`",
        ),
        ("command = [\"srv\"]\nheaders = { A = \"1\" }", "mcp `m` has `headers`"),
        ("command = []", "mcp `m` has an empty `command`"),
    ] {
        fails(&[(AGENT, agent(mcp).as_str())], error);
    }
    let good = "command = [\"srv\"]\nenv = { A = \"1\" }\nscope = \"session\"";
    assert!(load_with(&[(AGENT, agent(good).as_str())]).is_ok());
}

#[test]
fn a_harness_hook_on_an_unknown_event_is_an_error() {
    let hook = |event: &str| format!("harness = \"claude\"\n[hooks.h]\nevent = \"{event}\"\ncommand = \"true\"\n");
    fails(
        &[(AGENT, hook("PreTolUse").as_str())],
        "hook `h` has an unknown event `PreTolUse`",
    );
    assert!(load_with(&[(AGENT, hook("PreToolUse").as_str())]).is_ok());
}

#[test]
fn a_rig_var_may_not_hide_a_field_of_the_rig() {
    let project = BASE.iter().find(|(p, _)| *p == "shop/loop-manager.toml").unwrap().1;
    let hiding = project.replace("vars = {", "vars = { path = \"/elsewhere\",");
    fails(
        &[("shop/loop-manager.toml", hiding.as_str())],
        "rig `backend`: var `path` would hide `rig.path`",
    );
}

#[test]
fn a_formula_in_a_subdirectory_is_loaded() {
    let formula = "name = \"deep\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\ntimeout = \"1s\"\n";
    let config = load_with(&[("kit/formulas/more/deep.toml", formula)]).unwrap();
    let shop = config.projects.iter().find(|p| p.name == "shop").unwrap();
    assert!(shop.packs[1].formulas.contains_key("deep"));
}
