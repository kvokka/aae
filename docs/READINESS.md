# Readiness

Per-component readiness on the 0–10 scale of spec §14, scored by its letter: 0 nothing; 2 contract written; 4 code
exists and the main path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15
criteria green on the owner's host; 10 real owner runs without intervention (criterion 20). An open criterion of a
component caps its score (§2, decision 53): a criterion counts as open when a row of it failed, or was not run on the
code assessed. Updated at the end of every work session.

`Criteria` lists the §15 criteria of each component; criterion 22 of `aae-acceptance` checks every score against them:
one with a `FAIL` row, an `open` row (a part of the criterion no row checks yet, `aae-acceptance/src/readiness.rs`),
no row, or a row not run holds the score at 6. In a CI run, which runs no model, a row not run counts for the criteria
§15 checks by a request (1, 2, 4a, 8, 20) while the `full-run` marker below, the last full run, names a commit older
than the last change of `crates/`. A 10 needs criterion 20 not open, which in a full run means every row of it green;
and the `readiness` marker must name a commit at or after the last change of `crates/`.

Assessed 2026-11-16 at `01851cb`, by the letter of the scale. The last change of `crates/` is `4b03eae`; at
`01851cb` every row of the spec ran with real models in one full run (claude haiku 2.1.325, codex `gpt-5.6-luna`
0.168.0, br 0.7.6): 47 of 49 rows passed in 1335 s. The request rows 1 (this host's release build), 2, 4a, 8 and 17
passed, and so did both demos of the current config shape: demo 1 (planner and frontend coder on claude haiku, backend
coder on codex luna) in 423 s, demo 2 (planner on sol, coders on luna, qa on terra) in 658 s and one round, its second
run in a row after one at `6699f51` (480 s, one round). Of the other two rows, 22 failed on this file, then still
assessed at `8963068`, and criterion 1's build for the four release targets runs only on a tag (`on a tag`).
Criterion 20 has no open part now. The components the demos exercise score 10; the others stay at 8, with what is
missing in their row.
<!-- MEASURE id=full-run date=2026-11-16 ver=01851cb env=linux-arm64-orbstack src=aae-acceptance note="every row of the spec with real models, 47 of 49: 22 failed on the readiness file then at 8963068, criterion 1's four-target build is for a tag; both live demos passed" -->
<!-- MEASURE id=readiness date=2026-11-16 ver=01851cb env=linux-arm64-orbstack src=docs/READINESS.md note="scored by the letter of spec 14 against the full run at this commit" -->

| Component | Criteria | Score | Missing for the next point |
|---|---|---|---|
| Event loop and state | 6, 7, 8, 18 | 10 | - |
| Runs and finisher | 2, 3, 4, 4a, 24, 25, 28, 29, 30, 31, 34 | 10 | - |
| Triggers and schedules | 5a, 5b, 6 | 8 | 10: the demos run no subscription or schedule; their own rows are green |
| Issue gates | 10, 19 | 8 | 10: the demos hit no gate; the claude half of criterion 10 is unverified (no claude quota limit seen live) |
| Recovery | 8, 9, 24, 29 | 10 | - |
| Config assembly | 12, 13, 26, 27, 31, 32, 33 | 10 | - |
| Instance and service registry | 13, 14 | 10 | - |
| claude adapter | 2, 4a, 9, 11, 34 | 10 | - |
| codex adapter | 2, 10, 11, 13, 34 | 10 | - |
| br adapter | 5a, 11, 16, 24, 28, 32 | 10 | - |
| Plugin hook execution | 4, 15 | 10 | - |
| `default-policy` | 8, 10 | 10 | - |
| `worktree` | - | 8 | 10: the demos use no worktree; no §15 criterion of its own, its tests are model-free |
| Shared MCP | 14 | 8 | 10: the demos use no shared MCP service; the bridge in front of a real claude session is not checked live |
| HTTP API | 17, 18 | 10 | - |
| CLI | 17 | 10 | - |
| MCP frontend | 17 | 8 | 10: the demos do not use it; a real claude session calls it in criterion 17 |
| Acceptance spec | 28, 29, 30, 31, 33, 34 | 10 | - |
| Delivery and `doctor` | 1, 23 | 8 | 10: the demos run from a release build; they ran on the debug build |
