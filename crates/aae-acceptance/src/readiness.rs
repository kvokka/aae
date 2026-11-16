//! Criterion 22 on the readiness file (spec §14, decision 53): its scores against the rows of the report and the
//! parts of §15 no row checks yet, and its marker against the last change of the code.

use crate::checks::root;

/// The parts of §15 no row checks yet, by criterion. The report shows each as an `open` row, so its criterion stays
/// open whatever its other rows say: green rows of a part of a criterion do not make the criterion green.
pub(crate) const OPEN: &[(&str, &str)] = &[
    (
        "20",
        "the demos of the current config shape (runs without a rig, agent scope, the project root) have not run live",
    ),
    (
        "24",
        "the beads of a rig carry its prefix; the row above checks `rig:` labels",
    ),
    (
        "28",
        "a task: text files the root bead, an id of a bead takes it up, no second bead",
    ),
    (
        "29",
        "a step repeats up to `max_attempts` while its check script fails; exit code 75 fails it at once",
    ),
    (
        "30",
        "`drain` keeps up to `pool` items in work and follows `on_item_failure`",
    ),
    (
        "31",
        "a `formula` step runs its formula as a child run; formulas with `internal = true`",
    ),
    (
        "32",
        "the beads of a rig carry its prefix, and `drain` takes the rig of a child run from it",
    ),
    ("33", "an agent's prompt template and files from the project"),
    (
        "34",
        "a step ends once its outcome is declared, without waiting for the background, and leaves no orphan",
    ),
];

/// Criterion 22 against the rest of the report (spec §14, §2, decision 53), the report's rows as `(criterion, mark)`.
/// A score of 8 is "the component's §15 criteria green", so one of its criteria (the `Criteria` column of
/// docs/READINESS.md) open caps it at 6: a `FAIL` row, a part in [`OPEN`] (whatever rows the run picked), or in a
/// full run a row not run or left out. A 10 needs
/// criterion 20 not open the same way: in a full run every row of it green. A full run also needs every component at
/// 8 or more, the letter of criterion 22; a CI run cannot judge the rows with a model and checks only the caps.
/// `Ok`: the components below 8.
pub(crate) fn verdict(readiness: &str, rows: &[(&str, &str)], full: bool) -> Result<String, String> {
    let marks = |c: &str| -> Vec<&str> { rows.iter().filter(|(r, _)| *r == c).map(|(_, m)| *m).collect() };
    let open = |c: &&str| {
        let marks = marks(c);
        let failed = |m: &&str| *m == "FAIL" || (full && *m == "not run");
        OPEN.iter().any(|(o, _)| o == c) || (full && marks.is_empty()) || marks.iter().any(failed)
    };
    let (mut low, mut wrong) = (Vec::new(), Vec::new());
    let mut table = readiness.lines().filter(|l| l.starts_with("| "));
    let header: Vec<&str> = table.next().unwrap_or_default().split('|').map(str::trim).collect();
    let column = |name: &str| header.iter().position(|h| *h == name);
    let (Some(criteria_at), Some(score_at)) = (column("Criteria"), column("Score")) else {
        return Err("docs/READINESS.md: the table has no `Criteria` and `Score` columns".into());
    };
    for line in table {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (name, score) = (cells[1], cells.get(score_at).copied().unwrap_or_default());
        let Ok(score) = score.parse::<f64>() else {
            wrong.push(format!("{name}: `{score}` is not a score"));
            continue;
        };
        let criteria = cells
            .get(criteria_at)
            .copied()
            .unwrap_or_default()
            .split(',')
            .map(str::trim);
        if let Some(c) = criteria.filter(|c| !c.is_empty() && *c != "-").find(open)
            && score > 6.0
        {
            wrong.push(format!("{name} scores {score} with criterion {c} open: at most 6"));
        }
        if score >= 10.0 && open(&"20") {
            wrong.push(format!("{name} scores {score} with criterion 20 open"));
        }
        if score < 8.0 {
            low.push(format!("{name} {score}"));
        }
    }
    if !wrong.is_empty() {
        return Err(format!("docs/READINESS.md claims too much: {}", wrong.join("; ")));
    }
    match low.is_empty() {
        true => Ok("every component at 8 or more".into()),
        false if full => Err(format!(
            "criterion 22 needs every component at 8 or more: {}",
            low.join(", ")
        )),
        false => Ok(format!("below 8: {}", low.join(", "))),
    }
}

/// The readiness file is current: its `MEASURE id=readiness` marker names (`ver=`) a commit at or after the last
/// one that changed `crates/`.
pub(crate) fn fresh(readiness: &str) -> Result<(), String> {
    let marker = readiness.lines().find(|l| l.contains("MEASURE id=readiness"));
    let ver = marker.and_then(|m| m.split_whitespace().find_map(|w| w.strip_prefix("ver=")));
    let ver = ver.ok_or("docs/READINESS.md has no `MEASURE id=readiness ... ver=<sha>` marker")?;
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root())
            .output();
        out.ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let last = git(&["log", "-1", "--format=%h", "--", "crates"]).ok_or("git log of crates/ failed")?;
    match git(&["merge-base", "--is-ancestor", &last, ver]) {
        Some(_) => Ok(()),
        None => Err(format!(
            "docs/READINESS.md is assessed at {ver}, not at or after {last}, the last change of crates/: re-assess it"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_may_not_claim_more_than_the_open_rows_allow() {
        let table =
            |score: &str| format!("| Component | Criteria | Score |\n|---|---|---|\n| CLI | 3, 17 | {score} |\n");
        let pass = [("3", "pass"), ("17", "pass"), ("17", "not run"), ("20", "pass")];
        assert_eq!(
            verdict(&table("8"), &pass[..3], false),
            Ok("every component at 8 or more".into())
        );
        // A row not run caps the score in a full run only; a FAIL or an open part caps it in either.
        if let Some((part, _)) = OPEN.first() {
            let with_part = |score: &str| table(score).replace("3, 17", &format!("3, {part}"));
            let err = verdict(&with_part("8"), &pass[..1], false).unwrap_err();
            assert!(err.contains(&format!("criterion {part} open")), "{err}");
            assert!(verdict(&with_part("6"), &pass[..1], false).is_ok());
        }
        assert!(
            verdict(&table("8"), &pass[..3], true)
                .unwrap_err()
                .contains("criterion 17 open")
        );
        let failed = [("3", "FAIL"), ("17", "pass")];
        assert!(verdict(&table("8"), &failed, false).unwrap_err().contains("at most 6"));
        assert_eq!(verdict(&table("6"), &failed, false), Ok("below 8: CLI 6".into()));
        // A full run needs 8 everywhere; a 10 needs criterion 20 green in it, and no FAIL of it in CI.
        let all = [("3", "pass"), ("17", "pass"), ("20", "pass"), ("20", "not run")];
        assert!(
            verdict(&table("6"), &all[..3], true)
                .unwrap_err()
                .contains("at 8 or more: CLI 6")
        );
        // An open part of criterion 20 refuses a 10 whatever its rows say.
        let demos_open = OPEN.iter().any(|(c, _)| *c == "20");
        assert_eq!(verdict(&table("10"), &all[..3], true).is_ok(), !demos_open);
        assert_eq!(verdict(&table("10"), &all, false).is_ok(), !demos_open);
        for (rows, full) in [(&all[..], true), (&all[..2], true), (&[("20", "FAIL")][..], false)] {
            let err = verdict(&table("10"), rows, full).unwrap_err();
            assert!(err.contains("criterion 20 open"), "{err}");
        }
        assert!(verdict("| Component | Score |\n| CLI | 8 |\n", &all, false).is_err());
    }

    #[test]
    fn readiness_is_stale_before_the_last_change_of_the_code() {
        let marker = |ver: &str| format!("<!-- {} id=readiness date=2026-11-14 ver={ver} env=x -->\n", "MEASURE");
        assert_eq!(fresh(&marker("HEAD")), Ok(()));
        let first = std::process::Command::new("git")
            .args(["rev-list", "--max-parents=0", "HEAD"])
            .output()
            .unwrap();
        let first = String::from_utf8(first.stdout).unwrap();
        let first = first.lines().next().unwrap();
        assert!(fresh(&marker(first)).unwrap_err().contains("re-assess"));
        assert!(fresh("no marker").is_err());
    }
}
