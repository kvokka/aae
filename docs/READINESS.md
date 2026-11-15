# Readiness

Per-component readiness on the 0–10 scale of spec §14, scored by its letter: 0 nothing; 2 contract written; 4 code
exists and the main path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15
criteria green on the owner's host; 10 real owner runs without intervention (criterion 20). An open criterion of a
component caps its score (§2, decision 53): a criterion counts as open when a row of it failed, or was not run on the
code assessed. Updated at the end of every work session.

`Criteria` lists the §15 criteria of each component; criterion 22 of `aae-acceptance` checks every score against them:
one with a `FAIL` row (or, in a full run, a row not run) holds the score at 6, a 10 needs criterion 20 green in a full
run (a CI run cannot run the demos and refuses a 10 only on a failed row of it), and the marker below must name a commit
at or after the last change of `crates/`.

Assessed 2026-11-16 at `15c7d3a`, the last change of `crates/` (a test that exec steps get `AAE_SOCKET`; criterion 22
judges a 10 like the other caps), for the full run with real models on the commit after it: the scores of 10 below stand
only if that run is green, criterion 20 with both demos included; row 22 of the run checks them. A 10 goes to a
component the demos run through end to end: a request, the planner's beads, `drain` to coders on claude and codex, demo
2's `format` and `qa` checks, which send the work back when one fails. A component the demos do not reach stays at 8:
its own criteria are green, but no unattended demo run goes through it.
<!-- MEASURE id=readiness date=2026-11-16 ver=15c7d3a env=linux-arm64-orbstack src=docs/READINESS.md note="scored for the full run with real models after 15c7d3a, both demos included" -->

| Component | Criteria | Score | Missing for the next point |
|---|---|---|---|
| Event loop and state | 6, 7, 8, 18 | 10 | - |
| Runs and finisher | 2, 3, 4, 4a, 24, 25 | 10 | - |
| Triggers and schedules | 5a, 5b, 6 | 8 | 10: the demos run no subscription or schedule; their own rows are green |
| Issue gates | 10, 19 | 8 | 10: the demos hit no gate; the claude half of criterion 10 is unverified (no claude quota limit seen live) |
| Recovery | 8, 9, 24 | 8 | 10: the demos run without a crash; criterion 8 kills the daemon under real models |
| Config assembly | 12, 13, 26 | 10 | - |
| Instance and service registry | 13, 14 | 10 | - |
| claude adapter | 2, 4a, 9, 11 | 10 | - |
| codex adapter | 2, 10, 11, 13 | 10 | - |
| br adapter | 5a, 11, 16, 24 | 10 | - |
| Plugin hook execution | 4, 15 | 10 | - |
| `default-policy` | 8, 10 | 10 | - |
| `worktree` | - | 8 | 10: the demos use no worktree; no §15 criterion of its own, its tests are model-free |
| Shared MCP | 14 | 8 | 10: the demos use no shared MCP service; the bridge in front of a real claude session is not checked live |
| HTTP API | 17, 18 | 10 | - |
| CLI | 17 | 10 | - |
| MCP frontend | 17 | 8 | 10: the demos do not use it; a real claude session calls it in criterion 17 |
| Acceptance spec | - | 10 | - |
| Delivery and `doctor` | 1, 23 | 8 | 10: the demos run a debug build; the release builds run on GitHub on tags only |
