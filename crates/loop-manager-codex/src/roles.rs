//! The subagent targets of a session as agent roles of its thread (docs/design/contracts.md 4.6,
//! docs/design/experiments.md "codex subagent roles"): codex takes a role only from a file, which the adapter writes
//! by its content under the state directory.

use loop_manager_contract::{
    config::EffectiveConfig,
    driver::{Failure, Fingerprint, ProcessPart, child::config_error},
};
use serde_json::{Map, Value, json};
use std::path::Path;

/// `config.agents` of a thread and the role files it names, their text by path.
type Roles = (Map<String, Value>, Map<String, Value>);

/// The roles of the targets of `config`: `config.agents` of its thread, over the `agents` table of the native block,
/// which it replaces; and the role files.
pub(crate) fn of(config: &EffectiveConfig, state: &Path) -> Result<Roles, Failure> {
    let mut files = Map::new();
    if config.targets.is_empty() {
        return Ok((Map::new(), files));
    }
    let native = config.native.as_ref().and_then(|n| n.value.get("agents"));
    let mut agents = native.and_then(Value::as_object).cloned().unwrap_or_default();
    let skills = super::part::skills(config);
    for (name, t) in &config.targets {
        let text = |s: &Option<loop_manager_contract::config::Sourced<String>>| s.as_ref().map(|s| s.value.clone());
        let mut role = toml::Table::new();
        // An empty one drops the role: a target without a role prompt keeps the session's instructions.
        let instructions = text(&t.system_prompt).filter(|p| !p.trim().is_empty());
        let keys = [
            ("developer_instructions", instructions),
            ("model", text(&t.model)),
            ("model_reasoning_effort", text(&t.effort)),
        ];
        for (key, value) in keys {
            if let Some(value) = value {
                role.insert(key.to_owned(), value.into());
            }
        }
        // A role only turns skills off: those of the process its target has not.
        let has = |k: &String| t.skills.get(k).is_some_and(|s| s.value.enabled);
        let off: Vec<toml::Value> = (skills.iter())
            .filter(|(k, _)| !has(k))
            .map(|(_, path)| {
                let path = path.join("SKILL.md").display().to_string();
                toml::Value::Table(toml::Table::from_iter([
                    ("path".to_owned(), path.into()),
                    ("enabled".to_owned(), false.into()),
                ]))
            })
            .collect();
        if !off.is_empty() {
            let config = toml::Table::from_iter([("config".to_owned(), toml::Value::Array(off))]);
            role.insert("skills".to_owned(), toml::Value::Table(config));
        }
        let role = toml::to_string(&role).map_err(|e| config_error(format!("subagent `{name}`: {e}")))?;
        let hash = &Fingerprint::of("codex", &ProcessPart(json!(role))).0[..16];
        let path = state.join("codex").join("roles").join(format!("{hash}.toml"));
        let description = text(&t.description).unwrap_or_default();
        agents.insert(name.clone(), json!({ "description": description, "config_file": path }));
        files.insert(path.display().to_string(), json!(role));
    }
    Ok((agents, files))
}

/// The thread parameters of session part `part`, the role files it names written first.
pub(crate) fn write(part: &Value) -> Result<Value, Failure> {
    let mut params = part.clone();
    let Some(Value::Object(files)) = params.as_object_mut().and_then(|p| p.remove("roles")) else {
        return Ok(params);
    };
    for (path, text) in files {
        let path = Path::new(&path);
        if path.exists() {
            continue;
        }
        let io = |e: std::io::Error| config_error(format!("{}: {e}", path.display()));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        // Whole or not at all: a thread of another session may read it as it spawns.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text.as_str().unwrap_or_default()).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)?;
    }
    Ok(params)
}
