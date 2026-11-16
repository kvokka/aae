# Developing aae

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
  demos' `run.sh`, which `mise run acceptance` runs; jq and rumdl 0.3.4 also for the CI scenario of demo 2's format
  step.

Each task is a list of plain commands in `[tasks.<name>]` of `mise.toml`; run them by hand in that order:

- `test`: `cargo nextest run --locked`.
- `ci`: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -q -- -D warnings`,
  `cargo nextest run --locked --no-tests=warn`, `cargo deny check`, `cargo machete`, then `scripts/unused-pub.sh`,
  `scripts/core-boundary.sh`, `scripts/select-biased.sh`, `scripts/budget-selftest.sh`, `scripts/budget.sh`, and a
  `git grep` that fails on an absolute home path (`/home/…`, `/Users/…`) outside `.loop/` and `docs/init/`.
- `acceptance-ci`: `cargo build -q --locked -p aae`, then `cargo run -q --locked -p aae-acceptance -- --ci`.
- `acceptance`: the same without `--ci`.
- `acceptance-release`: `cargo run -q --locked -p aae-acceptance -- --release 1`.
- `build-all`: `cargo zigbuild --release --locked --target <target>` for each of the four targets.

## Where each kind of test runs

| Task | What it runs | Where | Wall time |
|---|---|---|---|
| `mise run test` | unit and integration tests (`cargo nextest run`), the chaos stand among them | locally, the everyday loop; CI too | 29 s (2026-11-14, `1a732a4`) |
| `mise run ci` | the above plus formatting, clippy, `cargo deny`, `cargo machete`, the unused-`pub` check, the line budgets and the core boundary (`scripts/core-boundary.sh`: no harness name, `br`, `max_sessions`, `instance_idle_s`, `beads_dir` or `.beads` in the code of `aae-core`) and the fixed branch order of its `select!` (`scripts/select-biased.sh`: `biased;` with a comment on the order), no absolute home paths in the repository | locally before a commit; CI on every commit | 62 s (2026-11-14, `75a5ee0`) |
| `mise run acceptance-ci` | the acceptance scenarios without a model, each on a real `aae daemon`, then the checks of criteria 9, 11, 17, 22 and 23 | CI on every commit (job `acceptance`); locally optional | 71 s warm (2026-11-14, `75a5ee0`) |
| `mise run acceptance-release` | criterion 1 with `--release`: `cargo zigbuild --release` for the four targets, then this host's binary runs | the workflow `release` on a `v*` tag; no tag is made only to check it | 169 s cold (2026-11-14, `a070cd6`+WP-52) |
| `mise run acceptance` | all acceptance scenarios, including those on a real model; the rows of criterion 20 run `demos/<demo>/run.sh`, whose result stays in the demo's directory, ignored by git | by hand: they cost money and need a harness login | not measured |

<!-- MEASURE id=dev-test-wall date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run test" note="warm build; nextest summary 28.8 s for 286 tests; another build ran beside it" -->
<!-- MEASURE id=dev-ci-wall date=2026-11-14 ver=75a5ee0 env=linux-arm64-orbstack-9-cores src="mise run ci" note="warm build, wall time of the whole task" -->
<!-- MEASURE id=dev-acceptance-ci-wall date=2026-11-14 ver=75a5ee0 env=linux-arm64-orbstack-9-cores src="mise run acceptance-ci" note="warm build, wall time of the whole task" -->
<!-- MEASURE id=dev-acceptance-release-wall date=2026-11-14 ver=a070cd6 env=linux-arm64-orbstack-9-cores src="mise run acceptance-release" note="cold release build of the four targets" -->
<!-- MEASURE id=dev-ci-wall date=2026-11-14 ver=75a5ee0 env=linux-arm64-orbstack-9-cores src="mise run ci" note="warm build, wall time of the whole task; nextest summary 36.9 s for 331 tests" -->
<!-- MEASURE id=dev-acceptance-ci-wall date=2026-11-14 ver=75a5ee0 env=linux-arm64-orbstack-9-cores src="mise run acceptance-ci" note="warm build, run right after mise run ci; the model-free scenarios concurrently, then the cargo and script checks one by one; no release builds; re-measure on the CI runner" -->

The times are for a warm build on 9 cores. A cold build comes on top, once.

CI has no access to a model by design: it builds, tests and checks with scripted and fake providers only. The
scenarios on a real model and the demos run only on a developer's machine, on the cheapest model. Acceptance runs on
Linux arm64; the other three targets are built (criterion 1) but not exercised. Dependencies must have MIT-compatible
licenses (`cargo deny` checks them); no code of br or bv enters the tree.
The repository is public: no secrets and no absolute paths of a developer's machine enter it.

Run `mise run test` and `mise run acceptance-ci` one after the other, not side by side: the first builds
`target/debug/aae` with the features of the test dependencies, the second without them, and each rewrites the binary
the other's daemons are running.

## The chaos stand

<!-- MEASURE id=dev-chaos-share date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run test" note="the per-test times of the eight chaos shares in the nextest output" -->
`crates/aae-testkit/tests/chaos` plays 1000 new seeds on every run, in eight shares that run in parallel (14–17 s each
on the machine above). Each share prints its random base seed; a failure names its seed and the command that replays
it:

```sh
AAE_CHAOS_SEED=<n> cargo nextest run -p aae-testkit --test chaos
```

A failing seed is a bug of the core, not of the test. The crash scenario of the acceptance spec (criterion 8) prints its
pause seed the same way (`AAE_CRASH_SEED`).

## Acceptance scenarios

The product has no separate test mode: checking formulas and checking aae itself is one executable acceptance spec,
`crates/aae-acceptance`, which implements the criteria of the first release. Its scenarios go the way working runs
go, on a real daemon and the real harness binaries. The scenarios on a real model use a cheap one, set by the agents
of their own fixtures (haiku for claude), and run only when a user asks for the full spec with one command
(`mise run acceptance`): it costs money and needs a login, so nothing starts them automatically, on a commit or on a
schedule. The scenarios without a model (a fake provider, the scripted driver, exec steps) are the same scenarios of
the same spec, and CI runs this subset on every commit; the launch column of each criterion in spec §15 says which
part it belongs to. A run of the spec writes a report with the date, the version of aae and the versions of the
harnesses.

Pass criterion numbers to run only those: `cargo build -p aae && cargo run -p aae-acceptance -- --ci 3 19`. The
report, with a line for every row of §15, goes to `target/acceptance/report.md`. After the scenarios the same binary
runs the rows that live in cargo and the scripts, one at a time: `cargo zigbuild --release` for this host's target
(criterion 1), the crash tests of criterion 9, the contract tests of criterion 11,
`scripts/budget.sh` (22) and clippy, `cargo machete` and `scripts/unused-pub.sh` (23); they build in the target
directory the binary runs from. `--ci` leaves criterion 1 out (the row shows `not run`): the release build is the
slowest part, so the full local run (`mise run acceptance`) checks it. A developer's machine builds its own target
only: the row of the four release targets runs only with `--release` (`mise run acceptance-release`), which the
workflow `release` runs on a `v*` tag; without it the row shows `on a tag` and counts for no score. A part of a
criterion that no row checks yet shows as an `open` row (`crates/aae-acceptance/src/readiness.rs`, `OPEN`): it does
not fail the run, but it keeps its criterion open, so criterion 22 caps the readiness scores of the components that
list it.
