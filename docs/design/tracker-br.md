# The br tracker

How aae uses [br](https://github.com/Dicklesworthstone/beads_rust) (beads_rust) as the tracker of a project. The
tracker interface and its errors are in [contracts.md](contracts.md) §5; the bead and run relation is below
([Beads and runs](#beads-and-runs)). Code: `crates/aae-br` (the adapter), `crates/aae-core/src/run/beads.rs` and
`run/bead_events.rs` (the core side), `crates/aae/src/daemon.rs` (the bv workspace file).

## An external binary, and its license

- br is a separate binary. It is pinned in the project's `mise.toml` (0.7.6) and installed by the user; aae runs it
  as a process and never links, vendors or ships it.
- Its license is MIT with a rider that grants no rights to OpenAI, Anthropic, their affiliates or anyone acting on
  their behalf. Linking or vendoring would carry the rider into the aae distribution, so aae does neither, and it
  warns wherever a user meets br: the README, the install guide (`docs/guide/install.md`) and `aae doctor`.
- Not assessed: whether the rider applies when Claude and Codex agents run `br` on a user's behalf.

## One database per project

- A project with a `<project>/.beads` directory has a br tracker: one database per project, outside the git of its
  rigs. The rig of a bead is the prefix of its id, so an epic spans rigs and `ready` is shared
  ([below](#the-rig-of-a-bead)).
- The database path is always explicit: every br call and every session or exec step gets `BEADS_DIR`. br's own
  discovery walks up to `$HOME`, and an agent without an explicit path silently writes to someone else's tracker.
- Every attempt has its own actor: the adapter passes `--actor` to br, sessions get `BD_ACTOR=aae:<run>:<n>`. br
  tells claim owners apart only by name, and the actor also carries the correlation of an agent's writes. Experiment 3
  confirmed both: `--actor X` and `BD_ACTOR=X` land in br's `events.actor`.
- Nothing of the orchestrator is written to the tracker: no runs, steps or sessions.

## Beads and runs

- A project works without a tracker too. A run gets its bead from a task input (`type = "task"`); runs without a
  bead are allowed.
- The daemon claims, releases and closes; an agent never claims. The run is recorded as the bead's owner in the
  transaction that creates it, and the tracker claim (owner `aae:<run>`) is made when it leaves `queued`. The claim is
  held for the whole run, through all its attempts, and let go only when the run is terminal: `passed` closes the
  bead, `failed` and `cancelled` release it conditionally (owner compared). Transitions:
  [contracts.md 6.2](contracts.md#62-transitions).
- A `formula` step lends the bead of its run to the child run: the child makes no claim and at its end neither closes
  nor releases the bead, the parent run does that at its own end.
- A bead has at most one live owner run; a second request for it is refused with the reason.
- A bead named by id that belongs to another rig is refused at the start; the claim checks it again
  (`failed{infrastructure, bead_of_another_rig}`, the claim released). A start with no rig takes the rig of
  the bead's id prefix, and works in the project root when the id matches no rig
  ([contracts.md 6.1](contracts.md#61-states)). A bead that still carries a `rig:` label is refused at the start,
  with the hint that the rig of a bead is the prefix of its id.
- Instead of an id the input may ask for the next ready bead, optionally in an epic: `{ next_ready = true, epic }`.
  The ready list comes from the adapter; the daemon picks and records the owner in one transaction, taking the first
  bead in the tracker's order that is in the epic, of the run's rig or of no rig, without an owner and without a
  closed gate. A bead whose labels the run cannot take (a `harness:` the config does not give its ai steps, an
  `agent:` that does not resolve) is passed over and the next one taken; the reply lists it with the reason in
  `passed_over`. A bead named by id with such labels is refused. With none ready no run is created: the caller gets
  the explicit answer `{"nothing_ready": true}`, a trigger `trigger.skipped{nothing_ready}` (its detail names the
  beads passed over).
- Work is cut into beads and the next one is taken by this ready pick, within an epic too. A chain of several
  formulas over one bead is not in the first release: `pass` always closes the bead.
- The failure counter of a bead lives in the state of the orchestrator: every failed run that held the claim adds
  one, `passed` resets it. At `bead_failures` (orchestrator config, default 3) the core closes the gate
  `bead:<project>/<id>` with a `gate.closed` event; the ready pick skips the bead until a human opens the gate, which
  resets the counter. Without it a released bead would raise `bead.ready` again and repeat forever.
- The ownership record in the state is the truth; br's assignee mirrors it. A failed `br close` or release does not
  change the outcome of the run: the operation is retried through the queue `tracker:<project>`, and a difference is
  shown by an event (`tracker.sync_failed`, `resync.diverged`). A `.beads` directory the daemon cannot watch at start
  gives `tracker.sync_failed{op: "watch"}` with no retry: that tracker is then read only by the periodic resync (every 5
  min).
- The agent uses `br` itself for the content of its work: child beads, comments, dependencies.
- The daemon creates one kind of bead: the root of a run started from the text of a task, an epic without labels whose
  `external_ref` is the key of the creation (`aae-` and 16 hex digits). The adapter first looks the key up in
  `br list -a`, so a creation repeated after a restart returns the same bead. The children of a claimed root are
  ready; a released root with open children is not.
- The ready beads under a root, at any depth (`under`, for `drain`), are one command:
  `br ready --unassigned --limit 0 --parent <root> --recursive`. br does not call an epic with open children ready,
  so an inner epic joins that list only once its own children are closed. A root br does not have is exit 3.
- A task input holds a string or `{ next_ready = true, epic }`. A string the core knows as a bead (in the last ready
  set, or one a run owned) is that bead at once. Any other string goes to the tracker when the run leaves `queued`,
  as an effect on the queue `tracker:<project>`: the adapter returns the bead whose id is exactly the string (looked
  up in the same `br list -a`, not by `br show`, which takes partial ids), else creates the root: title the first line
  of the text that is not blank (at most 80 characters), description the whole text. A bead filed a moment ago is
  thus taken up, not filed again; while it is `queued`, a gate on `bead:<project>/<the string>` holds it as if it
  held the bead. Edge: a text that is exactly `<prefix of a rig>-<word>` and names no rig runs on
  that rig; a bead that another run holds fails the run (`held by`), as the start could not see it. The key is drawn once and kept in the effect, so a repeat after `kill -9` creates nothing new; a repeated
  answer is dropped. The run then owns and claims the root as any bead, and its prompt sees the text as
  `{{ bead.description }}`. Known limit: a tracker that answers after `start_timeout` leaves a root without a run.
- Two labels of a bead shape every run on it. `agent:<name>` runs every ai step with that agent (a variant by its
  name, else the agent of that name) instead of the step's; the name has no pack prefix, as br refuses a dot in a label.
  `harness:<name>` is checked against the effective config of each ai step: a difference refuses the start with
  both harness names. Without `agent:` each step runs its own agent, without `harness:` nothing is checked; several of
  either label refuse the start.

## The rig of a bead

- The rig of a bead is its id up to the last `-`, matched against the prefixes of the project's rigs (`api-x1.2` is
  of the rig with prefix `api`). No match: a bead of the root, which runs in the root whatever the agent's scope. There
  is no `rig:` label.
- A rig's prefix is the acronym of the words of its name (`backend` is `b`, `my-project` is `mp`) or its `prefix`
  key ([config.md](config.md)). A session of a `rig`-scope agent on a rig and an `exec` step on a rig get
  `BD_ISSUE_PREFIX=<prefix>`, so `br create` there gives the rig's prefix. The daemon's own br calls drop it.
- An agent of scope `root` files a bead for a rig with
  `BD_ISSUE_PREFIX=<prefix> br create "<title>" --deps parent-child:<root bead> -l agent:<agent>,harness:<harness>`.
  Not `--parent`: br's `--parent <root>` gives the id `<root>.<n>` and loses the rig prefix. A dependency on another
  bead: `--deps "parent-child:<root>,blocks:<id>"`.
- `aae doctor` warns when a rig's prefix equals the br database's own prefix: the root's beads would read as the
  rig's. Changing a rig's `prefix` makes its existing beads beads of the root.

## Generated from the project configs

- `BEADS_DIR` in the environment of every session and every call, and, for a rig, `BD_ISSUE_PREFIX`.
- At daemon start, `.bv/workspace.yaml` next to the orchestrator config: one repository per project with a tracker,
  with the id prefix from `br config get issue_prefix` (a project whose prefix br does not give is listed without
  one, and bv assumes `<name>-`). The trackers are fixed for the daemon's life (a reload that changes them is
  refused), so the file is written at start only. Run `bv --workspace <file>` to see every project's beads.

## Limits of br the adapter is shaped by

- A rig name obeys the label rules: ASCII letters, digits, `-`, `_`.
- A bead's JSON has no epic field: the adapter computes the parent.
- br 0.7.6 has no conditional close and no "give the bead back" command: release and close check the owner first
  (`--if-unchanged` on release). br refuses to close a parent with open children (exit 3); the adapter releases it
  instead, so `passed` leaves such a bead open without an assignee.
- `br show` does not find a bead by `external_ref`: the key of a creation is looked up in the whole list.
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
  ready beads under a root, the changes of each command, the watcher, creation once per key, the close of a parent
  with an open child.
