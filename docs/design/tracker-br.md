# The br tracker

How aae uses [br](https://github.com/Dicklesworthstone/beads_rust) (beads_rust) as the tracker of a project. The
tracker interface and its errors are in [contracts.md](contracts.md) §5; the bead and run relation is in the
spec §10.3. Code: `crates/aae-br` (the adapter), `crates/aae-core/src/run/beads.rs` and `run/bead_events.rs` (the
core side), `crates/aae/src/daemon.rs` (the bv workspace file).

## An external binary, and its license

- br is a separate binary. It is pinned in the project's `mise.toml` (0.7.6) and installed by the user; aae runs it
  as a process and never links, vendors or ships it.
- Its license is MIT with a rider that grants no rights to OpenAI, Anthropic, their affiliates or anyone acting on
  their behalf. Linking or vendoring would carry the rider into the aae distribution, so aae does neither, and it
  warns wherever a user meets br: the README, the install guide (`docs/guide/install.md`) and `aae doctor`.
- Not assessed: whether the rider applies when Claude and Codex agents run `br` on a user's behalf.

## One database per project

- A project with a `<project>/.beads` directory has a br tracker: one database per project, outside the git of its
  rigs. A rig is the label `rig:<name>`, so an epic spans rigs and `ready` is shared; per-rig id prefixes are lost.
- The database path is always explicit: every br call and every session or exec step gets `BEADS_DIR`. br's own
  discovery walks up to `$HOME`, and an agent without an explicit path silently writes to someone else's tracker.
- Every attempt has its own actor: the adapter passes `--actor` to br, sessions get `BD_ACTOR=aae:<run>:<n>`. br
  tells claim owners apart only by name, and the actor also carries the correlation of an agent's writes. Only the
  `--actor` flag was confirmed by experiment; the environment variable is passed as well.
- Nothing of the orchestrator is written to the tracker: no runs, steps or sessions.

## Generated from the project configs

- `BEADS_DIR` in the environment of every session and every call, and the rig labels.
- At daemon start, `.bv/workspace.yaml` next to the orchestrator config: one repository per project with a tracker,
  with the id prefix from `br config get issue_prefix` (a project whose prefix br does not give is listed without
  one, and bv assumes `<name>-`). The trackers are fixed for the daemon's life (a reload that changes them is
  refused), so the file is written at start only. Run `bv --workspace <file>` to see every project's beads.

## Limits of br the adapter is shaped by

- A rig name obeys the label rules: ASCII letters, digits, `-`, `_`.
- A bead's JSON has no epic field: the adapter computes the parent.
- br 0.7.6 has no conditional close and no "give the bead back" command: release and close check the owner first
  (`--if-unchanged` on release).
- The exit code of a busy claim differs between versions: the adapter branches on the exit code (4), not on text.
- br has no events. Changes made through aae raise events at once; changes made by hand are caught by a watcher on
  `.beads/` and read from br's `events` table read-only, under br's `.write.lock` held shared (a stock SQLite
  reader otherwise resets the WAL index under a br write). Reading that table depends on br's schema.
- Every br command costs time linear in the size of the database: `close` takes 81 ms on 100 beads, 311 ms on
  5 thousand, 3.2–4.4 s on 50 thousand (br 0.7.6).
  <!-- MEASURE id=br-close-latency date=2026-11-10 ver=br-0.7.6 env=linux-arm64-orbstack,9vcpu,12gb src=reports/11 note="medians, synthetic database: 2 % epics, 60 % closed; measure again on every br update" -->

## Versions

- Development and tests use the br pinned in `mise.toml`. The adapter declares the versions it was tested on
  (`aae_br::TESTED`); the daemon at start and `aae doctor` warn about a version outside them.
- br has had regressions between patch versions, so the adapter has its own contract tests on the pinned br
  (`crates/aae-br/tests/contract.rs`): claim under concurrency, conditional release, close, ready in an epic, the
  changes of each command, the watcher.
