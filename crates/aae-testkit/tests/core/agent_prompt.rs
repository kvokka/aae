//! A run gets the template of its agent, and a project rewrites the agent of a pack with files of its own (spec §8.1,
//! criterion 33), on the scripted driver.

use crate::common;

use common::{beads::slow_turn, world};
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn a_run_gets_the_project_template_of_its_agent_filled_with_the_project_file() {
    let w = world(&[
        (
            "shop/.aae/agents/dev/prompt.template.md",
            "Build {{ rig.name }} so that:\n{{ agent.checks }}",
        ),
        ("shop/.aae/agents/dev/checks.md", "- it compiles\n"),
    ]);
    w.driver.script(slow_turn());
    let run = w.create().await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(w.driver.sent(&run, 1).1, ["Build web so that:\n- it compiles\n"]);
}
