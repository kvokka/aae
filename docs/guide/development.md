# Developing aae

The repository pins its toolchain with [mise](https://mise.jdx.dev); `mise install` brings Rust, the cargo tools and
the pinned claude, codex and br. Every check below is a mise task (`mise.toml`).

## Where each kind of test runs

| Task | What it runs | Where | Wall time |
|---|---|---|---|
| `mise run test` | unit and integration tests (`cargo nextest run`), the chaos stand among them | locally, the everyday loop; CI too | 29 s (2026-11-14, `1a732a4`) |
| `mise run ci` | the above plus formatting, clippy, `cargo deny`, `cargo machete`, the unused-`pub` check and the line budgets | locally before a commit; CI on every commit | 34 s (2026-11-14, `1a732a4`) |
| `mise run acceptance-ci` | the acceptance scenarios without a model, each on a real `aae daemon` | CI on every commit (job `acceptance`); locally optional | 55 s (2026-11-14, `1a732a4`) |
| `mise run acceptance` | all acceptance scenarios, including those on a real model | by hand: they cost money and need a harness login | not measured |

<!-- MEASURE id=dev-test-wall date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run test" note="warm build; nextest summary 28.8 s for 286 tests; another build ran beside it" -->
<!-- MEASURE id=dev-ci-wall date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run ci" note="warm build, wall time of the whole task; nextest part 28.2 s" -->
<!-- MEASURE id=dev-acceptance-ci-wall date=2026-11-14 ver=1a732a4 env=linux-arm64-orbstack-9-cores src="mise run acceptance-ci" note="15 scenarios run concurrently; the longest, criterion 8, takes 55 s and criterion 7 watches an idle daemon for 30 s; 14 of 15 passed, criterion 10 failed fast as its workaround was being removed (WP-44a)" -->

The times are for a warm build on 9 cores. A cold build comes on top, once.

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
report, with the date and the versions of `aae` and the harnesses, goes to `target/acceptance/report.md`. What is
checked by hand is in [the manual acceptance checks](../acceptance-manual.md).
