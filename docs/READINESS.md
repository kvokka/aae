# Readiness

Per-component readiness on the 0–10 scale of spec §14, scored by its letter: 0 nothing; 2 contract written; 4 code
exists and the main path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15
criteria green on the owner's host; 10 real owner runs without intervention (criterion 20). An open criterion of a
component caps its score (§2.1 item 16): a criterion counts as open when a row of it failed, or was not run on the
code assessed. Updated at the end of every work session.

`Criteria` lists the §15 criteria of each component; criterion 22 of `aae-acceptance` checks every score against
them: one with a `FAIL` row (or, in a full run, a row not run) holds the score at 6, a 10 needs criterion 20 green,
and the marker below must name a commit at or after the last change of `crates/`.

Assessed 2026-11-14 at `7942f5e`. The model-free rows and the cargo tests were green on the code before this
session's changes and are checked again by the CI run of this commit; criterion 8 passed 10 of 10 runs under a load
of 8 busy cores at `1dee5b0`, its orphan check now able to fail. The rows with a real model (2, 4a, the live rows of
8 and 17) and the release builds (1) were last green at `495f0e6` and earlier: since then the core's recovery, both
harness adapters, the config and the CLI changed, and no full `mise run acceptance` ran on this code (the last full
run was at `5c5014f`). The components with such a criterion stay at 6 until a full run shows it green.
<!-- MEASURE id=readiness date=2026-11-14 ver=3af4457 env=linux-arm64-orbstack src=docs/READINESS.md note="assessed against the letter of the scale; the model rows are open until a full run on this code" -->

| Component | Criteria | Score | Missing for the next point |
|---|---|---|---|
| Event loop and state | 6, 7, 8, 18 | 6 | 8: the live rows of criterion 8 (`kill -9` mid-turn of claude and codex) on this code |
| Runs and finisher | 2, 3, 4, 4a | 6 | 8: rows 2 and 4a with a real model on this code |
| Triggers and schedules | 5a, 5b, 6 | 8 | 10: criterion 20; 5b runs on the wall clock, not a managed one |
| Issue gates | 10, 19 | 8 | 10: criterion 20; a model gate is learned only from a run's own failure |
| Recovery | 8, 9 | 6 | 8: the live rows of criterion 8 on this code (recovery changed in this session) |
| Config assembly | 12, 13 | 8 | 10: criterion 20 |
| Instance and service registry | 13, 14 | 8 | 10: criterion 20 |
| claude adapter | 2, 4a, 9, 11 | 6 | 8: rows 2 and 4a on this code (the adapter changed in this session) |
| codex adapter | 2, 10, 11, 13 | 6 | 8: row 2 on this code (the adapter changed in this session) |
| br adapter | 5a, 11, 16 | 8 | 10: criterion 20 |
| Plugin hook execution | 4, 15 | 8 | 10: criterion 20 |
| `default-policy` | 8, 10 | 6 | 8: the live rows of criterion 8, where it decides the resume of a real turn |
| `worktree` | - | 8 | 10: criterion 20; no §15 criterion of its own, its tests are model-free |
| Shared MCP | 14 | 8 | 10: criterion 20; the bridge in front of a real claude session is not checked live |
| HTTP API | 17, 18 | 6 | 8: the live row of criterion 17 on this code |
| CLI | 17 | 6 | 8: the live row of criterion 17 on this code |
| MCP frontend | 17 | 6 | 8: the live row of criterion 17 on this code |
| Acceptance spec | - | 6 | 8: a full run of every row on this code; its checks of 8 and 22 could not fail before |
| Delivery and `doctor` | 1, 23 | 6 | 8: the release builds of criterion 1 on this code; never run on GitHub (tags only) |
