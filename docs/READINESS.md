# Readiness

Per-component readiness on the 0–10 scale of spec §14, scored by its letter: 0 nothing; 2 contract written; 4 code
exists and the main path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15
criteria green on the owner's host; 10 real owner runs without intervention (criterion 20). An open criterion of a
component caps its score. Updated at the end of every work session.

Assessed 2026-11-14 at `5c76ba2`: the `kill -9` tests of criterion 9 green in 20 full runs of the suite in a row
at `4d32b10`, with the failure typed by the time of its own event. Live, on claude haiku and codex
gpt-5.6-luna: the request row 2 green at `5c76ba2` (3.8 s and 6.7 s), the request rows 2, 4a, 8 and 17 green at
`495f0e6`, and both demos of criterion 20 passed from empty rigs to a green smoke test at `775ffcb` and again at
`495f0e6` (`demos/*/RESULT.md`); the whole `mise run acceptance` was last run at `5c5014f`, 21 of 21. No component is at 10 while the owner has not run aae
for real without intervention.
<!-- MEASURE id=readiness date=2026-11-14 ver=5c76ba2 env=linux-arm64-orbstack src=docs/READINESS.md note="self-assessment by the implementing agent against the letter of the scale; re-assess at the end of every session" -->

| Component | Score | Missing for the next point |
|---|---|---|
| Event loop and state | 8 | 10: real owner runs without intervention (criterion 20) |
| Runs and finisher | 8 | 10: criterion 20 |
| Triggers and schedules | 8 | 10: criterion 20 |
| Issue gates | 8 | 10: criterion 20 |
| Recovery | 8 | 10: criterion 20 |
| Config assembly | 8 | 10: criterion 20 |
| Instance and service registry | 8 | 10: criterion 20; a green GitHub CI run of the service tests is still to be seen |
| claude adapter | 8 | 10: criterion 20; the claude half of criterion 10 (a lost model limit) is not yet observed live |
| codex adapter | 8 | 10: criterion 20 |
| br adapter | 8 | 10: criterion 20 |
| Plugin hook execution | 8 | 10: criterion 20 |
| `default-policy` | 8 | 10: criterion 20 |
| `worktree` | 8 | 10: criterion 20 |
| Shared MCP | 8 | 10: criterion 20; a green GitHub CI run of the service tests is still to be seen |
| HTTP API | 8 | 10: criterion 20 |
| CLI | 8 | 10: criterion 20 |
| MCP frontend | 8 | 10: criterion 20 |
| Acceptance spec | 8 | 10: criterion 20; a green GitHub CI run of the whole set is still to be seen |
| Delivery and `doctor` | 8 | 10: criterion 20 |
