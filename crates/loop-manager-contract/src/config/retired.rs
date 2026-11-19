//! A parse error about a key an earlier config shape had, as the one line that says where it is written now.

/// What an earlier config shape said, and where it is written now: the error then is that one line.
const RETIRED: [(&str, &str); 7] = [
    (
        "unknown field `permission_mode`",
        "`permission_mode` is native to a harness: its own key in `[settings.native.<harness>]`",
    ),
    (
        "unknown field `overrides`",
        "`[overrides.<agent>]` is `[agents.<agent>]` now",
    ),
    ("unknown variant `prompt`", "step kind `prompt` is `ai` now"),
    (
        "unknown field `config`, expected one of `path`",
        "a `[[addon]]` has no `config`: its params are given where it is attached, `addons = [{ name, params }]`",
    ),
    (
        "unknown field `prompt`",
        "an `ai` step has no `prompt`: its agent's template is `agents/<agent>/prompt.template.md`",
    ),
    (
        "unknown variant `bead`",
        "input type `bead` is `task` now: a text, a bead id or `{ next_ready = true }`",
    ),
    (
        "unknown field `bead_failures`",
        "`bead_failures` is `bead_failed_runs` now",
    ),
];

/// A parse error of `text` as one line with the hint of [`RETIRED`] when it is about a retired key, else as it is.
pub(super) fn retired(message: String, text: &str) -> String {
    let Some((said, hint)) = RETIRED.iter().find(|(said, _)| message.contains(said)) else {
        return message.trim_end().to_owned();
    };
    let at = message
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("TOML parse error at "));
    match at {
        Some(at) => format!("{}: {hint}", key_at(said, at, text).unwrap_or_else(|| at.to_owned())),
        None => (*hint).to_owned(),
    }
}

/// The place of the retired key itself: the parser points an unknown key of a tagged table at the table's header.
fn key_at(said: &str, at: &str, text: &str) -> Option<String> {
    let key = said.strip_prefix("unknown field `")?.split('`').next()?;
    let header: usize = at.strip_prefix("line ")?.split(',').next()?.parse().ok()?;
    let (n, line) = text
        .lines()
        .enumerate()
        .skip(header.checked_sub(1)?)
        .find(|(_, l)| (l.trim_start().strip_prefix(key)).is_some_and(|rest| rest.trim_start().starts_with('=')))?;
    let column = line.len() - line.trim_start().len() + 1;
    Some(format!("line {}, column {column}", n + 1))
}
