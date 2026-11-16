# Test rules

## What to run, and when

| When | What | Command |
|---|---|---|
| while you work | the tests of what you touch, clippy of the crate | `cargo nextest run -p <crate> -E 'test(<name>)'`, `cargo clippy -p <crate> --all-targets -q -- -D warnings` |
| once, on the committed HEAD, before you hand the work over | **the gate** | `mise run ci` (in the loop: `scripts/loop/checks NN`) |
| only when the owner asks | the model-free acceptance scenarios and the chaos stand, in CI | `gh workflow run acceptance`, then `gh run watch` |
| only when the owner asks | live models, the demos, release builds, on this machine | `mise run acceptance`, `demos/<demo>/run.sh`, `mise run acceptance-release` |

- `mise run ci` is the whole gate: formatting, clippy, the unit and integration tests without the chaos stand,
  `cargo deny`, `cargo machete` and the scripts of `scripts/`. A change is done when it is green on the HEAD you hand over. Do not run it after
  every edit: targeted tests are the everyday loop.
- [Acceptance scenarios](../acceptance.md) and the demos are never a condition of "done" and are never run on your
  own judgement, however large the change. They run when the owner's request, the plan package or a journal entry of
  the owner names them, and only the part it names. You may say in your report that a change deserves a scenario; you
  do not run it.
- `aae-acceptance` is not a default member of the workspace: plain `cargo build`, `clippy` and `nextest` skip it,
  `mise run ci` checks it with `--workspace`. Check a change to it with `--workspace` too
  (`cargo clippy --workspace --all-targets -q -- -D warnings`), not `-p aae-acceptance`: that package alone takes
  other features of `tokio`, `mio` and `uuid`, and cargo builds a second copy of those dependencies.
- The one exception is a scenario you edited yourself: run that criterion alone
  (`cargo build -q -p aae && cargo run -q -p aae-acceptance -- --ci <N>`), as the test of what you touched.
- Acceptance is allowed to be slow, which is why it is kept out of the way: its model-free rows run in CI only, in
  the workflow `acceptance` that starts by hand, never as a local habit. Do not move a slow check into the gate, and
  do not make a test of the gate slow to cover what an acceptance row covers.
- Fast tests come before new features: a slow or flaky suite is fixed before anything new is built on it. A test
  that grows the time of the gate without a reason is a defect.
- The chaos stand with its 1000 seeds is acceptance criterion 35 and is not in the gate. While you change the run
  machine of the core, a few dozen seeds are a targeted test that takes seconds:
  `AAE_CHAOS_SEEDS=96 cargo nextest run -p aae-testkit --test chaos`.
- Every command that may not return (a build, tests, a daemon, a harness, `aae run wait`, the network, a wait for a
  file or a port) runs with an explicit limit: the tool's `timeout` or `timeout <sec> <command>`. A timeout that
  fired is a finding: write down what hung, do not repeat the command without a limit.
- No second `CARGO_TARGET_DIR` and no git worktree of this repository: the disk is nearly full. Do not run
  `mise run test` and an acceptance run side by side: each rewrites `target/debug/aae` under the other's daemons.
- Command output is always cut: `cargo nextest run --no-fail-fast --status-level fail --final-status-level fail
  2>&1 | tail -80`.
- The dev profile carries no debug info: a panic names its file and line, a backtrace names functions without
  lines. When that is not enough to find one failure, prefix that one command with
  `CARGO_PROFILE_DEV_DEBUG=line-tables-only RUST_BACKTRACE=1`. It builds a second copy of the workspace crates
  (half a minute, 1.6 GB), so it is not a habit.

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
- The integration tests of the core and those of the binary are modules of one test binary each:
  `crates/aae-testkit/tests/core` and `crates/aae/tests/e2e`; the chaos stand is a binary of its own. A new test is
  a module there, not a new file under `tests/`: every binary is one more link on each change. A test that moves
  keeps its name in the nextest filters of `crates/aae-acceptance/src/checks.rs` and `mise.toml`, or a filter
  silently selects nothing.
- On a fake provider, fail an attempt of claude with 529 (`Reply::Overloaded`): with `CLAUDE_CODE_MAX_RETRIES=0`
  that is exactly one request. On a 400 "prompt is too long" claude sends compaction requests of its own, which
  shifts a scripted plan of replies.
- claude hides MCP tools behind its tool search, so a fake provider sees `mcp__<server>__<tool>` only with
  `ENABLE_TOOL_SEARCH=false` in the agent's environment. codex puts the tools of an MCP server into a
  `{"type":"namespace","name":"mcp__<server>"}` entry, and the call must carry that `namespace`.
