# Readiness

Per-component readiness on the 0–10 scale of spec §14: 0 nothing; 2 contract written; 4 code exists and the main
path works by hand; 6 model-free tests green, including failure paths; 8 the component's §15 criteria green on the
owner's host; 10 real owner runs without intervention (criterion 20). Updated at the end of every work session.

Assessed 2026-11-13 on top of `8da61bd` (the last code commit assessed).
<!-- MEASURE id=readiness date=2026-11-13 ver=8da61bd env=linux-arm64-orbstack src=docs/READINESS.md note="self-assessment by the implementing agent; re-assess at the end of every session" -->

| Component | Score | Missing for the next point |
|---|---|---|
| Event loop and state | 3 | `crates/aae-core`: single writer, WAL state, outbox with `seq`, durable consumer acknowledgements, JSONL journal (rotation, read from `seq`, gap), idempotency keys, effect queue, deadlines, a system clock for the daemon and a managed one for deterministic checks, the driver trait with a scripted driver (`crates/aae-testkit`), with model-free tests; runs and attempts tables (state schema v3); missing: a daemon to run it by hand (WP-22) |
| Runs and finisher | 3 | `crates/aae-core/src/run`: run state machine of contracts 6.1–6.3 for prompt and exec steps (queue by workdir, orchestrator and agent concurrency limits, preparing with the config snapshot and the rendered prompt, attempts with tokens, deciding events with explicit abstain and the hook deadline, decisions, attempt and remark ceilings, deadlines and silence budgets with the step timeout paused between turns and in requires_action, cancel, hollow turn and `continue`, the finisher with `stale_attempt`/`run_terminal`/`invalid`, `finishing` with accept, veto and remark, `passed`, explicit continuation of another run's session; exec steps with the outcome from the exit code, 75 as an attempt failure, timeout and cancel), tested on the scripted driver and real commands, the step timeout and its pauses on the managed clock; parked runs on gates with `park_timeout`; missing: recovery (WP-20), the `aae finish` command and a manual path (WP-22, WP-23) |
| Triggers and schedules | 2 | code (WP-17); contract: `contracts.md` 2, 3.5; the `where` and `inputs` language is code (`aae-core/src/template.rs`) |
| Issue gates | 3 | `aae-core/src/run/gates.rs`: closures per `(key, closed_by)` in the state, the run's keys (account and model once reported), queued runs wait with the gate as the reason, a start under a closed key parks (invariant 18), `until` timers, operator close/open messages, `gate.*` events, `gates` in `run.attempt_failed`, `park` and a gate on the other `run.attempt_failed` decisions; tested on the managed clock; missing: the bead gate (WP-19), the API and CLI to close and open by hand (WP-22, WP-23) |
| Recovery | 2 | code (WP-20); contract: `contracts.md` 6.4 |
| Config assembly | 3 | code: the files (`aae-core/src/config/`, format in `docs/design/config.md`) with load and validation, the seven layers merged with the layer of every value, "explain", the snapshot value, the driver `split` with fingerprints; model-free tests; missing: a command to explain by hand (WP-23), reload held by the daemon (WP-22) |
| Instance and service registry | 2 | code (WP-18); contract: `contracts.md` 8 |
| claude adapter | 2 | code (WP-27); contract: `contracts.md` 4 |
| codex adapter | 2 | code (WP-29); contract: `contracts.md` 4 |
| br adapter | 2 | code (WP-31); contract: `contracts.md` 5 |
| Plugin hook execution | 3 | `aae-core/src/process.rs`, `run/hooks.rs`: hooks of an event queued with it in one transaction, run as commands (own process group, clean environment, timeout, bounded output), decisions combined per `contracts.md` 3.3, `plugin.hook_failed`; tested with real commands; missing: a daemon to see it by hand (WP-22), in-process built-ins (WP-33), `AAE_SOCKET`/`AAE_PLUGIN_PARAMS` (WP-22, WP-32) |
| `default-policy` | 2 | code (WP-33); contract: `contracts.md` 10 |
| `worktree` | 0 | contract (WP-33); its hook decision shape is fixed (`contracts.md` 3.2) |
| Shared MCP | 0 | contract (WP-34) |
| HTTP API | 0 | contract (WP-22); the finisher call is fixed (`contracts.md` 6.3) |
| CLI | 0 | contract (WP-23); only `aae --version` exists |
| MCP frontend | 0 | contract (WP-25) |
| Acceptance spec | 0 | runner skeleton (WP-24); how fakes reach the daemon is fixed (`contracts.md` 9) |
| Delivery and `doctor` | 1 | workspace, CI, budget checks and cross-builds exist; `doctor` contract (WP-35) |
