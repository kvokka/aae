//! The split of an effective config into the app-server process and the thread.

use crate::{
    Codex, part,
    tests::{agent, config},
};
use aae_core::driver::{Driver, FailureKind, SessionPlan};
use serde_json::{Value, json};
use std::path::Path;

#[test]
fn the_process_gets_the_native_block_and_the_thread_the_rest() {
    let mut config = config();
    config.hooks.insert(
        "stop".into(),
        agent(aae_core::config::Hook {
            enabled: true,
            event: Some("Stop".into()),
            matcher: None,
            command: Some("true".into()),
            timeout: None,
        }),
    );
    let plan = SessionPlan::New {
        workdir: "/w".into(),
        run: "r".into(),
        attempt: 1,
        token: "tok".into(),
    };
    let driver = Codex::new("codex".into(), "/state".into(), None, 8);
    let split = driver.split(&config, &plan).unwrap();
    let p = &split.process.0;
    let args = part::args(p);
    assert!(
        args.contains(&"sandbox_mode=\"danger-full-access\"".to_owned()),
        "{args:?}"
    );
    let provider = args.iter().find(|a| a.starts_with("model_providers=")).unwrap();
    let parsed: toml::Table = toml::from_str(provider).unwrap();
    assert_eq!(
        parsed["model_providers"]["fake"]["wire_api"].as_str(),
        Some("responses")
    );
    assert!(p["home"].as_str().unwrap().starts_with("/state/codex/"));
    let env = part::env(p, "i1", None);
    assert!(env.contains(&("OPENAI_API_KEY".to_owned(), "sk-fake".to_owned())));
    assert!(env.iter().any(|(k, v)| k == "HOME" && v.ends_with("/home")));
    let s = &split.session.0;
    assert_eq!(s["cwd"], "/w");
    assert_eq!(
        (s["model"].as_str(), s["approvalPolicy"].as_str()),
        (Some("gpt-fake"), Some("never"))
    );
    assert_eq!(
        s["config"]["shell_environment_policy"]["set"],
        json!({ "AAE_RUN": "r", "AAE_ATTEMPT_TOKEN": "tok", "BD_ACTOR": "aae:r:1" })
    );
    assert_eq!(s["config"]["hooks"]["Stop"][0]["hooks"][0]["command"], "true");
    assert_eq!(s["config"]["bypass_hook_trust"], true);
    assert_eq!(s["config"]["model_reasoning_effort"], "low");
    // The session part is not in the fingerprint: two runs of one agent share a process.
    let other = SessionPlan::New {
        workdir: "/v".into(),
        run: "q".into(),
        attempt: 2,
        token: "t2".into(),
    };
    assert_eq!(driver.split(&config, &other).unwrap().fingerprint, split.fingerprint);
    config.native = Some(agent(json!("x")));
    assert_eq!(driver.split(&config, &plan).unwrap_err().kind, FailureKind::ConfigError);
}

#[test]
fn a_process_without_a_key_of_its_own_logs_in_by_the_host_provider() {
    let host = tempfile::tempdir().unwrap();
    let login = |isolated: bool| {
        let mut p = json!({ "home": "/h", "env": {}, "overrides": ["model=\"m\""] });
        part::host_provider(&mut p, host.path(), isolated);
        let keyed = part::env(&p, "i1", None).iter().any(|(k, _)| k == "CARGO_MANIFEST_DIR");
        (keyed, part::args(&p))
    };
    assert!(!login(false).0, "no host config, no key");
    std::fs::write(
        host.path().join("config.toml"),
        "model_provider = \"my.proxy\"\n[model_providers.\"my.proxy\"]\nenv_key = \"CARGO_MANIFEST_DIR\"\n\
         base_url = \"http://proxy\"\nexperimental_bearer_token = \"secret\"\nhttp_headers = { a = \"secret\" }\n",
    )
    .unwrap();
    let (keyed, args) = login(false);
    assert!(keyed);
    assert!(
        !args.iter().any(|a| a.contains("proxy")),
        "codex reads the host config itself: {args:?}"
    );
    // An isolated process does not read the host config: the provider comes as flags before the native block's.
    let (keyed, args) = login(true);
    assert!(keyed);
    let at = |flag: &str| args.iter().position(|a| a.starts_with(flag)).expect(flag);
    assert!(at("model_provider=\"my.proxy\"") < at("model=\"m\""), "{args:?}");
    let table: toml::Table = toml::from_str(&args[at("model_providers=")]).unwrap();
    assert_eq!(
        table["model_providers"]["my.proxy"]["base_url"].as_str(),
        Some("http://proxy")
    );
    assert!(
        !args.iter().any(|a| a.contains("secret")),
        "secrets stay out of argv: {args:?}"
    );
}

#[test]
fn no_sandbox_unless_the_config_or_the_inherited_host_config_sets_one() {
    let host = tempfile::tempdir().unwrap();
    let sandbox = |config: &aae_core::config::EffectiveConfig| -> Vec<String> {
        let p = part::process(config, Path::new("/state"), host.path()).unwrap();
        part::args(&p)
            .into_iter()
            .filter(|a| a.starts_with("sandbox_mode="))
            .collect()
    };
    let mut config = config();
    config.native = Some(agent(json!({ "sandbox_mode": "read-only" })));
    assert_eq!(sandbox(&config), ["sandbox_mode=\"read-only\""]);
    config.native = None;
    assert_eq!(sandbox(&config), ["sandbox_mode=\"danger-full-access\""]);
    config.inherit_host = Some(agent(true));
    assert_eq!(sandbox(&config), ["sandbox_mode=\"danger-full-access\""]);
    std::fs::write(host.path().join("config.toml"), "sandbox_mode = \"workspace-write\"\n").unwrap();
    assert_eq!(sandbox(&config), Vec::<String>::new(), "the host's own choice stands");
    config.inherit_host = Some(agent(false));
    assert_eq!(
        sandbox(&config),
        ["sandbox_mode=\"danger-full-access\""],
        "an isolated one does not read it"
    );
}

#[test]
fn a_process_inheriting_the_host_with_skills_gets_a_home_linking_the_hosts() {
    let (host, state, skill) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    for file in ["auth.json", "config.toml", "AGENTS.md"] {
        std::fs::write(host.path().join(file), "").unwrap();
    }
    for dir in ["skills/mail", "skills/notes"] {
        std::fs::create_dir_all(host.path().join(dir)).unwrap();
    }
    let mut config = config();
    config.inherit_host = Some(agent(true));
    let p = part::process(&config, state.path(), host.path()).unwrap();
    assert_eq!(p["home"], json!(host.path()), "without skills the host's home itself");
    assert!(p["user_home"].is_null());
    let mail = aae_core::config::Skill {
        enabled: true,
        path: Some(skill.path().to_owned()),
    };
    config.skills.insert("mail".into(), agent(mail));
    let p = part::process(&config, state.path(), host.path()).unwrap();
    let home = Path::new(p["home"].as_str().unwrap());
    assert!(home.starts_with(state.path()), "{p}");
    assert!(p["user_home"].is_null(), "HOME stays the host's");
    part::materialize(&p).unwrap();
    for (link, target) in [
        ("auth.json", host.path().join("auth.json")),
        ("config.toml", host.path().join("config.toml")),
        ("AGENTS.md", host.path().join("AGENTS.md")),
        ("skills/mail", skill.path().to_owned()),
        ("skills/notes", host.path().join("skills/notes")),
    ] {
        assert_eq!(std::fs::read_link(home.join(link)).unwrap(), target, "{link}");
    }
    let env = part::env(&p, "i1", None);
    assert!(env.contains(&("CODEX_HOME".to_owned(), home.to_string_lossy().into_owned())));
}

#[test]
fn only_a_process_inheriting_the_host_gets_its_network_settings() {
    let vars = |p: Value| part::daemon_vars(&p).collect::<Vec<_>>();
    let inheriting = vars(json!({ "home": "/h" }));
    for var in ["PATH", "HTTPS_PROXY", "NO_PROXY", "SSL_CERT_FILE"] {
        assert!(inheriting.contains(&var), "{var}");
    }
    let isolated = vars(json!({ "home": "/h", "user_home": "/u" }));
    assert!(isolated.contains(&"PATH") && !isolated.contains(&"HTTPS_PROXY"));
}

#[test]
fn a_thread_read_that_got_no_answer_is_no_missing_transcript() {
    use aae_core::driver::{Failure, Source, Transcript};
    let failed = |kind| crate::rpc::unread(Failure::new(kind, Source::Adapter));
    assert_eq!(failed(FailureKind::Stall).unwrap_err().kind, FailureKind::Stall);
    assert_eq!(
        failed(FailureKind::HarnessCrash).unwrap_err().kind,
        FailureKind::HarnessCrash
    );
    assert_eq!(failed(FailureKind::Unknown).unwrap(), Transcript::Missing);
}
