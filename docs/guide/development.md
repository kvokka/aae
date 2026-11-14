# Developing aae

The repository pins its toolchain with [mise](https://mise.jdx.dev); `mise install` brings Rust, the cargo tools and
the pinned claude, codex and br. Every check below is a mise task (`mise.toml`).

## Where each kind of test runs

| Task | What it runs | Where | Wall time |
|---|---|---|---|
| `mise run test` | unit and integration tests (`cargo nextest run`), the chaos stand among them | locally, the everyday loop; CI too | 29 s (2026-11-14, `1a732a4`) |
| `mise run ci` | the above plus formatting, clippy, `cargo deny`, `cargo machete`, the unused-`pub` check and the line budgets | locally before a commit; CI on every commit | 34 s (2026-11-14, `1a732a4`) |
| `mise run acceptance-ci` | the acceptance scenarios without a model, each on a real `aae daemon`, then the checks of criteria 9, 11, 22 and 23 | CI on every commit (job `acceptance`); locally optional | 64 s warm with the release builds (2026-11-14, `a070cd6`+WP-52), less without them |
| `mise run acceptance-release` | criterion 1: `cargo zigbuild --release` for the four targets, then this host's binary runs | CI on a `v*` tag (workflow `release`) | 169 s cold (2026-11-14, `a070cd6`+WP-52) |
| `mise run acceptance` | all acceptance scenarios, including those on a real model | by hand: they cost money and need a harness login | not measured |

<!-- MEASURE id=dev-test-wall date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run test" note="warm build; nextest summary 28.8 s for 286 tests; another build ran beside it" -->
<!-- MEASURE id=dev-ci-wall date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run ci" note="warm build, wall time of the whole task; nextest part 28.2 s" -->
<!-- MEASURE id=dev-acceptance-ci-wall date=2026-11-14 ver=a070cd6+WP-52 env=linux-arm64-orbstack-9-cores src="mise run acceptance-ci" note="the 16 model-free scenarios concurrently, then the checks one by one; warm: release builds up to date; cold: the four release builds took 169 s of the 265 s; re-measure on the CI runner" -->

The times are for a warm build on 9 cores. A cold build comes on top, once.

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

A failing seed is a bug of the core, not of the test. The crash scenario of the acceptance spec (criterion 8) prints its pause seed the same way
(`AAE_CRASH_SEED`).

## Acceptance scenarios

Pass criterion numbers to run only those: `cargo build -p aae && cargo run -p aae-acceptance -- --ci 3 19`. The
report, with a line for every row of §15, goes to `target/acceptance/report.md`. After the scenarios the same binary
runs the rows that live in cargo and the scripts, one at a time: `cargo zigbuild --release` for the four targets
(criterion 1, about a minute per target warm), the crash tests of criterion 9, the contract tests of criterion 11,
`scripts/budget.sh` (22) and clippy, `cargo machete` and `scripts/unused-pub.sh` (23); they build in the target
directory the binary runs from. `--ci` leaves criterion 1 out (the row shows `not run`): the release builds are the
slowest part, so CI runs them only on a `v*` tag, as `mise run acceptance-release`.
