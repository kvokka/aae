# Harness updates

aae drives `claude` and `codex` through their own protocols, so every new harness version is checked before it is
pinned. The check is the contract suite of spec §7.8: the real binary against a fake provider API, no model and no
account.

## Updating a harness

1. Pin the new version in `mise.toml` (`mise use claude@<x.y.z>` or `mise use codex@<x.y.z>`) and commit the lock
   file with it.
2. Run the contract suite and the daemon tests of that harness:
   - claude: `cargo nextest run -p aae-claude` and `cargo nextest run -p aae --test claude`;
   - codex: `cargo nextest run -p aae-codex` and `cargo nextest run -p aae --test codex`.
3. A red scenario is a change of the harness protocol or of its failure wording. Fix the adapter (the failure
   classification tables live in `crates/aae-claude/src/stream.rs` and `crates/aae-codex/src/`), never the
   scenario's expectation, unless the harness now reports more than before.
4. When all is green, widen the tested range in the adapter's `capabilities().versions` (outside it the daemon and
   `aae doctor` warn) and add a line to the log below.
5. Optionally, one real turn on the cheapest model of the harness confirms the path end to end.

## What the claude suite covers

`crates/aae-claude/tests/contract.rs`, one test per scenario of spec §7.8:

| # | Scenario | Expected from the adapter |
|---|---|---|
| 1 | 429 twice, then an answer | two `retrying`, the turn completes, no failure |
| 2 | 429 for ever | `rate_limited`, `retried_by_harness` |
| 3 | quota with a reset time | `rate_limited` without `retry_at`: with an API key claude reports no reset time |
| 4 | quota in unknown words | `rate_limited`, by the 429 status |
| 5 | 529 | `overloaded`, `retried_by_harness` |
| 6 | 401 | `auth_required` |
| 7 | no credit (`insufficient_quota`) | `billing` |
| 8 | prompt too long | `context_overflow` |
| 9 | unknown model | `config_error` |
| 10 | `kill -9` in a command | `harness_crash` and `exited` within a second |
| 11 | EOF on stdin in a turn | the turn finishes unobserved, the command's effect happens, claude exits 0 |
| 12 | silent provider | no turn end, no failure (the core's watchdog owns the stall); an interrupt cancels the turn |
| 13 | permission request | `permission.requested` for `Bash`; `allow` runs the command and the turn completes |
| 14 | background command after the turn | idle with `background` first, idle without background after the command |
| 15 | second writer in the session | claude does not refuse it: one writer per session is the core's to keep |

`crates/aae/tests/claude.rs` adds the daemon side: the finisher passes a prompt run, `kill -9` in a command is
typed within a second and resumed with the bead's claim held (criteria 9 and 16), and a finisher called with a
background command running passes the run only after the command (criterion 4a on the fake).

## What the codex suite covers

`crates/aae-codex/tests/contract.rs`, the same 15 scenarios, where codex differs:

| # | Scenario | Expected from the adapter |
|---|---|---|
| 1 | 429 twice, then an answer | codex retries by itself (the fake sends `retry-after`), the turn completes |
| 3 | quota with a reset time | `quota_exhausted` with `retry_at` from the rate-limit snapshot |
| 7 | `insufficient_quota` | codex words it as `usageLimitExceeded` "… check your plan and billing details": `billing` |
| 11 | EOF on stdin in a turn | the app-server exits at once with code 0, the turn left open: `harness_crash` |
| 14 | command sent to the background | codex tracks no background tasks: the turn ends, idle without background, no turn follows |
| 15 | second writer in the thread | refused ("already has an active writer"): `transcript_divergence` |

The rest match claude: rate limits, overload, 401, context, unknown model, `kill -9`, silence and permission.

## Log

| Date | Harness | Version | Result |
|---|---|---|---|
| 2026-11-13 | claude | 2.1.322 | contract suite green, first pin |
| 2026-11-13 | codex | 0.167.2 | contract suite green, first pin |
