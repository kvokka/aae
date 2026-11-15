# Readiness

Per-component readiness on the 0–10 scale of spec §14, scored by its letter: 0 nothing; 2 contract written; 4 code
exists and the main path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15
criteria green on the owner's host; 10 real owner runs without intervention (criterion 20). An open criterion of a
component caps its score (§2, decision 53): a criterion counts as open when a row of it failed, or was not run on the
code assessed. Updated at the end of every work session.

`Criteria` lists the §15 criteria of each component; criterion 22 of `aae-acceptance` checks every score against
them: one with a `FAIL` row (or, in a full run, a row not run) holds the score at 6, a 10 needs criterion 20 green,
and the marker below must name a commit at or after the last change of `crates/`.

Assessed 2026-11-15 at `02e0ead`, after a formula became a list of steps (spec §9.1). The model-free rows ran on
it: 22 of the 23 CI rows passed, row 22 failing only on this file's marker. The full run with real models last ran on
`96dd606` (30 of 32, the codex row of criterion 8 lost to the provider's network and green on a rerun); its live rows
were not rerun on the steps. Criteria 24 (beads and `drain`), 25 (the return with a rounds limit) and 26 (packs from
git) have no rows yet, so the components they belong to score 6; the new demos of criterion 20 replace the old ones
later in this round. None scores 10: a 10 needs criterion 20 green in the run that checks this file.
<!-- MEASURE id=readiness date=2026-11-15 ver=02e0ead env=linux-arm64-orbstack src=docs/READINESS.md note="assessed against the letter of the scale on the model-free rows at 02e0ead; the last full run with real models is of 96dd606" -->

| Component | Criteria | Score | Missing for the next point |
|---|---|---|---|
| Event loop and state | 6, 7, 8, 18 | 8 | 10: criterion 20 in the run that checks this file |
| Runs and finisher | 2, 3, 4, 4a, 24, 25 | 6 | 8: criteria 24 (beads and `drain`) and 25 (the return) |
| Triggers and schedules | 5a, 5b, 6 | 8 | 10: criterion 20 in the run that checks this file |
| Issue gates | 10, 19 | 8 | 10: criterion 20 in the run that checks this file; the claude half of criterion 10 is unverified (no claude quota limit seen live) |
| Recovery | 8, 9, 24 | 6 | 8: criterion 24 (`kill -9` of the daemon on every step, no step and no bead run twice) |
| Config assembly | 12, 13, 26 | 6 | 8: criterion 26 (packs from git by sha, tag or the latest) |
| Instance and service registry | 13, 14 | 8 | 10: criterion 20 in the run that checks this file |
| claude adapter | 2, 4a, 9, 11 | 8 | 10: criterion 20 in the run that checks this file |
| codex adapter | 2, 10, 11, 13 | 8 | 10: criterion 20 in the run that checks this file |
| br adapter | 5a, 11, 16, 24 | 6 | 8: criterion 24 (beads with rig, agent and harness labels; `drain` over the tracker) |
| Plugin hook execution | 4, 15 | 8 | 10: criterion 20 in the run that checks this file |
| `default-policy` | 8, 10 | 8 | 10: criterion 20 in the run that checks this file |
| `worktree` | - | 8 | 10: criterion 20 in the run that checks this file; no §15 criterion of its own, its tests are model-free |
| Shared MCP | 14 | 8 | 10: criterion 20 in the run that checks this file; the bridge in front of a real claude session is not checked live |
| HTTP API | 17, 18 | 8 | 10: criterion 20 in the run that checks this file |
| CLI | 17 | 8 | 10: criterion 20 in the run that checks this file |
| MCP frontend | 17 | 8 | 10: criterion 20 in the run that checks this file |
| Acceptance spec | - | 8 | 10: criterion 20 in the run that checks this file |
| Delivery and `doctor` | 1, 23 | 8 | 10: criterion 20 in the run that checks this file; the release builds run on GitHub on tags only |
