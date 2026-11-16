# Test rules

## What to run, and when

| When | What | Command |
|---|---|---|
| while you work | the tests of what you touch, clippy of the crate | `cargo nextest run -p <crate> -E 'test(<name>)'`, `cargo clippy -p <crate> --all-targets -q -- -D warnings` |
| once, on the committed HEAD, before you hand the work over | **the gate** | `mise run ci` (in the loop: `scripts/loop/checks NN`) |
| only when the owner asks | acceptance scenarios, live models, the demos, release builds | `mise run acceptance-ci`, `mise run acceptance`, `demos/<demo>/run.sh`, `mise run acceptance-release` |

- `mise run ci` is the whole gate: formatting, clippy, the unit and integration tests, `cargo deny`, `cargo machete`
  and the scripts of `scripts/`. A change is done when it is green on the HEAD you hand over. Do not run it after
  every edit: targeted tests are the everyday loop.
- [Acceptance scenarios](../acceptance.md) and the demos are never a condition of "done" and are never run on your
  own judgement, however large the change. They run when the owner's request, the plan package or a journal entry of
  the owner names them, and only the part it names. You may say in your report that a change deserves a scenario; you
  do not run it.
- Fast tests come before new features: a slow or flaky suite is fixed before anything new is built on it. A test
  that grows the time of the gate without a reason is a defect.
- While iterating on the core you may play fewer chaos seeds: `AAE_CHAOS_SEEDS=96 cargo nextest run -p aae-testkit
  --test chaos`. The gate plays its full count.
- Every command that may not return (a build, tests, a daemon, a harness, `aae run wait`, the network, a wait for a
  file or a port) runs with an explicit limit: the tool's `timeout` or `timeout <sec> <command>`. A timeout that
  fired is a finding: write down what hung, do not repeat the command without a limit.
- No second `CARGO_TARGET_DIR` and no git worktree of this repository: the disk is nearly full. Do not run
  `mise run test` and an acceptance run side by side: each rewrites `target/debug/aae` under the other's daemons.
- Command output is always cut: `cargo nextest run --no-fail-fast --status-level fail --final-status-level fail
  2>&1 | tail -80`.

## Live models

- CI has no access to a model, by design. A run on a real `claude` or `codex` happens only on the owner's request,
  on the cheapest model (claude: haiku; codex: the cheapest available), narrowly: before it you know what is being
  checked, after it the journal has an entry (what, how many runs, the result).
- The demos run on the models the owner named: demo 1 on `haiku` and `luna`, demo 2 on `sol`, `terra`, `luna`.
- The model `astra` is never run, anywhere.
- Burning quota on purpose for the sake of a check is forbidden.
- Do not write "codex is not logged in" as a fact without `codex login status` and one cheap run.

## The demos

- `demos/` is frozen: it changes only on the owner's request. A finished demo is not reworked, re-run or re-recorded
  because the code changed.
- If your change would stop a demo's config from loading, say so in your report and in the journal; do not edit the
  demo.
- When the owner does ask: a demo's config is a sample a user writes their own from, so it holds nothing extra or
  unclear; everything a run creates is in `.gitignore`; the author's result lies beside it with the suffix
  `-example` and a `MEASURE` marker saying when and on what it was made (date, versions, models); a demo's
  dependencies are in its own `mise.toml` without a lock.

## How tests are written

- Tests run against the real harness binaries. A fake driver is only a scripted one (a delay, a turn that ends
  without an outcome, a provider error, exit 75, a hang, a channel cut mid-turn, a permission request, an event
  outside a turn) or a replay of real transcripts.
- The [invariants](../design/invariants.md) are checked deterministically: a managed clock, a seed, a check after
  every step. Failure injection: `kill -9` of the daemon at every transaction boundary and between the write to the
  state and the call of the tracker; recovery must converge. Idempotency: any operation applied twice, any event
  delivered twice or not at all, gives the same final state.
- No locked seeds. A test passes on any seed; a random stand takes a new base seed on every run and prints it; a
  failed run is replayed by the printed seed. A range of seeds picked for a green run is forbidden. A failing seed is
  a bug of the code, not of the test.
- Wait for an event, not for time. A flaky test is a defect and is fixed by its cause, not by a longer timeout.
- `start_paused` is forbidden in a test that starts a real process (a service, an exec step, a harness, `sh`): while
  the runtime waits for the process the virtual clock jumps to the next timer. Such a test runs on real time with
  short intervals. Where no process runs, use the managed clock (`Clock::managed`, `start_paused`), not the wall
  clock.
- The repository's own `.beads` is not a fixture: tests of aae make their own temporary br directories.
