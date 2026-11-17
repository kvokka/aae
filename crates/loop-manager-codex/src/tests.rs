use super::*;

pub(crate) fn agent<T>(value: T) -> loop_manager_contract::config::Sourced<T> {
    loop_manager_contract::config::Sourced {
        value,
        from: loop_manager_contract::config::Layer::Agent,
    }
}

pub(crate) fn config() -> EffectiveConfig {
    EffectiveConfig {
        services: Default::default(),
        harness: "codex".into(),
        harness_from: loop_manager_contract::config::Layer::Agent,
        scope: Default::default(),
        description: None,
        subagents: None,
        targets: Default::default(),
        model_given: None,
        model: Some(agent("gpt-fake".into())),
        effort: Some(agent("low".into())),
        system_prompt: None,
        inherit_host: Some(agent(false)),
        env: [("OPENAI_API_KEY".to_owned(), agent("sk-fake".to_owned()))].into(),
        mcp: Default::default(),
        hooks: Default::default(),
        skills: Default::default(),
        addons: Vec::new(),
        native: Some(agent(
            json!({ "sandbox_mode": "danger-full-access", "approval_policy": "never",
            "model_providers": { "fake": { "base_url": "http://127.0.0.1:1/v1", "wire_api": "responses" } } }),
        )),
    }
}
