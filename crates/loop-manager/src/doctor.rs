//! The checks of `loop-manager doctor` (docs/guide/install.md), made by the daemon (`GET /v1/doctor`) or, for a file named with
//! `--config`, by the CLI without one: the harness and br binaries with their versions and logins, the private
//! directories, the state and its outbox; and `loop-manager daemon --service`, the service file that runs the daemon.

use crate::{
    daemon::Paths,
    harness::{adapter, binary, load},
};
use loop_manager_core::config::{Config, EffectiveConfig, Name};
use loop_manager_front::cli::Service;
use serde_json::{Value, json};
use std::{
    env,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// An outbox row older than this means the journal does not keep up (contracts 3.5).
const OUTBOX_STALE_MS: i64 = 60_000;

/// One finding: `level` is `ok`, `warning` or `error`.
fn check(what: impl Into<String>, level: &str, detail: impl Into<String>) -> Value {
    json!({ "check": what.into(), "level": level, "detail": detail.into() })
}

/// Every check, in the order `loop-manager doctor` prints them.
pub(crate) fn findings(paths: &Paths) -> Vec<Value> {
    let mut found = Vec::new();
    match load(paths) {
        Err(errors) => found.extend(errors.into_iter().map(|e| check("config", "error", e))),
        Ok(config) => {
            let path = paths.config.display();
            let detail = if paths.config.exists() {
                path.to_string()
            } else {
                format!("no config file at {path}, the defaults")
            };
            found.push(check("config", "ok", detail));
            for (name, harness) in &config.harnesses {
                // `load` refused a harness with no adapter.
                let Some(adapter) = adapter(name) else { continue };
                let (path, source) = binary(name, harness.binary.as_deref());
                found.push(binary_check(name, &path, source, |p| {
                    loop_manager_core::supported_version(p, adapter.minimum)
                }));
                let checks = (adapter.doctor)(&agents(&config, name));
                found.extend(checks.into_iter().map(|c| check(c.check, c.level, c.detail)));
            }
            if config.projects.iter().any(|p| p.dir.join(".beads").is_dir()) {
                let (path, source) = binary("br", None);
                found.push(binary_check("br", &path, source, |p| {
                    loop_manager_core::tested_version(p, loop_manager_br::TESTED)
                }));
                found.push(check(
                    "br license",
                    "warning",
                    "br is MIT with an OpenAI/Anthropic rider, not an OSI license; loop-manager only runs the installed \
                     binary — check that the rider fits your use",
                ));
                // A rig's beads must not share the prefix of the root's (docs/design/tracker-br.md); the core does not
                // know br.
                for p in config.projects.iter().filter(|p| p.dir.join(".beads").is_dir()) {
                    let Some(db) = loop_manager_br::issue_prefix(&path, &p.dir.join(".beads")) else {
                        continue;
                    };
                    for (rig, _) in p.rigs.iter().filter(|(_, r)| r.prefix.as_deref() == Some(db.as_str())) {
                        found.push(check(
                            "br prefix",
                            "warning",
                            format!(
                                "rig `{rig}` of project `{}` has the prefix `{db}` of the br database, so the root's \
                                 beads read as its own; set `prefix` in `[rigs.{rig}]`",
                                p.name
                            ),
                        ));
                    }
                }
            }
        }
    }
    // Without a runtime directory the socket lives in the state directory: one line for it, not two.
    let socket_dir = paths.socket.parent().filter(|d| *d != paths.state);
    for dir in [Some(paths.state.as_path()), socket_dir].into_iter().flatten() {
        found.push(private(dir));
    }
    found.push(state(&paths.state.join("state.db")));
    found
}

/// The agents on `harness` in every project, root and rig, labelled `<project>/<place>@<agent>`, with their effective
/// configs: each pack agent alone, `None` where it does not assemble, then as the runs of every prompt formula have
/// it, a variant by its name.
fn agents(config: &Config, harness: &str) -> Vec<(String, Option<EffectiveConfig>)> {
    let mut found = std::collections::BTreeMap::new();
    for p in &config.projects {
        let rigs: Vec<&str> = p.places().map(|(r, _)| r).collect();
        for pack in &p.packs {
            for (name, agent) in &pack.agents {
                let name = Name {
                    pack: pack.name.clone(),
                    name: name.clone(),
                };
                for rig in &rigs {
                    let label = format!("{}/{rig}@{name}", p.name);
                    match config.effective(&p.name, Some(rig), &name) {
                        Ok(c) if c.harness == harness => drop(found.insert(label, Some(c))),
                        Err(_) if agent.harness.as_deref() == Some(harness) => drop(found.insert(label, None)),
                        _ => {}
                    }
                }
            }
            for (formula, f) in pack.formulas.iter().map(|(n, f)| (format!("{}.{n}", pack.name), f)) {
                for (rig, step) in rigs.iter().flat_map(|r| f.steps.iter().map(move |s| (r, &s.name))) {
                    // An exec step has no agent.
                    if let Ok((agent, c)) = config.for_run(&p.name, rig, &formula, step)
                        && c.harness == harness
                    {
                        found.insert(format!("{}/{rig}@{agent}", p.name), Some(c));
                    }
                }
            }
        }
    }
    found.into_iter().collect()
}

/// The binary exists and its version passes `version`, the check of its adapter.
fn binary_check(name: &str, path: &Path, source: &str, version: impl FnOnce(&Path) -> Result<String, String>) -> Value {
    let Some(path) = on_disk(path) else {
        return check(name, "error", format!("{} (from {source}): not found", path.display()));
    };
    match version(&path) {
        Ok(version) => check(name, "ok", format!("{} `{version}` (from {source})", path.display())),
        Err(why) => check(name, "warning", why),
    }
}

/// `path` if it is a file, or where a bare name is found on `PATH`.
fn on_disk(path: &Path) -> Option<PathBuf> {
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_owned());
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(path))
        .find(|p| p.is_file())
}

/// A directory of the daemon is the owner's only; one not made yet is made so at start.
fn private(dir: &Path) -> Value {
    let what = format!("dir {}", dir.display());
    match std::fs::metadata(dir) {
        Err(_) => check(what, "ok", "made with mode 0700 at start"),
        Ok(meta) if meta.permissions().mode() & 0o077 == 0 => check(what, "ok", "mode 0700"),
        Ok(meta) => check(
            what,
            "error",
            format!(
                "mode {:o}: others can read it; chmod 700",
                meta.permissions().mode() & 0o777
            ),
        ),
    }
}

/// The state opens, and its outbox drains into the journal.
fn state(db: &Path) -> Value {
    if !db.exists() {
        return check("state", "ok", "no state yet: made at start");
    }
    let status = loop_manager_core::run::Reader::open(db).and_then(|r| r.status());
    let status = match status {
        Ok(status) => status,
        Err(e) => return check("state", "error", format!("{}: {e}", db.display())),
    };
    let depth = status["outbox_depth"].as_i64().unwrap_or_default();
    let age = status["outbox_oldest_ms"]
        .as_i64()
        .map_or(0, |t| loop_manager_core::Clock::system().now_ms() - t);
    let detail = format!("outbox: {depth} rows, oldest {} s", age / 1000);
    // A running daemon drains it at once; an old row means the journal fails, or no daemon ran since.
    let level = if age > OUTBOX_STALE_MS { "warning" } else { "ok" };
    check("state", level, detail)
}

/// The service file that runs `loop-manager daemon` with `config` (absolute), and the `PATH` of this shell so the daemon
/// finds mise (docs/design/config.md). The daemon is not started: the file is printed for the user to install.
pub(crate) fn service(kind: Service, config: Option<PathBuf>) -> ExitCode {
    let exe = env::current_exe().map_or_else(|_| "loop-manager".into(), |p| p.display().to_string());
    let mut args = vec![exe, "daemon".to_owned()];
    if let Some(config) = config {
        let config = std::path::absolute(&config).unwrap_or(config);
        args.extend(["--config".to_owned(), config.display().to_string()]);
    }
    let path = env::var("PATH").unwrap_or_default();
    print!("{}", unit(kind, &args, &path));
    ExitCode::SUCCESS
}

fn unit(kind: Service, args: &[String], path: &str) -> String {
    match kind {
        Service::Systemd => {
            let quote = |s: &str| {
                let s = s.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{}\"", s.replace('%', "%%").replace('$', "$$"))
            };
            let exec: Vec<String> = args.iter().map(|a| quote(a)).collect();
            format!(
                "# ~/.config/systemd/user/loop-manager.service; then: systemctl --user enable --now loop-manager\n\
                 [Unit]\nDescription=loop-manager daemon\n\n[Service]\nExecStart={}\nEnvironment={}\n\
                 Restart=on-failure\n\n[Install]\nWantedBy=default.target\n",
                exec.join(" "),
                quote(&format!("PATH={path}"))
            )
        }
        Service::Launchd => {
            let xml = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            let program: String = args
                .iter()
                .map(|a| format!("    <string>{}</string>\n", xml(a)))
                .collect();
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!-- ~/Library/LaunchAgents/dev.loop-manager.daemon.plist; then: launchctl load -w <that file> -->\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>dev.loop-manager.daemon</string>\n\
                 \x20 <key>ProgramArguments</key>\n  <array>\n{program}  </array>\n\
                 \x20 <key>EnvironmentVariables</key>\n  <dict>\n    <key>PATH</key>\n    \
                 <string>{}</string>\n  </dict>\n\
                 \x20 <key>RunAtLoad</key>\n  <true/>\n\
                 \x20 <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
                 </dict>\n</plist>\n",
                xml(path)
            )
        }
    }
}
