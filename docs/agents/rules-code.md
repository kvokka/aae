# Code rules

## Less code

- Write only what the asked result needs. No stubs and no code "for later": no empty implementations, `todo!` or
  `unimplemented!`, no unused parameters, fields, flags or extension points for future features, no generalization
  over a single case. What [the first release leaves out](../overview.md#what-the-first-release-leaves-out) is
  absent from the code altogether.
- No mechanism without a use: every function serves real work that exists today.
- Before a new mechanism, check whether the harness vendor already has it.
- The first release is not out, so there is no compatibility with an old config: a retired key is a config error in
  one line that says where it is written now. No transition layers.
- The core knows no harness, no role and no br: those come through the driver and tracker interfaces
  (`scripts/core-boundary.sh`). Policy lives in addons and config, not in the core
  ([architecture](../architecture.md#boundaries)).
- `tokio::select!` in the core (`loop-manager-contract`, `loop-manager-core`) is written only with `biased;` and a
  comment on why the branches stand in that order (`scripts/select-biased.sh`): a random choice between ready branches
  breaks the replay of a seed.
- Follow Clippy's default thresholds for the workspace lints in `Cargo.toml`. CI checks every crate and target.
- Keep unit tests in a separate `tests.rs` or `tests/` module.
- Ask the owner when harness updates keep breaking the contract tests and eat more work than the product, a vendor
  charges for headless use in a way the owner will not accept, or the vendors' own tools cover most of the formulas
  in real use.

## Measurements

Any measurement (time, memory, size, count, share), wherever it appears (docs, a report, a comment on a threshold in
code), carries a visible date, a visible version or short sha of what was measured, and an invisible comment beside
it, in one format for the whole project:

```
<!-- MEASURE id=<slug> date=<YYYY-MM-DD> ver=<version or sha> env=<OS and stand> src=<where from> note="<conditions; what to measure again>" -->
```

In code the same marker stands in the language's comment syntax. `scripts/measure.sh` checks that every marker of the
repository (outside `.loop/` and `.claude/`) has `id`, `date`, `ver` and `env`; what the agents of a demo wrote
(`demos/*/*-example/`) is no measurement of this repository and is not checked. A measurement with no marker at all
is for the review to catch.

## Dependencies and tools

- Everything is at its latest release when added, and pinned: a crate comes by `cargo add <crate>` without a
  version, `Cargo.lock` is in git; tools have exact versions in `mise.toml` (`mise use <tool>@<x.y.z>`), claude and
  codex a release line (`2.1`, `0`); there is no `mise.lock`. No update without a reason; an update is a commit of
  its own. A harness or br update is accepted by the contract tests ([harness updates](../harness-updates.md)).
- Dependencies have MIT-compatible licenses (`cargo deny`). No code of br or bv enters the tree.
- mise is a convenience, not a requirement. A command that is not found: `mise exec -- <command>`.
- Structural search in Rust is `ast-grep`: `ast-grep -l rust -p '$X.unwrap()' crates/`.

## Repository

- The repository is public, MIT: no secrets, no absolute paths of a developer's machine (`mise run ci` greps for
  them), no research reports. No other project of the owner, the predecessor among them, is named in code, docs or
  the loop's files.
- Work goes in `main`. Commits are atomic, messages in English. No force-push, no rewriting of history, no
  `--no-verify`. Temporary logs of the work are not committed.
- Never `rm -r` or `rm -rf`, in a command or in a script you write: tracked files go by `git rm -r -q <path>`,
  untracked ones by `git clean -fdq -- <path>` or `find <path> -delete`, temporary ones live in `mktemp -d` and stay.
- Each of `experiments/*`, `demos/*` has a short README: what is there and why.
- An experiment's result goes to [experiments](../design/experiments.md) with `MEASURE` markers, and the code of a
  recorded experiment is deleted. A cheap experiment comes before the code that leans on it.
