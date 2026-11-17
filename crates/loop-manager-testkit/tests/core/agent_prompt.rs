//! A run gets the template of its agent, and a project rewrites the agent of a pack with files of its own
//! (docs/design/config.md, criterion 33), on the scripted driver.

use crate::common;

use common::{
    beads::{FIX, slow_turn},
    world,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn a_run_gets_the_project_template_of_its_agent_filled_with_the_project_file() {
    let w = world(&[
        (
            "shop/.loop-manager/agents/dev/prompt.template.md",
            "Build {{ rig.name }} so that:\n{{ agent.checks }}",
        ),
        ("shop/.loop-manager/agents/dev/checks.md", "- it compiles\n"),
    ]);
    w.driver.script(slow_turn());
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.driver.sent(&run, 1).1, ["Build web so that:\n- it compiles\n"]);
}

/// The `agent:` label of a bead names the agent of its run (docs/design/tracker-br.md), and with it the template the
/// run renders.
#[tokio::test(start_paused = true)]
async fn a_run_of_a_bead_labelled_with_an_agent_gets_that_agent_template() {
    let qa = [
        ("shop/agents/qa/agent.toml", "harness = \"scripted\"\n"),
        ("shop/agents/qa/prompt.template.md", "Check {{ rig.name }} as qa."),
    ];
    let w = world(&[FIX, &qa].concat());
    let bead = w.tracker.add(&["agent:qa"], None);
    w.changed().await;
    w.driver.script(slow_turn());
    let run = w.run_on(json!(bead)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.driver.sent(&run, 1).1, ["Check web as qa."]);
}
