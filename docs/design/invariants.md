# Core invariants and recovery

The rules the core keeps whatever the harness, the policy, the operator or a crash does. They are numbered; code,
tests and the other design documents cite them as "invariant N", so the numbers are stable: an invariant is never
renumbered and its number is never reused. The list is distilled from the fix
history of an earlier orchestrator of the same kind, where about half of the commits were fixes: 1 609 of 3 156
since 2026-07-01 (counted 2026-11-10 at its commit `f1f75f53c`).
<!-- MEASURE id=prior-orchestrator-fix-commits date=2026-11-10 ver=f1f75f53c env=linux-arm64-orbstack src=reports/21 note="git log --since=2026-07-01: 3156 commits, 1609 of them fixes; one manual pass over the subjects" -->

All of them except 19 are mechanisms of the core and are never cut to fit a line budget. Thresholds (how many
times, how long) are config parameters with defaults; the choice of a reaction is policy (`default-policy`,
[contracts](contracts.md) 10).

## The invariants

| № | Invariant | Where it lives |
|---|---|---|
| 1 | One writer of the state: the daemon | `aae-core/src/writer.rs`, [event model](event-model.md) |
| 2 | Ownership with a generation: a run and a bead have at most one live owner; every effect of an attempt (finisher, close, release) checks the generation, a stale one is dropped | contracts 6.2, 6.3, 5.1 |
| 3 | The daemon claims a bead in the same transaction as the decision to launch; an agent never claims | contracts 5.1, 6.2 |
| 4 | A bead is released only conditionally: owner and generation compared | contracts 5.1 |
| 5 | A session's death loses no work: exit or EOF moves the run to "attempt failed" in one transaction; the bead stays with the run until the run is terminal | contracts 6.2, 4.4 |
| 6 | Confirmed death before replacement: a new process for the same run or session starts only after nothing is left of the instance it replaces, its marked descendants included; a live shared instance is left alone | contracts 8 |
| 7 | No descendant of a stopped session survives. A process group is not enough (a claude child orphaned with its own group after `kill -9` kept working); the daemon is a child subreaper and sweeps by the instance marker. Limit: a descendant that dropped the environment (`env -i`, `sudo` without `-E`, a container runtime) carries no marker, so the sweep misses it; the subreaper only reaps it once it exits on its own | contracts 8, [experiments](experiments.md#descendants-after-kill--9-invariant-7) |
| 8 | One writer per harness session, the daemon: while a run lives nobody else writes into its session | contracts 4 |
| 9 | Intent before effect: launch, stop and kill are recorded first and executed after; recovery completes or rolls back | [event model](event-model.md), contracts 8 |
| 10 | Idempotency by key (run, attempt): create, issue, outcome and event are repeatable without duplicates | contracts 1, 6.2; `Idempotency-Key` of the API |
| 11 | An unfinished turn does not wait silently after a daemon restart: it is interrupted and a new attempt follows (reconnection is not done in the first release) | contracts 6.4 |
| 12 | Success only by an explicit outcome: a turn ending without the finisher is not success | contracts 6.3 |
| 13 | An interruption by the infrastructure (cancel, restart, limit) is not an outcome of the formula; when attempts run out the run fails with class `infrastructure`, distinct from a fail by the agent or by a veto | contracts 6.1, 6.2 |
| 14 | Every transitional state has a deadline and a terminal exit | contracts 6.1 |
| 15 | A watchdog on an issued run: a deadline to the first event and a silence budget that depends on the state. Waiting for a human is not silence; a running tool and background subagents have their own, longer budget. A retry the harness announces adds its pause to the limit. The core raises the failure `stall`, the policy chooses the reaction. The thresholds are `[watchdog]` of the orchestrator config (config.md) | contracts 6.1, 4.3 |
| 16 | Attempt and restart counters live in the state and are bounded; the core has an absolute ceiling the policy cannot raise (20 failed attempts per run over all its steps, 10 remarks per attempt); an attempt whose `auth_required`, `billing` or `quota_exhausted` failure parked the run on its own `account:` gate, closed by that decision, is outside the attempts ceiling (spec §2, decisions 49, 50); any other park counts. Limit: a policy may still close that gate for a moment and retry at once — every such round is a real attempt of the harness | contracts 6.1 |
| 17 | Resume is checked: only when the adapter confirmed the transcript exists. "New session keeping the ownership" is a core mechanism; after how many refused resumes to switch to it is policy | contracts 6.2, 6.4 |
| 18 | An operator hold beats automation: a pause or cancel by a human is a record in the state read by every automatic decision | contracts 6.2, 7 |
| 19 | A worktree has one owner and is removed only when its commits are reachable from refs (the `worktree` plugin) | `aae-plugins/src/worktree.rs` |
| 20 | An event is a reason to re-read the state; resync is mandatory; ambiguity means zero effects and a loud `resync.diverged` (a repeated or skipped driver `seq` included) | [event model](event-model.md) |

How they are checked: the deterministic chaos stand (`aae-testkit/tests/chaos`) plays seeded sequences of
commands, harness behaviour, policy hooks, operator actions, lost and repeated deliveries and daemon crashes, and
checks invariants 2, 3, 10, 12, 13, 14, 15, 16 and 17 on the state and the journal after every step. Its formulas
have several steps, and one starts with a `drain` step over a root bead with two children; besides the invariants
the stand checks the steps (S1–S4: each step finishes once and in order, attempts follow the steps, no session
serves two steps, a run passes from its last step) and the drain step (D1: a bead gets at most one child run per
entry into the step; D2: once the step finished, no child run on a bead is alive; D3: the attempt of a drain step
that a crash of the daemon caught ends with `daemon_crash`). Each full share of seeds kills the daemon mid-drain at least once. Every run
plays new seeds from a random base it prints; a failure names its seed and `AAE_CHAOS_SEED=<n>` replays it.
Invariant 6 runs real marked processes (`aae-testkit/tests/old_instances.rs`, `process::tests::wait_gone_*`). The
others are covered by the integration suites of `aae-testkit/tests` and by the acceptance scenarios (criterion 8:
20 `kill -9` of the daemon mid-turn in CI and 5 on each real harness; criterion 24: a `kill -9` of the daemon in
each step of a formula of `exec`, `drain` and `exec` steps on the real br, no step and no bead run twice).

## Recovery

At daemon start ([contracts](contracts.md) 6.4 has the exact state rules):

1. Open the state and drain the outbox.
2. For every recorded harness instance check pid + start time, kill its process group, sweep every process with
   its marker and wait for the end. Instances that survived the restart are not reconnected in the first release:
   reconnection is an assumption left for later. Then the hook processes the dead daemon left running (the hooks
   still queued) are killed by their `AAE_INSTANCE=h-…` markers in one sweep, before any effect of the new daemon
   starts; a hook that runs again may start before the old process is gone, so hooks are idempotent.
3. Runs with a live attempt — `starting`, `running` in a turn, `requires_action` — move to `interrupted` with the
   attempt failure `daemon_crash`, the deciding hooks they still had queued dropped (after the sweep of item 2); the
   policy decides what follows. A run waiting for a decision on a deciding
   event (`preparing`, `interrupted`, `finishing`, `running` after a turn without an outcome) keeps waiting for it:
   its hooks run again, the declared outcome and an exec command's result stand. A `remark` or `continue` then
   fails the attempt with `daemon_crash`, as the session it needs is gone (contracts 3.4, 6.4). A run goes on from
   the step it is at: the move to the next step commits with the accepted outcome of the step before, so a finished
   step never runs again, and an exec command the restart killed runs again in a new attempt of its step. The
   attempt of a `drain` step is interrupted as any: its next attempt gives out no bead the entry into the step has
   already given (their runs recover as any run).
4. The deadlines that wait for an effect's answer (a decision, the claim of a bead) count again from the start;
   the deadlines of real time stay, and the schedules are armed (a missed schedule fires at most once). Resync, the
   process table included: what carries the marker of an instance this state no longer records is swept.

A planned restart drains first (`POST /v1/daemon/drain`, `POST /v1/daemon/restart`, `aae drain`, `aae restart`):
no new run starts and no new attempt either (a retry decided meanwhile parks with `run.parked{gate: null}` and the
next daemon starts it, `run.resumed{gate_key: "daemon_stopping"}`), the active runs — a run in a turn, a hook, a
policy decision or a preparation — go on until none is left or the drain timeout (default 300 s) passes; a `remark`
or `continue` at the turn boundary starts no turn but fails the attempt with `daemon_crash`, so its retry parks
with the remark kept for the next daemon. Then the daemon stops (`daemon.stopping{reason}`) and recovery resumes
what the drain cut.

Prompt delivery is "at least once with an idempotent effect": the per-attempt record `intent → sent → acked →
settled` and the prompt key the harness keeps in its transcript (contracts 6.4). Harness protocols give no
idempotency of their own.

Measured on the owner's host (2026-11-13, aae 0.1.0; criterion 8, 20 kills mid-step): after a `kill -9` the daemon
answers again within 0.13 s, far under the 30 s bound; every run reaches its end exactly once, every trigger fires once,
no orphan process is left.
<!-- MEASURE id=crash-restart date=2026-11-13 ver=aae-0.1.0 env=linux-arm64-orbstack src=aae-acceptance note="criterion 8 CI scenario, slowest restart to the first answer of 20; 0.03-0.13 s across runs" -->

## Limits

- macOS is not checked in the first release. Known differences that will surface at the first check: no
  subreaper, no `/proc` and no cgroup (so no marker sweep), `PR_SET_PDEATHSIG` (which codex uses to kill its
  child commands) exists only on Linux, and file watching and credential storage differ. No code is written for
  them ahead of that check.
