# Readiness

Per-component readiness on the 0–10 scale of spec §14, scored by its letter: 0 nothing; 2 contract written; 4 code
exists and the main path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15
criteria green on the owner's host; 10 real owner runs without intervention (criterion 20). An open criterion of a
component caps its score (§2, decision 53): a criterion counts as open when a row of it failed, or was not run on the
code assessed. Updated at the end of every work session.

`Criteria` lists the §15 criteria of each component; criterion 22 of `aae-acceptance` checks every score against
them: one with a `FAIL` row (or, in a full run, a row not run) holds the score at 6, a 10 needs criterion 20 green,
and the marker below must name a commit at or after the last change of `crates/`.

Assessed 2026-11-15 at `96dd606`, the code of `crates/` since `20f8d4c`. The full `mise run acceptance` ran on it
with the cheapest real models (claude haiku, codex gpt-5.6-luna): 30 of 32 rows passed, the live rows of 2, 4a and 17
and both demos of criterion 20 included. The codex row of criterion 8 failed once because codex could not reach its
backend (`run.retrying` "Reconnecting... waiting for network" three times, so the model never started the command),
and row 22 failed only because criterion 8 was then open; a rerun of criterion 8 passed all three rows (claude 5 of 5,
codex 5 of 5). Every component scores 8. None scores 10 though both demos passed: criterion 22 checks this file on
every commit without the demos (CI runs no model), and a 10 needs criterion 20 green in the run that checks it.
<!-- MEASURE id=readiness date=2026-11-15 ver=96dd606 env=linux-arm64-orbstack src=docs/READINESS.md note="assessed against the letter of the scale after a full run with real models on 96dd606 and a rerun of criterion 8" -->

| Component | Criteria | Score | Missing for the next point |
|---|---|---|---|
| Event loop and state | 6, 7, 8, 18 | 8 | 10: criterion 20 in the run that checks this file |
| Runs and finisher | 2, 3, 4, 4a | 8 | 10: criterion 20 in the run that checks this file |
| Triggers and schedules | 5a, 5b, 6 | 8 | 10: criterion 20 in the run that checks this file |
| Issue gates | 10, 19 | 8 | 10: criterion 20 in the run that checks this file; the claude half of criterion 10 is unverified (no claude quota limit seen live) |
| Recovery | 8, 9 | 8 | 10: criterion 20 in the run that checks this file |
| Config assembly | 12, 13 | 8 | 10: criterion 20 in the run that checks this file |
| Instance and service registry | 13, 14 | 8 | 10: criterion 20 in the run that checks this file |
| claude adapter | 2, 4a, 9, 11 | 8 | 10: criterion 20 in the run that checks this file |
| codex adapter | 2, 10, 11, 13 | 8 | 10: criterion 20 in the run that checks this file |
| br adapter | 5a, 11, 16 | 8 | 10: criterion 20 in the run that checks this file |
| Plugin hook execution | 4, 15 | 8 | 10: criterion 20 in the run that checks this file |
| `default-policy` | 8, 10 | 8 | 10: criterion 20 in the run that checks this file |
| `worktree` | - | 8 | 10: criterion 20 in the run that checks this file; no §15 criterion of its own, its tests are model-free |
| Shared MCP | 14 | 8 | 10: criterion 20 in the run that checks this file; the bridge in front of a real claude session is not checked live |
| HTTP API | 17, 18 | 8 | 10: criterion 20 in the run that checks this file |
| CLI | 17 | 8 | 10: criterion 20 in the run that checks this file |
| MCP frontend | 17 | 8 | 10: criterion 20 in the run that checks this file |
| Acceptance spec | - | 8 | 10: criterion 20 in the run that checks this file |
| Delivery and `doctor` | 1, 23 | 8 | 10: criterion 20 in the run that checks this file; the release builds run on GitHub on tags only |
