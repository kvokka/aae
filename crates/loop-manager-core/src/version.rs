//! The version checks of the binaries the daemon runs: a harness binary from the inclusive minimum of its adapter,
//! with no upper bound (contracts 4.5); br within the range its adapter was tested on. The daemon warns at start, and
//! `loop-manager doctor` does, when a binary fails its check.

use std::path::Path;

type Version = (u32, u32, u32);

/// The `--version` output of `binary`, or why it is not `minimum` or newer.
pub fn supported_version(binary: &Path, minimum: Version) -> Result<String, String> {
    let out = version_output(binary)?;
    if supported(&out, minimum) {
        return Ok(out);
    }
    Err(format!(
        "{} is `{out}`; supported: {} or newer",
        binary.display(),
        dotted(minimum)
    ))
}

/// The `--version` output of `binary`, or why it is not a tested version.
pub fn tested_version(binary: &Path, tested: [Version; 2]) -> Result<String, String> {
    let out = version_output(binary)?;
    match parse(&out) {
        Some(v) if (tested[0]..=tested[1]).contains(&v) => Ok(out),
        _ => Err(format!(
            "{} is `{out}`; tested: {} to {}",
            binary.display(),
            dotted(tested[0]),
            dotted(tested[1])
        )),
    }
}

/// The trimmed standard output of `binary --version`.
fn version_output(binary: &Path) -> Result<String, String> {
    match crate::orphans::output_std(std::process::Command::new(binary).arg("--version")) {
        Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned()),
        Err(e) => Err(format!("{}: {e}", binary.display())),
    }
}

/// `--version` output that names `minimum` or a newer version.
fn supported(out: &str, minimum: Version) -> bool {
    parse(out).is_some_and(|v| v >= minimum)
}

fn dotted((a, b, c): Version) -> String {
    format!("{a}.{b}.{c}")
}

/// The first `major.minor.patch` word of `--version` output: `2.1.322 (Some Tool)`, `some-cli 0.167.2`.
fn parse(text: &str) -> Option<Version> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word.split('.').map(|p| p.parse().ok());
        let version = (parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    })
}

#[test]
fn a_version_is_read_from_the_version_output() {
    assert_eq!(parse("2.1.322 (Claude Code)"), Some((2, 1, 322)));
    assert_eq!(parse("codex-cli 0.167.2"), Some((0, 167, 2)));
    assert_eq!(parse("claude"), None);
    assert_eq!(parse("2.1"), None);
}

#[test]
fn a_harness_version_is_supported_from_the_minimum_up_with_no_upper_bound() {
    let minimum = (2, 1, 329);
    for (out, want) in [
        ("2.1.328 (Claude Code)", false),
        ("2.0.999 (Claude Code)", false),
        ("1.9.400 (Claude Code)", false),
        ("2.1.329 (Claude Code)", true),
        ("2.1.330 (Claude Code)", true),
        ("2.2.0 (Claude Code)", true),
        ("3.0.0 (Claude Code)", true),
        ("Claude Code", false),
    ] {
        assert_eq!(supported(out, minimum), want, "{out}");
    }
}
