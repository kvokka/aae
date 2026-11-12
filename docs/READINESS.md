# Readiness

Per-component readiness on the 0–10 scale of spec §14: 0 nothing; 2 contract written; 4 code exists and the main
path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15 criteria green on the
owner's host; 10 real owner runs without intervention (criterion 20). Updated at the end of every work session.

Assessed 2026-11-12 on top of `8994286` (the commit before the assessment).
<!-- MEASURE id=readiness date=2026-11-12 ver=8994286 env=linux-arm64-orbstack src=docs/READINESS.md note="self-assessment by the implementing agent; re-assess at the end of every session" -->

| Component | Score | Missing for the next point |
|---|---|---|
| Event loop and state | 0 | contract (WP-08) |
| Runs and finisher | 0 | contract (WP-08) |
| Triggers and schedules | 0 | contract (WP-08) |
| Issue gates | 0 | contract (WP-08) |
| Recovery | 0 | contract (WP-08) |
| Config assembly | 0 | contract (WP-08) |
| Instance and service registry | 0 | contract (WP-08); codex process/thread split settled by experiment 2 |
| claude adapter | 0 | contract (WP-08); protocol and process experiments done (`docs/design/experiments.md`) |
| codex adapter | 0 | contract (WP-08); app-server experiment done (`docs/design/experiments.md`) |
| br adapter | 0 | contract (WP-08); event-source experiment done (`docs/design/experiments.md`) |
| Plugin hook execution | 0 | contract (WP-08) |
| `default-policy` | 0 | contract (WP-08) |
| `worktree` | 0 | contract (WP-08) |
| Shared MCP | 0 | contract (WP-08) |
| HTTP API | 0 | contract (WP-08) |
| CLI | 0 | contract (WP-08); only `aae --version` exists |
| MCP frontend | 0 | contract (WP-08) |
| Acceptance spec | 0 | runner skeleton (WP-24) |
| Delivery and `doctor` | 1 | workspace, CI, budget checks and cross-builds exist; `doctor` contract (WP-08) |
