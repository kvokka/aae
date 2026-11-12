# aae

## The br tracker and its license

A project with a `<project>/.beads` directory uses [br](https://github.com/Dicklesworthstone/beads_rust) as its
tracker. br is a separate binary you install yourself (this repository pins 0.7.6 in `mise.toml`); aae runs it as a
process and never links, vendors or ships it.

**br is not plain MIT.** Its license is MIT with a rider that grants no rights to OpenAI, Anthropic, their affiliates
or anyone acting on their behalf. Check whether that rider applies to you before you install br. Whether it applies to
Claude or Codex agents running `br` inside an aae session has not been assessed.

## Acceptance spec

The acceptance criteria of the first release are executable scenarios in `crates/aae-acceptance`, each on a real
`aae daemon` in a temporary directory tree. `mise run acceptance-ci` runs the scenarios without a model, as CI does on
every commit (about 11 minutes: one of them watches an idle daemon for 10). `mise run acceptance` runs all of them,
including those that call a real model: they use the cheapest model, cost money and need your harness login. Pass
criterion numbers to run only those: `cargo build -p aae && cargo run -p aae-acceptance -- --ci 3 19`. The report,
with the date and the versions of `aae` and the harnesses, goes to `target/acceptance/report.md`.
