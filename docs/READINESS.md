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

Assessed 2026-11-16 at `8963068`, by the letter of the scale. The project root and agent scope (decisions 95, 99) and
the new criteria 28 to 34 opened work, all of it done now: rig prefixes of beads (24, 32), the task input (28), step
repeats with their check script (29), `drain` with `pool` and `on_item_failure` (30), the `formula` step and `internal`
formulas (31), the agent's prompt template with files from the project (33) and the background stopped after an
outcome (34), each with its rows green. At `8963068`, the last change of `crates/`, every row of the spec but those
of criteria 20 and 22 ran with real models (claude haiku, codex `gpt-5.6-luna`, claude 2.1.325, codex-cli 0.168.0,
br 0.7.6): 42 of 42 passed in 536 s, the request rows 1, 2, 4a, 8 and 17 among them. Criterion 22 is this file,
checked by `acceptance-ci` on the same code. Criterion 20 is open: the demos of the current config shape (runs
without a rig, agent scope, `[root]`) have not run live (the last live demo runs were on `c1fa9bc`), so no component
scores 10.
<!-- MEASURE id=full-run date=2026-11-16 ver=8963068 env=linux-arm64-orbstack src=aae-acceptance note="every row of the spec but criteria 20 and 22 with real models, 42 of 42; the live demos of 20 are open, last run on c1fa9bc; 22 and the CI rows of 20 by acceptance-ci on the same code" -->
<!-- MEASURE id=readiness date=2026-11-16 ver=8963068 env=linux-arm64-orbstack src=docs/READINESS.md note="scored by the letter of spec 14 against the full run at this commit and the open part of criterion 20" -->

| Component | Criteria | Score | Missing for the next point |
|---|---|---|---|
| Event loop and state | 6, 7, 8, 18 | 8 | 10: the demos of the current shape run live |
| Runs and finisher | 2, 3, 4, 4a, 24, 25, 28, 29, 30, 31, 34 | 8 | 10: the demos of the current shape run live |
| Triggers and schedules | 5a, 5b, 6 | 8 | 10: the demos run no subscription or schedule; their own rows are green |
| Issue gates | 10, 19 | 8 | 10: the demos hit no gate; the claude half of criterion 10 is unverified (no claude quota limit seen live) |
| Recovery | 8, 9, 24, 29 | 8 | 10: the demos of the current shape run live |
| Config assembly | 12, 13, 26, 27, 31, 32, 33 | 8 | 10: the demos of the current shape run live |
| Instance and service registry | 13, 14 | 8 | 10: the demos of the current shape run live |
| claude adapter | 2, 4a, 9, 11, 34 | 8 | 10: the demos of the current shape run live |
| codex adapter | 2, 10, 11, 13, 34 | 8 | 10: the demos of the current shape run live |
| br adapter | 5a, 11, 16, 24, 28, 32 | 8 | 10: the demos of the current shape run live (criterion 20) |
| Plugin hook execution | 4, 15 | 8 | 10: the demos of the current shape run live |
| `default-policy` | 8, 10 | 8 | 10: the demos of the current shape run live |
| `worktree` | - | 8 | 10: the demos use no worktree; no §15 criterion of its own, its tests are model-free |
| Shared MCP | 14 | 8 | 10: the demos use no shared MCP service; the bridge in front of a real claude session is not checked live |
| HTTP API | 17, 18 | 8 | 10: the demos of the current shape run live |
| CLI | 17 | 8 | 10: the demos of the current shape run live |
| MCP frontend | 17 | 8 | 10: the demos do not use it; a real claude session calls it in criterion 17 |
| Acceptance spec | 28, 29, 30, 31, 33, 34 | 8 | 10: the demo rows of criterion 20 run live with the rest |
| Delivery and `doctor` | 1, 23 | 8 | 10: the demos of the current shape run live from a release build |
