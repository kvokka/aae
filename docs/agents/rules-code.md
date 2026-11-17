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
  (`scripts/core-boundary.sh`). Policy lives in plugins and config, not in the core
  ([architecture](../architecture.md#boundaries)).
- `tokio::select!` in the core (`aae-contract`, `aae-core`) is written only with `biased;` and a comment on why the
  branches stand in that order (`scripts/select-biased.sh`): a random choice between ready branches breaks the replay
  of a seed.

## Budgets

Lines of code without blanks and comments, counted by `tokei`; tests and generated code are left out by path.
`scripts/budget.sh` checks them in `mise run ci`.

| Part | Limit |
|---|---|
| the core (`aae-contract`, `aae-core`) | 18 000 |
| **the whole first release** | **30 000** |
| **hard stop** | **35 000** |
| a harness adapter, each | 3 500 |
| the tracker adapter | 1 200 |
| the three frontends together | 4 000 |
| the bundled plugins together | 1 500 |
| the rest: `doctor`, service files, pack loading, the MCP bridge | 4 300 |
| experiments (`experiments/`), outside the limits above | 5 000 |

- A limit is a ceiling, not a goal. The parts add up to more than the release limit: both hold.
- A file is at most 400 lines, tests included. Unit tests live in a separate `tests.rs`
  (`#[cfg(test)] mod tests;`), or the counter takes them for product code.
- Past the release limit nothing ships; past the hard stop no feature is added until code is removed.
- Stop and ask the owner when: the hard stop is passed; harness updates keep breaking the contract tests and eat more
  work than the product; a vendor charges for headless use in a way the owner will not accept; the vendors' own tools
  cover most of the formulas in real use.

## Measurements

Any measurement (time, memory, size, count, share), wherever it appears (docs, a report, a comment on a threshold in
code), carries a visible date, a visible version or short sha of what was measured, and an invisible comment beside
it, in one format for the whole project:

```
<!-- MEASURE id=<slug> date=<YYYY-MM-DD> ver=<version or sha> env=<OS and stand> src=<where from> note="<conditions; what to measure again>" -->
```

In code the same marker stands in the language's comment syntax. `scripts/budget.sh` checks that every marker has
`id`, `date`, `ver` and `env`; a measurement with no marker at all is for the review to catch.

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
