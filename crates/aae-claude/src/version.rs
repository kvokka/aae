//! The tested range of `claude` (contracts 4.5).

use crate::Claude;

/// The versions of `claude` the adapter is checked against (contract tests, experiments), inclusive.
pub(crate) const TESTED: [(u32, u32, u32); 2] = [(2, 1, 322), (2, 1, 322)];

impl Claude {
    /// Why the configured binary is not a tested `claude`, if it is not: the daemon warns with it at start.
    pub fn untested(&self) -> Option<String> {
        let shown = self.binary.display();
        let out = match std::process::Command::new(&self.binary).arg("--version").output() {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            Err(e) => return Some(format!("{shown}: {e}")),
        };
        let dotted = |(a, b, c): (u32, u32, u32)| format!("{a}.{b}.{c}");
        match parse(&out) {
            Some(v) if (TESTED[0]..=TESTED[1]).contains(&v) => None,
            _ => Some(format!(
                "{shown} is `{out}`; tested: {} to {}",
                dotted(TESTED[0]),
                dotted(TESTED[1])
            )),
        }
    }
}

/// `(major, minor, patch)` of `claude --version` output such as `2.1.322 (Claude Code)`.
pub(crate) fn parse(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split_whitespace().next()?.split('.').map(|p| p.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}
