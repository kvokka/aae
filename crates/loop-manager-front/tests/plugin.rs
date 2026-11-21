//! The skills plugin of Claude Code and Codex in `plugins/loop-manager/` (docs/guide/recovery.md): its manifests are the
//! shape both harnesses install, and its skills name only commands the CLI has.

use clap::{CommandFactory, Parser};
use loop_manager_front::cli::Cli;
use serde_json::Value;
use std::{fs, path::Path};

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// A named plugin in a marketplace file.
fn entry<'a>(market: &'a Value, name: &str) -> &'a Value {
    assert!(market["name"].is_string(), "{market}");
    market["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap()
}

/// The repository is a marketplace of plugins in the current format of each harness: Claude Code reads
/// `.claude-plugin/marketplace.json` and the plugin's `.claude-plugin/plugin.json`, Codex reads
/// `.agents/plugins/marketplace.json` and the plugin's root `plugin.json` (Claude Code 2.1, Codex 0.169). Neither
/// relies on a format the other harness reads only for compatibility.
#[test]
fn the_manifests_are_what_claude_and_codex_install() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let claude = json(&root.join(".claude-plugin/marketplace.json"));
    assert!(claude["owner"]["name"].is_string(), "{claude}");
    let codex = json(&root.join(".agents/plugins/marketplace.json"));
    assert_eq!(claude["name"], "loop-manager");
    assert_eq!(codex["name"], "loop-manager");
    for name in ["loop-manager", "run-card"] {
        check_plugin(&root, entry(&claude, name), entry(&codex, name));
    }
}

fn check_plugin(root: &Path, claude_entry: &Value, codex_entry: &Value) {
    assert_eq!(codex_entry["source"]["source"], "local", "{codex_entry}");
    for key in ["installation", "authentication"] {
        assert!(codex_entry["policy"][key].is_string(), "{codex_entry}");
    }
    assert!(codex_entry["category"].is_string(), "{codex_entry}");
    let source = claude_entry["source"].as_str().unwrap();
    assert!(source.starts_with("./") && !source.contains(".."), "{source}");
    assert_eq!(
        codex_entry["source"]["path"], source,
        "both marketplaces point to one plugin"
    );
    let dir = root.join(source);
    assert!(
        !dir.join(".codex-plugin").exists(),
        "no compatibility manifest of Codex"
    );
    let version = json(&dir.join("plugin.json"))["version"].clone();
    assert!(version.is_string());
    for manifest in [".claude-plugin/plugin.json", "plugin.json"] {
        let plugin = json(&dir.join(manifest));
        let name = plugin["name"].as_str().unwrap();
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{name}"
        );
        assert_eq!(
            claude_entry["name"], name,
            "the Claude marketplace entry names the plugin of {manifest}"
        );
        assert_eq!(
            codex_entry["name"], name,
            "the Codex marketplace entry names the plugin of {manifest}"
        );
        // Both harnesses cache a plugin by its version: a release that changes a skill changes the version.
        assert_eq!(plugin["version"], version, "{manifest}");
    }
    if claude_entry["name"] == "run-card" {
        assert_eq!(json(&dir.join("mcp.json"))["mcpServers"]["run-card"]["type"], "stdio");
        return;
    }
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    for skill in fs::read_dir(dir.join("skills")).unwrap() {
        let skill = skill.unwrap().path();
        let text = fs::read_to_string(skill.join("SKILL.md")).unwrap();
        let front = text
            .strip_prefix("---\n")
            .and_then(|t| t.split_once("\n---\n"))
            .unwrap()
            .0;
        let field = |key: &str| front.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(": "));
        assert_eq!(
            field("name"),
            skill.file_name().unwrap().to_str(),
            "{}",
            skill.display()
        );
        assert!(
            field("description").is_some_and(|d| !d.is_empty()),
            "{}",
            skill.display()
        );
    }
}

/// Words of the first command in a shell pipeline: a single-quoted part is one word, even with a pipe inside.
fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted) = (Vec::new(), String::new(), false);
    for c in line.chars() {
        match c {
            '\'' => quoted = !quoted,
            '|' if !quoted => break,
            ' ' if !quoted => out.extend((!word.is_empty()).then(|| std::mem::take(&mut word))),
            c => word.push(c),
        }
    }
    out.extend((!word.is_empty()).then_some(word));
    out
}

#[test]
fn command_words_keep_a_quoted_pipe_and_stop_at_the_next_command() {
    assert_eq!(
        words("loop-manager run start shop check --input 'name=left | right' --json | jq -r .run"),
        [
            "loop-manager",
            "run",
            "start",
            "shop",
            "check",
            "--input",
            "name=left | right",
            "--json"
        ]
    );
}

#[test]
fn every_command_a_skill_names_exists() {
    let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/loop-manager/skills");
    let mut seen = 0;
    for skill in fs::read_dir(&skills).unwrap() {
        let text = fs::read_to_string(skill.unwrap().path().join("SKILL.md")).unwrap();
        let mut block = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with("```") {
                block = line == "```sh";
                continue;
            }
            // A command line of a `sh` block parses with its options and values.
            if block && line.starts_with("loop-manager ") {
                if let Err(e) = Cli::try_parse_from(words(line)) {
                    panic!("`{line}`: {e}");
                }
                seen += 1;
            }
            // A command in running text names an existing command, and its flags exist on it.
            for span in line.split('`').skip(1).step_by(2) {
                if let Some(rest) = span.strip_prefix("loop-manager ") {
                    let mut command = Cli::command();
                    command.build();
                    let words = words(rest);
                    let path = words
                        .iter()
                        .position(|w| w.starts_with(['-', '<']))
                        .unwrap_or(words.len());
                    for word in &words[..path] {
                        let sub = command
                            .find_subcommand(word)
                            .unwrap_or_else(|| panic!("no command `{span}`"));
                        command = sub.clone();
                    }
                    for flag in words[path..].iter().filter_map(|w| w.strip_prefix("--")) {
                        let known = command.get_arguments().any(|a| a.get_long() == Some(flag));
                        assert!(known, "no flag `--{flag}` in `{span}`");
                    }
                    seen += 1;
                }
            }
        }
    }
    assert!(seen > 0, "no command found in {}", skills.display());
}
