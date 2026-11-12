# Readiness

Per-component readiness on the 0–10 scale of spec §14: 0 nothing; 2 contract written; 4 code exists and the main
path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15 criteria green on the
owner's host; 10 real owner runs without intervention (criterion 20). Updated at the end of every work session.

Assessed 2026-11-13 on top of `1ae3394` (the last code commit assessed).
<!-- MEASURE id=readiness date=2026-11-13 ver=1ae3394 env=linux-arm64-orbstack src=docs/READINESS.md note="self-assessment by the implementing agent; re-assess at the end of every session" -->

| Component | Score | Missing for the next point |
|---|---|---|
| Event loop and state | 3 | `crates/aae-core`: single writer, WAL state, outbox with `seq`, durable consumer acknowledgements, JSONL journal (rotation, read from `seq`, gap), idempotency keys, effect queue, deadlines, a system clock for the daemon and a managed one for deterministic checks, the driver trait with a scripted driver (`crates/aae-testkit`), with model-free tests; missing: runtime tables of runs and attempts (WP-14) and a daemon to run it by hand (WP-22) |
| Runs and finisher | 2 | code: state machine and finisher (WP-14); contract: `contracts.md` 6 |
| Triggers and schedules | 2 | code (WP-17); contract: `contracts.md` 2, 3.5; the `where` and `inputs` language is code (`aae-core/src/template.rs`) |
| Issue gates | 2 | code (WP-16); contract: `contracts.md` 7 |
| Recovery | 2 | code (WP-20); contract: `contracts.md` 6.4 |
| Config assembly | 3 | code: the files (`aae-core/src/config/`, format in `docs/design/config.md`) with load and validation, the seven layers merged with the layer of every value, "explain", the snapshot value, the driver `split` with fingerprints; model-free tests; missing: a command to explain by hand (WP-23), reload held by the daemon (WP-22) |
| Instance and service registry | 2 | code (WP-18); contract: `contracts.md` 8 |
| claude adapter | 2 | code (WP-27); contract: `contracts.md` 4 |
| codex adapter | 2 | code (WP-29); contract: `contracts.md` 4 |
| br adapter | 2 | code (WP-31); contract: `contracts.md` 5 |
| Plugin hook execution | 2 | code (WP-15); contract: `contracts.md` 3 |
| `default-policy` | 2 | code (WP-33); contract: `contracts.md` 10 |
| `worktree` | 0 | contract (WP-33); its hook decision shape is fixed (`contracts.md` 3.2) |
| Shared MCP | 0 | contract (WP-34) |
| HTTP API | 0 | contract (WP-22); the finisher call is fixed (`contracts.md` 6.3) |
| CLI | 0 | contract (WP-23); only `aae --version` exists |
| MCP frontend | 0 | contract (WP-25) |
| Acceptance spec | 0 | runner skeleton (WP-24); how fakes reach the daemon is fixed (`contracts.md` 9) |
| Delivery and `doctor` | 1 | workspace, CI, budget checks and cross-builds exist; `doctor` contract (WP-35) |
