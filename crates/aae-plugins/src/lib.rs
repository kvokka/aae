//! The plugins bundled with aae (spec §11.5): in-process executors of the orchestrator hook contract
//! (docs/design/contracts.md 3.1). The core runs them as hooks of their events (`aae_core::config::BUNDLED`) with
//! their `[builtin.<name>]` table as parameters (`worktree`: the loader's); the daemon checks those at start.

mod policy;
mod worktree;

use serde::de::DeserializeOwned;
use serde_json::Value;

/// The hook of bundled plugin `name` on `event`: its decision, `None` for no decision, or why it failed.
pub fn run(name: &str, event: &Value, params: &Value) -> Result<Option<Value>, String> {
    match name {
        "default-policy" => Ok(policy::decide(event, &parse(name, params)?)),
        "worktree" => worktree::hook(event, &parse(name, params)?),
        _ => Err(format!("`{name}` is not a bundled plugin")),
    }
}

/// Checks the parameters of bundled plugin `name`.
pub fn check(name: &str, params: &Value) -> Result<(), String> {
    match name {
        "default-policy" => parse::<policy::Params>(name, params).map(drop),
        "worktree" => parse::<worktree::Params>(name, params).map(drop),
        _ => Err(format!("`{name}` is not a bundled plugin")),
    }
}

fn parse<T: DeserializeOwned>(name: &str, params: &Value) -> Result<T, String> {
    serde_json::from_value(params.clone()).map_err(|e| format!("[builtin.{name}]: {e}"))
}

#[cfg(test)]
mod tests;
