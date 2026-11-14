use super::{Paths, driver};
use aae_core::{config::Harness, driver::ProcessPart};
use serde_json::json;
use std::time::Duration;

#[test]
fn the_codex_settings_reach_the_driver_capabilities() {
    let paths = Paths {
        config: "/nonexistent/config.toml".into(),
        state: "/nonexistent/state".into(),
        socket: "/nonexistent/aae.sock".into(),
    };
    let caps = |options: &str| {
        let harness = Harness {
            binary: Some("/nonexistent/codex".into()),
            options: toml::from_str(options).unwrap(),
            ..Harness::default()
        };
        let (driver, _, _) = driver("codex", &harness, &paths).unwrap();
        let caps = driver.capabilities(&ProcessPart(json!(null)));
        (caps.sessions_per_instance, caps.instance_idle)
    };
    assert_eq!(caps(""), (8, Duration::from_secs(60)), "the defaults");
    let set = caps("max_sessions = 3\ninstance_idle_s = 0");
    assert_eq!(
        set,
        (3, Duration::ZERO),
        "0: an app-server stops once it has no threads"
    );
}
