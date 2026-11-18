# Developing loop-manager

The repository pins its toolchain with [mise](https://mise.jdx.dev); `mise install` brings Rust, the cargo
tools, br (an exact version) and claude and codex (a release line: `2.1`, `0`; there is no `mise.lock`).
Every check below is a mise task (`mise.toml`).

mise is a convenience, not a requirement: install the same tools any way you like.

- Rust 1.100 with the components `clippy`, `rustfmt`, `rust-src`, `rust-analyzer`; for the release builds also the
  targets `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`.
- cargo-nextest 0.9.151, cargo-deny 0.20.2, cargo-machete 0.9.2 (`cargo install --locked <crate>@<version>`) and
  tokei 15.0.0 (`scripts/budget.sh`).
- zig 0.17.1 and cargo-zigbuild 0.24.1: for the static release build (`acceptance`, `build-all`,
  `acceptance-release`).
- br 0.7.6, claude (line `2.1`), codex (line `0`): for the acceptance scenarios that run them; jq 1.8.2 for the
  demos' `run.sh`, which `mise run acceptance` runs; jq and rumdl 0.3.4 also for the model-free scenario of demo 2's
  format step. Demo 3 also needs gh 2.103.2, logged in, from its own `mise.toml` (`mise install` in
  `demos/3-review-gate`).

Each task is a list of plain commands in `[tasks.<name>]` of `mise.toml`; run them by hand in that order:

- `test`: `cargo nextest run --locked -E 'not binary(chaos)'`. Without `-p` or `--workspace` cargo builds the
  `default-members` of the workspace: every crate but the acceptance runner `loop-manager-acceptance`.
- `ci`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -q -- -D warnings`,
  `cargo nextest run --workspace --locked --no-tests=warn -E 'not binary(chaos)'`, `cargo deny check`,
  `cargo machete`, then `scripts/unused-pub.sh`,
  `scripts/core-boundary.sh`, `scripts/select-biased.sh`, `scripts/budget-selftest.sh`, `scripts/budget.sh`, and a
  `git grep` that fails on an absolute home path (`/home/…`, `/Users/…`) outside `.loop/`.
- `acceptance-ci`: `cargo build -q --locked -p loop-manager`, then
  `cargo run -q --locked -p loop-manager-acceptance -- --ci`.
- `acceptance`: `cargo build -q --locked --release -p loop-manager`, then `cargo run -q --locked -p
  loop-manager-acceptance -- --loop-manager
  <repo>/target/release/loop-manager`: every scenario, the demos among them, runs the release build a user installs.
- `acceptance-release`: `cargo run -q --locked -p loop-manager-acceptance -- --release 1`.
- `build-all`: `cargo zigbuild --release --locked --target <target>` for each of the four targets.

## Where each kind of test runs

| Task | What it runs | When | Wall time |
|---|---|---|---|
| `mise run test` | unit and integration tests (`cargo nextest run`), without the chaos stand | the everyday loop, usually narrowed to a crate or a test | 25 s (2026-11-17, `57d8ff8`) |
| `mise run ci` | the above plus formatting, clippy, `cargo deny`, `cargo machete`, the unused-`pub` check, the line budgets, the core boundary (`scripts/core-boundary.sh`: no harness name, `br`, `max_sessions`, `instance_idle_s`, `beads_dir` or `.beads` in the code of the core, `loop-manager-contract` and `loop-manager-core`, and no `loop-manager-core` among the normal and build dependencies of `loop-manager-claude`, `loop-manager-codex`, `loop-manager-br` and `loop-manager-addons`), the fixed branch order of its `select!` (`scripts/select-biased.sh`: `biased;` with a comment on the order), no absolute home paths in the repository and no test that starts `loop-manager` past the common constructor, which keeps out the environment of a loop-manager session the tests may run in ([test rules](../agents/rules-tests.md)) | **the gate of a change**: locally before handing work over; CI on every commit | 33 s (2026-11-17, `57d8ff8`) |
| `mise run acceptance-ci` | the [acceptance scenarios](../acceptance.md) without a model, each on a real `loop-manager daemon`, and the chaos stand | on request, in the workflow `acceptance` (started by hand) | 144 s (2026-11-16, `04984d6`) |
| `mise run acceptance-release` | acceptance criterion 1 with `--release`: `cargo zigbuild --release` for the four targets, then this host's binary runs | the workflow `release` on a `v*` tag; no tag is made only to check it | 169 s cold (2026-11-14, `a070cd6`) |
| `mise run acceptance` | all acceptance scenarios, including those on a real model and the demos, whose result stays in the demo's directory, ignored by git | on request: they cost money and need a harness login | 2650 s (2026-11-16, `b32c55c`) |

<!-- MEASURE id=dev-test-wall date=2026-11-17 ver=57d8ff8 env=linux-arm64-orbstack-9-cores src="mise run ci" note="warm build; the nextest summary inside the task, 536 tests, without the chaos stand" -->
<!-- MEASURE id=dev-ci-wall date=2026-11-17 ver=57d8ff8 env=linux-arm64-orbstack-9-cores src="mise run ci" note="warm build, wall time of the whole task" -->
<!-- MEASURE id=dev-acceptance-ci-wall date=2026-11-16 ver=04984d6 env=linux-arm64-orbstack-9-cores src="mise run acceptance-ci" note="warm build, run right after mise run ci; the scenarios concurrently, then the cargo and script checks one by one; measured before the chaos stand joined it, which adds its own time" -->
<!-- MEASURE id=dev-acceptance-release-wall date=2026-11-14 ver=a070cd6 env=linux-arm64-orbstack-9-cores src="mise run acceptance-release" note="cold release build of the four targets" -->
<!-- MEASURE id=dev-acceptance-wall date=2026-11-16 ver=b32c55c env=linux-arm64-orbstack-9-cores src="mise run acceptance" note="warm release build; every row with real models; the two demos take most of it, demo 2 runs twice" -->

The times are for a warm build on 9 cores. A cold build comes on top, once.

CI has no access to a model by design. On a commit it runs `mise run ci` and nothing else; the model-free acceptance
scenarios run in the workflow `acceptance`, started by hand, and the runs on a real model and the demos only on a
developer's machine, both only when asked for ([acceptance](../acceptance.md)).
Dependencies must have MIT-compatible licenses (`cargo deny` checks them); no code of br or bv enters the tree.
The repository is public: no secrets and no absolute paths of a developer's machine enter it.

Run `mise run test` and `mise run acceptance-ci` one after the other, not side by side: the first builds
`target/debug/loop-manager` with the features of the test dependencies, the second without them, and each rewrites the
binary the other's daemons are running.

## The chaos stand

<!-- MEASURE id=dev-chaos-share date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run test" note="the per-test times of the eight chaos shares in the nextest output" -->
`crates/loop-manager-testkit/tests/chaos` checks the invariants of the core on random sequences of steps. It is
acceptance criterion 35, not a part of `mise run test` or `mise run ci`: it plays 1000 new seeds on every run, in eight
shares that run in parallel (14–17 s each on the machine above). Each share prints its random base seed; a failure names
its seed and the command that replays it:

```sh
LOOP_MANAGER_CHAOS_SEED=<n> cargo nextest run -p loop-manager-testkit --test chaos
```

A failing seed is a bug of the core, not of the test. `LOOP_MANAGER_CHAOS_SEEDS=<n>` plays another number of seeds: a
few dozen take seconds and make a targeted test while you change the core.

The rules of work on this repository (budgets, tests, what is decided) are in [docs/agents](../agents/README.md).
