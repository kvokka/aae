use super::*;

fn agent<T>(value: T) -> aae_contract::config::Sourced<T> {
    aae_contract::config::Sourced {
        value,
        from: aae_contract::config::Layer::Agent,
    }
}

fn config(native: Option<Value>) -> EffectiveConfig {
    EffectiveConfig {
        services: Default::default(),
        harness: "claude".into(),
        harness_from: aae_contract::config::Layer::Agent,
        scope: Default::default(),
        description: None,
        subagents: None,
        targets: Default::default(),
        model_given: None,
        model: None,
        effort: None,
        system_prompt: None,
        inherit_host: None,
        env: Default::default(),
        mcp: Default::default(),
        hooks: Default::default(),
        skills: Default::default(),
        plugins: Vec::new(),
        native: native.map(agent),
    }
}

fn process(resume: bool, inherit: bool) -> Value {
    json!({ "session_id": "s1", "resume": resume, "inherit_host": inherit, "model": "claude-haiku-5-5",
        "effort": null, "system_prompt": "be brief", "permission_mode": null })
}

#[test]
fn every_flag_goes_again_on_a_resume_and_isolation_cuts_the_host() {
    let new = part::args(&process(false, true));
    let resumed = part::args(&process(true, false));
    let pair = |args: &[String], flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].clone());
    assert_eq!(pair(&new, "--session-id").as_deref(), Some("s1"));
    assert_eq!(pair(&resumed, "--resume").as_deref(), Some("s1"));
    for args in [&new, &resumed] {
        assert_eq!(pair(args, "--model").as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(pair(args, "--append-system-prompt").as_deref(), Some("be brief"));
    }
    assert_eq!(pair(&new, "--setting-sources"), None, "the host is inherited");
    assert_eq!(pair(&resumed, "--setting-sources").as_deref(), Some("project,local"));
    assert_eq!(
        pair(&new, "--permission-prompts").as_deref(),
        Some("none"),
        "nothing waits for a person"
    );
    let prompts = new.iter().filter(|a| a.starts_with("--permission-prompt"));
    assert_eq!(prompts.count(), 1, "no prompt tool: {new:?}");
}

#[test]
fn the_transcript_tells_where_the_prompt_stands() {
    let dir = tempfile::tempdir().unwrap();
    let session = json!({ "session_id": "s1", "config_dir": dir.path(), "workdir": "/w/my rig" });
    let key = PromptKey::new("r", 1, 1);
    assert_eq!(
        transcript(&session, key, Path::new("/nonexistent")),
        Transcript::Missing
    );
    let file = dir.path().join("projects/-w-my-rig/s1.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let user = json!({ "type": "user", "uuid": key.0.to_string() });
    for (lines, delivered) in [
        (vec![json!({ "type": "queue-operation" })], false),
        (vec![user.clone()], true),
        (vec![user, json!({ "type": "assistant" })], true),
    ] {
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(&file, text.join("\n")).unwrap();
        assert_eq!(
            transcript(&session, key, Path::new("/nonexistent")),
            Transcript::Present { delivered }
        );
    }
    // A session inheriting the host writes where the daemon's `CLAUDE_CONFIG_DIR` says, and is read there.
    let inherited = json!({ "session_id": "s1", "config_dir": null, "workdir": "/w/my rig" });
    let answered = Transcript::Present { delivered: true };
    assert_eq!(transcript(&inherited, key, dir.path()), answered);
}

#[test]
fn the_native_permission_mode_is_a_flag_and_not_a_settings_key() {
    let mut config = config(Some(json!({ "permission_mode": "plan", "env": { "X": "1" } })));
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
        actor: None,
        bead: None,
    };
    let p = part::process(&config, &plan, Path::new("/state")).unwrap();
    assert!(part::args(&p).windows(2).any(|w| w == ["--permission-mode", "plan"]));
    assert_eq!(p["settings"]["env"]["X"], "1");
    assert!(p["settings"].get("permission_mode").is_none(), "{}", p["settings"]);
    // A config naming no mode gets the harness-decides default (decision 117); a native `defaultMode` is its own.
    config.native = None;
    let p = part::process(&config, &plan, Path::new("/state")).unwrap();
    assert_eq!(p["permission_mode"], "bypassPermissions");
    assert!(
        part::args(&p)
            .windows(2)
            .any(|w| w == ["--permission-mode", "bypassPermissions"])
    );
    config.native = Some(agent(json!({ "permissions": { "defaultMode": "plan" } })));
    let p = part::process(&config, &plan, Path::new("/state")).unwrap();
    assert!(p["permission_mode"].is_null());
    assert!(!part::args(&p).iter().any(|a| a == "--permission-mode"));
    assert_eq!(p["settings"]["permissions"]["defaultMode"], "plan");
}

#[test]
fn a_config_the_adapter_cannot_deliver_is_a_config_error() {
    let mut config = config(None);
    let driver = Claude::new("claude".into(), "/state".into(), None);
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
        actor: None,
        bead: None,
    };
    let one = driver.split(&config, &plan).unwrap();
    assert_eq!(
        one,
        driver.split(&config, &plan).unwrap(),
        "the session id is derived from the attempt"
    );
    config.native = Some(agent(json!("permissions")));
    assert_eq!(driver.split(&config, &plan).unwrap_err().kind, FailureKind::ConfigError);
    config.native = None;
    let shared = aae_contract::config::Mcp {
        command: Some(vec!["srv".into()]),
        env: Default::default(),
        url: None,
        headers: Default::default(),
        scope: Some(aae_contract::config::McpScope::Project),
    };
    config.mcp.insert("shared".into(), agent(shared));
    let split = driver.split(&config, &plan).unwrap();
    assert_eq!(
        split.process.0["mcp"]["mcpServers"]["shared"]["type"], "stdio",
        "a stdio server reaching the adapter is the session's own: the core shares the others behind the bridge"
    );
}

fn new_plan() -> SessionPlan {
    SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "t".into(),
        actor: None,
        bead: None,
    }
}

#[test]
fn the_agents_of_the_list_reach_the_session_and_again_on_a_resume() {
    use aae_contract::config::{Hook, Mcp, Skill};
    let hook = |command: &str| Hook {
        enabled: true,
        event: Some("PostToolUse".into()),
        matcher: None,
        command: Some(command.into()),
        timeout: None,
    };
    let stdio = |command: &str| Mcp {
        command: Some(vec![command.into()]),
        env: Default::default(),
        url: None,
        headers: Default::default(),
        scope: None,
    };
    let skill = |path: &str| Skill {
        enabled: true,
        path: Some(path.into()),
    };
    let mut session = config(None);
    session.hooks.insert("log".into(), agent(hook("log")));
    session.mcp.insert("web".into(), agent(stdio("web")));
    session.skills.insert("plan".into(), agent(skill("/s/plan")));
    let mut qa = config(None);
    qa.description = Some(agent("Checks the work.".into()));
    qa.system_prompt = Some(agent("You check.".into()));
    qa.model = Some(agent("claude-haiku-5-5".into()));
    qa.effort = Some(agent("low".into()));
    // The hook and the server the session has are the session's; their names do not matter.
    qa.hooks = [
        ("same".into(), agent(hook("log"))),
        ("lint".into(), agent(hook("lint"))),
    ]
    .into();
    qa.mcp = [("web".into(), agent(stdio("web"))), ("db".into(), agent(stdio("db")))].into();
    qa.skills.insert("review".into(), agent(skill("/s/review")));
    session.targets.insert("kit.qa".into(), qa);
    let dir = tempfile::tempdir().unwrap();
    let p = part::process(&session, &new_plan(), dir.path()).unwrap();
    let lint = json!({ "matcher": "", "hooks": [{ "type": "command", "command": "lint" }] });
    let db = json!({ "type": "stdio", "command": "db", "args": [], "env": {} });
    let expected = json!({
        "description": "Checks the work.",
        "prompt": "You check.",
        "model": "claude-haiku-5-5",
        "effort": "low",
        "mcpServers": [{ "db": db }],
        "hooks": { "PostToolUse": [lint] },
    });
    assert_eq!(p["agents"], json!({ "kit.qa": expected }));
    assert_eq!(p["skills"], json!({ "plan": "/s/plan", "review": "/s/review" }));
    let files = Path::new(p["files"].as_str().unwrap());
    let flag = [
        "--agents".to_owned(),
        files.join("agents.json").to_string_lossy().into_owned(),
    ];
    let mut resumed = p.clone();
    resumed["resume"] = json!(true);
    for p in [&p, &resumed] {
        assert!(part::args(p).windows(2).any(|w| w == flag), "{:?}", part::args(p));
    }
    part::materialize(&p).unwrap();
    let written: Value = serde_json::from_slice(&std::fs::read(files.join("agents.json")).unwrap()).unwrap();
    assert_eq!(written, p["agents"]);
    // No target, no definition: neither the flag nor the file.
    session.targets.clear();
    let p = part::process(&session, &new_plan(), dir.path().join("none").as_path()).unwrap();
    assert!(p["agents"].is_null());
    assert!(!part::args(&p).iter().any(|a| a == "--agents"));
    part::materialize(&p).unwrap();
    assert!(!Path::new(p["files"].as_str().unwrap()).join("agents.json").exists());
}

#[test]
fn no_built_in_agent_type_is_left() {
    let mut session = config(None);
    let without = part::process(&session, &new_plan(), Path::new("/state")).unwrap();
    let mut qa = config(None);
    qa.description = Some(agent("Checks the work.".into()));
    session.targets.insert("kit.qa".into(), qa);
    let with = part::process(&session, &new_plan(), Path::new("/state")).unwrap();
    for p in [without, with] {
        let env = part::env(&p, "i1", None, Path::new("/state")).unwrap();
        let var = ("CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS".to_owned(), "1".to_owned());
        assert!(env.contains(&var), "{env:?}");
    }
}

mod process;
