//! The addon guides (docs/guide/addons.md, docs/guide/addon-development.md): the files their examples show load as
//! the config they describe, and the hooks of the example addon answer as the guide says.

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The fenced blocks of a guide with their language, each `(lang, path, text)`: `path` is the one of a first comment
/// line `# <path>` (after a `#!` line), the file the block shows, or empty.
fn blocks(guide: &str) -> Vec<(String, String, String)> {
    let text = fs::read_to_string(repo().join("docs/guide").join(guide)).unwrap();
    let mut out = Vec::new();
    let mut open: Option<(String, Vec<&str>)> = None;
    for line in text.lines() {
        match (open.take(), line.strip_prefix("```")) {
            (None, Some(lang)) => open = Some((lang.to_owned(), Vec::new())),
            (Some((lang, lines)), Some(_)) => {
                let tag = lines.iter().take(2).find_map(|l| l.strip_prefix("# "));
                let path = tag.filter(|t| !t.contains(' ')).unwrap_or_default();
                out.push((lang, path.to_owned(), lines.join("\n") + "\n"));
            }
            (Some((lang, mut lines)), None) => {
                lines.push(line);
                open = Some((lang, lines));
            }
            (None, None) => {}
        }
    }
    out
}

fn loop_manager(home: &Path, args: &[&str]) -> String {
    let config = home.join(".config/loop-manager/config.toml");
    let out = Command::new(env!("CARGO_BIN_EXE_loop-manager"))
        .args(args)
        .arg("--config")
        .arg(config)
        .env("LOOP_MANAGER_SOCKET", "/nonexistent/loop-manager.sock")
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{args:?}: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

fn run(script: &Path, args: &[&str], env: &[(&str, &str)], stdin: &str) -> String {
    use std::io::Write;
    let mut child = (Command::new(script).args(args).envs(env.iter().copied()))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // A hook that does not read its input closes the pipe first.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", script.display());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn the_examples_of_the_addon_guides_load_and_their_hooks_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let mut all = blocks("addons.md");
    all.extend(blocks("addon-development.md"));
    for (lang, path, text) in &all {
        if lang == "toml" {
            text.parse::<toml::Table>()
                .unwrap_or_else(|e| panic!("{path}: {e}\n{text}"));
        }
        if let Some(path) = path.strip_prefix("~/") {
            let file = home.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        } else if !path.is_empty() {
            // A file of this repository, shown as it is.
            let file = fs::read_to_string(repo().join(path)).unwrap();
            assert_eq!(text.split_once('\n').unwrap().1, file, "{path}");
        }
    }
    // What the guides show only in a tree: the project file, the review addon's prompt and skill; then the review
    // addon is declared and attached to `qa`.
    fs::write(home.join("src/shop/loop-manager.toml"), "").unwrap();
    let review = home.join("loop-manager-addons/review");
    fs::write(review.join("prompt.md"), "Review.\n").unwrap();
    fs::create_dir_all(review.join("skills/review-checklist")).unwrap();
    fs::write(review.join("skills/review-checklist/SKILL.md"), "Check.\n").unwrap();
    std::os::unix::fs::symlink(
        repo().join("addons/context-budget"),
        home.join("loop-manager-addons/context-budget"),
    )
    .unwrap();
    let append = |path: &str, text: &str| {
        let file = home.join(path);
        fs::write(&file, fs::read_to_string(&file).unwrap() + text).unwrap();
    };
    append(
        ".config/loop-manager/config.toml",
        "[[addon]]\npath = \"~/loop-manager-addons/review\"\n",
    );
    append("src/shop/agents/qa/agent.toml", "addons = [{ name = \"review\" }]\n");

    assert!(loop_manager(home, &["--json", "config", "check"]).contains("\"ok\":true"));
    let dev = loop_manager(home, &["config", "explain", "--project", "shop", "--agent", "dev"]);
    assert!(dev.contains("addons.context-budget.params.window = 150000"), "{dev}");
    assert!(
        dev.contains("{\"autoCompactWindow\":150000}  # addon context-budget"),
        "{dev}"
    );
    let qa = loop_manager(home, &["config", "explain", "--project", "shop", "--agent", "qa"]);
    assert!(
        qa.contains("REVIEW_WORD = \"tested\"") && qa.contains("hooks/remind' 'tested'"),
        "{qa}"
    );

    let remind = run(&review.join("hooks/remind"), &["tested"], &[], "{}");
    assert!(remind.contains("is tested"), "{remind}");
    let check = |summary: &str| {
        let event = serde_json::json!({ "type": "run.finishing",
            "data": { "outcome": { "result": "pass", "summary": summary } } });
        run(
            &review.join("hooks/check"),
            &[],
            &[("LOOP_MANAGER_ADDON_PARAMS", r#"{"word":"tested"}"#)],
            &event.to_string(),
        )
    };
    assert_eq!(check("Done, tested."), "");
    let remark: serde_json::Value = serde_json::from_str(&check("Done.")).unwrap();
    assert_eq!(remark["decision"], "remark");
}
