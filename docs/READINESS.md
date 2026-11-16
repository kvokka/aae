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

Assessed 2026-11-16 at `db441b4`, by the letter of the scale. The project root and agent scope (decisions 95, 99) and
the new criteria 28 to 34 opened work that is not done: step repeats (29), `drain` with `pool` (30), the `formula` step (31), agent files from the project (33), the background
after an outcome (34); rig prefixes of beads (24, 32) and the task input (28) are done and their rows green. Their components score 6: their model-free tests are green, but a criterion of theirs is
open. Criterion 20 is open too: the demos of the current config shape (runs without a rig, agent scope, `[root]`)
have not run live, so no component scores 10. The last full run with real models was on `ffaccea` (37 of 37 rows);
the live demo runs were on `c1fa9bc` (demo 1 on claude haiku and codex luna, demo 2 on codex sol, luna and terra).
Both are older than the config shape of today and count as history, not as a measure of this code: criteria 1, 2,
4a and 8 have not run on it, which holds the event loop, `default-policy` and delivery at 6 too.
<!-- MEASURE id=full-run date=2026-11-15 ver=ffaccea env=linux-arm64-orbstack src=aae-acceptance note="the last run of the whole spec with real models, 37 of 37 rows" -->
<!-- MEASURE id=readiness date=2026-11-16 ver=db441b4 env=linux-arm64-orbstack src=docs/READINESS.md note="scored by the letter of spec 14 against the CI rows of acceptance-ci at this commit and the open parts of criteria 20, 29 to 31, 33 and 34; no live run on this code" -->

| Component | Criteria | Score | Missing for the next point |
|---|---|---|---|
| Event loop and state | 6, 7, 8, 18 | 6 | 8: criterion 8 on real harnesses on the current code |
| Runs and finisher | 2, 3, 4, 4a, 24, 25, 28, 29, 30, 31, 34 | 6 | 8: step repeats (29), `drain` `pool` (30), the `formula` step (31), the background after an outcome (34); 2 and 4a on the current code |
| Triggers and schedules | 5a, 5b, 6 | 8 | 10: the demos run no subscription or schedule; their own rows are green |
| Issue gates | 10, 19 | 8 | 10: the demos hit no gate; the claude half of criterion 10 is unverified (no claude quota limit seen live) |
| Recovery | 8, 9, 24, 29 | 6 | 8: a repeat count that survives `kill -9` (29); criterion 8 on real harnesses on the current code |
| Config assembly | 12, 13, 26, 27, 31, 32, 33 | 6 | 8: agent files from the project (33), `internal` formulas (31) |
| Instance and service registry | 13, 14 | 8 | 10: the demos of the current shape run live |
| claude adapter | 2, 4a, 9, 11, 34 | 6 | 8: the background stopped after a declared outcome (34) |
| codex adapter | 2, 10, 11, 13, 34 | 6 | 8: the background stopped after a declared outcome (34) |
| br adapter | 5a, 11, 16, 24, 28, 32 | 8 | 10: the demos of the current shape run live (criterion 20) |
| Plugin hook execution | 4, 15 | 8 | 10: the demos of the current shape run live |
| `default-policy` | 8, 10 | 6 | 8: criterion 8 on real harnesses on the current code |
| `worktree` | - | 8 | 10: the demos use no worktree; no §15 criterion of its own, its tests are model-free |
| Shared MCP | 14 | 8 | 10: the demos use no shared MCP service; the bridge in front of a real claude session is not checked live |
| HTTP API | 17, 18 | 8 | 10: the demos of the current shape run live |
| CLI | 17 | 8 | 10: the demos of the current shape run live |
| MCP frontend | 17 | 8 | 10: the demos do not use it; a real claude session calls it in criterion 17 |
| Acceptance spec | 28, 29, 30, 31, 33, 34 | 6 | 8: a CI row for each of the new criteria 29 to 31, 33 and 34 |
| Delivery and `doctor` | 1, 23 | 6 | 8: criterion 1, the four release builds, on the current code |
