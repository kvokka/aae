//! Tests may run inside a session or an exec step of an outer loop-manager (docs/agents/rules-tests.md): what it
//! exports must not reach the processes a test starts.

use std::process::Command;

/// Removes from `command` what a session or an exec step of an outer loop-manager exports and this process inherited:
/// every `LOOP_MANAGER_*` but `LOOP_MANAGER_INSTANCE` (the daemon under test never reads it, and the outer daemon finds
/// forgotten test daemons by it), and the tracker's `BEADS_DIR`, `BD_ACTOR`, `BD_ISSUE_PREFIX`. Called before the
/// test's own `env`, which it does not see.
pub fn without_outer_session(command: &mut Command) -> &mut Command {
    for (name, _) in std::env::vars_os() {
        let text = name.to_string_lossy();
        let outer = (text.starts_with("LOOP_MANAGER_") && text != "LOOP_MANAGER_INSTANCE")
            || matches!(&*text, "BEADS_DIR" | "BD_ACTOR" | "BD_ISSUE_PREFIX");
        if outer {
            command.env_remove(&name);
        }
    }
    command
}
