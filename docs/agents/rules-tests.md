# Test rules

## The working devloop

The installed daemon, its socket, state, binary and tracker serve real runs. Only an explicit owner
request naming the operation on that daemon authorizes an update, a restart, a drain, a signal,
a config reload, a state change or a test there. A development epic authorizes the agent to report
on its assigned run and bead. Keep those operations separate from tests.

The devloop CI step runs through `devloop/scripts/test-env`, which removes outer run credentials and gives the
check private XDG directories and a private tracker path. For targeted checks in a working session use, for
example, `devloop/scripts/test-env cargo nextest run -p loop-manager -E 'test(drain)'`.
Exercise CLI, API and tracker behavior through the isolated test fixtures described below. Add a
fixture test if a changed user path lacks coverage. This also applies to reviewers and subagents.
A temporary working directory or a change to XDG variables alone still leaves the parent's
`LOOP_MANAGER_SOCKET` and `BEADS_DIR` in each fresh shell call.

## What to run, and when

| When | What | Command |
|---|---|---|
| while you work | the tests of what you touch, clippy of the crate | `cargo nextest run -p <crate> -E 'test(<name>)'`, `cargo clippy -p <crate> --all-targets -q -- -D warnings` |
| once, on the committed HEAD, before you hand the work over | **the gate** | `mise run ci` (the `tests` step runs it in devloop) |
| only when the owner asks | the model-free acceptance scenarios and the chaos stand, in CI | `gh workflow run acceptance`, then `gh run watch` |
| only when the owner asks | live models, the demos, on this machine | `mise run acceptance`, `demos/<demo>/run.sh` |

- `mise run ci` is the whole gate: formatting, clippy, the unit and integration tests without the chaos stand,
  `cargo deny`, `cargo machete` and the scripts of `scripts/`. A change is done when it is green on the HEAD you hand over. Do not run it after
  every edit: targeted tests are the everyday loop.
- [Acceptance scenarios](../acceptance.md) and the demos are never a condition of "done" and are never run on your
  own judgement, however large the change. They run when the owner's request, the epic or the owner's comment
  names them, and only the part it names. You may say in your report that a change deserves a scenario; you
  do not run it.
- `loop-manager-acceptance` is not a default member of the workspace: plain `cargo build`, `clippy` and `nextest` skip
  it, `mise run ci` checks it with `--workspace`. Check a change to it with `--workspace` too
  (`cargo clippy --workspace --all-targets -q -- -D warnings`), not `-p loop-manager-acceptance`: that package alone
  takes other features of `tokio`, `mio` and `uuid`, and cargo builds a second copy of those dependencies.
- The one exception is a scenario you edited yourself: run that criterion alone
  (`cargo build -q -p loop-manager && cargo run -q -p loop-manager-acceptance -- --ci <N>`), as the test of what you
  touched.
- Acceptance is allowed to be slow, which is why it is kept out of the way: its model-free rows run in CI only, in
  the workflow `acceptance` that starts by hand, never as a local habit. Do not move a slow check into the gate, and
  do not make a test of the gate slow to cover what an acceptance row covers.
- Fast tests come before new features: a slow or flaky suite is fixed before anything new is built on it. A test
  that grows the time of the gate without a reason is a defect.
- The chaos stand with its 1000 seeds is acceptance criterion 35 and is not in the gate. While you change the run
  machine of the core, a few dozen seeds are a targeted test that takes seconds:
  `LOOP_MANAGER_CHAOS_SEEDS=96 cargo nextest run -p loop-manager-testkit --test chaos`.
- Every command that may not return (a build, tests, a daemon, a harness, `loop-manager run wait`, the network, a wait
  for a file or a port) runs with an explicit limit: the tool's `timeout` or `timeout <sec> <command>`. A timeout that
  fired is a finding: write down what hung, do not repeat the command without a limit.
- No second `CARGO_TARGET_DIR` and no git worktree of this repository: the disk is nearly full. Do not run
  `mise run test` and an acceptance run side by side: each rewrites `target/debug/loop-manager` under the other's
  daemons.
- Command output is always cut: `cargo nextest run --no-fail-fast --status-level fail --final-status-level fail
  2>&1 | tail -80`.
- The dev profile carries no debug info: a panic names its file and line, a backtrace names functions without
  lines. When that is not enough to find one failure, prefix that one command with
  `CARGO_PROFILE_DEV_DEBUG=line-tables-only RUST_BACKTRACE=1`. It builds a second copy of the workspace crates
  (half a minute, 1.6 GB), so it is not a habit.

## Live models

- CI has no access to a model, by design. A run on a real `claude` or `codex` happens only on the owner's request,
  on the cheapest model (claude: haiku; codex: the cheapest available), for a specific check. Record the check, number of runs and result on the epic or in the report.
- The demos run on the models the owner named: demo 1 on `haiku` and `luna`, demo 2 on `sol`, `terra`, `luna`;
  demo 3 on `terra` and `luna`.
- The model `astra` is never run, anywhere.
- Burning quota on purpose for the sake of a check is forbidden.
- Do not write "codex is not logged in" as a fact without `codex login status` and one cheap run.

## The demos

- `demos/` is frozen: it changes only on the owner's request. A finished demo is not reworked, re-run or re-recorded
  because the code changed.
- If your change would stop a demo's config from loading, say so in your report; do not edit the
  demo.
- When the owner does ask: a demo's config is a sample a user writes their own from, so it holds nothing extra or
  unclear; everything a run creates is in `.gitignore`; the author's result lies beside it with the suffix
  `-example` and a `MEASURE` marker saying when and on what it was made (date, versions, models); a demo's
  dependencies are in its own `mise.toml` without a lock.

## How tests are written

- Tests run against the real harness binaries. A fake driver is only a scripted one (a delay, a turn that ends
  without an outcome, a provider error, exit 75, a hang, a channel cut mid-turn, an event outside a
  turn) or a replay of real transcripts.
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
- The repository's own `.beads` is not a fixture: tests of loop-manager make their own temporary br directories.
- Tests may run inside a session or an exec step of a loop-manager (as when loop-manager is developed with itself),
  which exports `LOOP_MANAGER_SOCKET`, `LOOP_MANAGER_RUN`, `LOOP_MANAGER_ATTEMPT_TOKEN`, `BEADS_DIR` and the like.
  Every run of loop-manager or br a test of the binary starts directly goes through `common::program` (the binary
  itself: `common::EXE`, `common::loop_manager`), and the acceptance runner through
  `loop_manager_testkit::without_outer_session`: they drop what the outer loop-manager exported, so the daemon under
  test does not listen on its socket and the CLI does not report to its run. A script a test starts that calls
  loop-manager itself (a demo's bridge) gets `LOOP_MANAGER_SOCKET` of the daemon under test from the test.
  `mise run ci` rejects `CARGO_BIN_EXE_loop-manager` outside `crates/loop-manager/tests/common`.
- The integration tests of the core and those of the binary are modules of one test binary each:
  `crates/loop-manager-testkit/tests/core` and `crates/loop-manager/tests/e2e`; the chaos stand is a binary of its own.
  A new test is a module there, not a new file under `tests/`: every binary is one more link on each change. A test that
  moves keeps its name in the nextest filters of `crates/loop-manager-acceptance/src/checks.rs` and `mise.toml`, or a
  filter silently selects nothing.
- On a fake provider, fail an attempt of claude with 529 (`Reply::Overloaded`): with `CLAUDE_CODE_MAX_RETRIES=0`
  that is exactly one request. On a 400 "prompt is too long" claude sends compaction requests of its own, which
  shifts a scripted plan of replies.
- claude hides MCP tools behind its tool search, so a fake provider sees `mcp__<server>__<tool>` only with
  `ENABLE_TOOL_SEARCH=false` in the agent's environment. codex puts the tools of an MCP server into a
  `{"type":"namespace","name":"mcp__<server>"}` entry, and the call must carry that `namespace`.
