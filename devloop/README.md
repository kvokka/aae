# devloop: loop-manager develops loop-manager

A loop-manager project whose rig is this repository. The owner files an epic as a bead; a run of the formula `epic`
has a coder work on it, runs the full test gate, has a reviewer review the result, and sends the work back to the
coder until the review scores 9 or more with the verdict `complete`. Nothing here ships with the binary: it is a pack
and a config, read by `loop-manager` as any other.

```text
devloop/
  config.toml           the orchestrator config of the loop's own daemon
  loop-manager.toml     the project: the addons, the settings of the sessions, the rig `repo` (`..`), the triggers
  agents/               coder and reviewer (steps of the formula); helper, subcoder, expert (subagents of the coder)
  formulas/epic.toml    code -> tests -> review -> gate -> report, with conditional owner waits
  scripts/              test, gate, owner-answer and report steps; self-tests in `mise run ci`
  results/<run>/        the completed run's report and comment snapshot, in git
  artifacts/<run>/      full logs and notes, local and ignored by git
  .beads/               the tracker of the epics (br), committed with the code; policy.yaml lists its statuses
```

## A run

- `code`: a new coder session works on the epic, commits as `type(scope): description [<epic id>]` and leaves a comment `CODER: done`,
  `partial` or `blocked` with `BASE:` and `HEAD:`. When only the owner can unblock it, the session finishes with
  `pass` and `data.needs_owner = true`: `code_owner` waits for the answer. A session stopped at its context limit
  still goes on to the next step.
- `ci_report`: runs `mise run ci` with a five-minute limit. The independent checks all run within that deadline;
  nextest runs the suite with fail-fast disabled. The run keeps the full log, per-check JSON report and digest
  under `target/devloop/runs/<run>/tests.*/`. The step returns the CI exit code, tested revision, dirty path
  count, failed checks and log path. A timeout retains the statuses of interrupted and pending checks.
- `ci_route`: green CI goes directly to review. Red CI returns the failed checks and log path to the coder up to
  three consecutive times. If CI is still red after those three fixes, the reviewer gets the report. A return from
  review or an owner answer starts a fresh sequence.
- `review`: the reviewer leaves a comment `REVIEW` with `SCORE: x/10`, `VERDICT:` and `HEAD:`, and gives the score
  and the verdict as the data of its `finish`. A review that fails or is stopped is written once more by a new
  session; a second failure fails the run. A blocking owner question takes the equivalent `review_owner` wait.
- `gate`: commits the round's tracker state locally. It accepts a review scored at least 9 with verdict `complete`
  and green CI on the current clean revision. Otherwise it marks the epic `rework` and returns it to the coder.
- `report`: without a model, `scripts/report.sh` extracts the coder's summary, the review, test digests and unresolved
  owner concerns, closes the epic and commits and pushes its closed tracker state with the report. Its TOON output
  is the run's result for a calling agent or external tool.

A run has 50 rounds (`rounds` in `formulas/epic.toml`). A CI return, a review under the bar or a return after an owner answer starts
the next round. All the working
context of an epic is in its comments: every round is a new session, which
reads the last 8 comments in its prompt and the rest with `br comments <id>`. Long notes of subagents go to
`target/devloop/runs/<run>/notes/`. The final step archives them with the full logs locally; the report and comment
snapshot survive in git. The reviewer judges repeated lack of progress from the reports and directs a different
approach or a consultation with the expert when needed.

One run at a time works in the repository (`max_writers = 1` of the rig): the others wait in `queued`. A session of
the coder or the reviewer is told to wrap up past 300000 tokens of context and stopped past 420000 (the addon
`context-budget`), and its state goes on the epic as a comment.

## The status of an epic

| Status | What it means | Who sets it |
|---|---|---|
| `open` | filed and waiting for a run, or the last run failed | you (`br create`); loop-manager when a run fails |
| `in_progress` | a run works on it and no review has sent it back yet | loop-manager, when a run claims it |
| `rework` | a review sent it back; the next rounds go on | `scripts/gate.sh` |
| `closed` | the review passed and the report step closes and publishes the epic | `scripts/report.sh`; loop-manager reconciles the close |

The agents change neither the status nor the assignee. br refuses a status not listed in `.beads/policy.yaml`;
`blocked` and `deferred` are there for you to use by hand. Two labels matter: `auto` makes a ready epic start a run,
and `needs-owner` marks an epic with an owner concern. A separate comment starting with `QUESTION: <id>` asks for a
blocking decision; `DECISION: <id>` records a debatable choice the agent made while continuing. `ANSWER: <id>`
resolves that concern. The following lines explain it in Russian. An ordinary finding is a `NOTE: …` comment.

The installed daemon, socket, state and tracker serve working epics. Agents use them to report on their assigned
work. Only an explicit owner request authorizes testing on, administering or upgrading this daemon. Coder and
reviewer checks use isolated tests, including tests of daemon startup, shutdown and tracker writes. The DCG addon
loads the [devloop pack](dcg.yaml) for these sessions through the project's `DCG_CUSTOM_PATHS` setting. It applies
to both Claude and Codex. The pack blocks daemon drain, restart, config reload, dev binary installation and
commands to stop, restart or disable the `loop-manager-dev` service.

## Install and start devloop

For an existing installation, check `devloop/lm status` and locate the service registration before starting or
recovering the daemon. A stopped service can still have a registration. Inspect the managers available on the host:

- On Linux, check both `systemctl --user list-unit-files --type=service` and `systemctl list-unit-files --type=service`.
- On macOS, inspect launchd with `launchctl print gui/$(id -u)` and `launchctl print system`, plus the installed
  LaunchAgents and LaunchDaemons plists.
- In Pitchfork, use `pitchfork daemons` for configured services and `pitchfork list` for their states. For a
  service registered by mise, inspect its `[daemons]` entry and use `mise daemons` with the owning config.
- Check any other supervisor configured on the host.

Identify devloop by its installed binary, arguments, project config, working directory and state path. Use the
existing manager and its registered service for an authorized start or recovery. Keep its config and binary
version. Use tmux only when the host has no daemon manager available for devloop. Check `devloop/lm status`
after startup.

For a fresh installation, work from the repository root. Initialize the tracker once with
`(cd devloop && br init --prefix dev)`. Use a checkout on `main` where `git push --dry-run` works without a prompt.
The coder and reviewer share that checkout.

```sh
mise run devloop-install
mise exec -- devloop/lm doctor --config devloop/config.toml
```

The install task copies the existing `debug/loop-manager` from Cargo's target directory to
`~/.local/opt/loop-manager-dev/bin/loop-manager`. If the artifact is missing or you want a newer build,
run `cargo build --locked -p loop-manager` first. For a new service, set the working directory to this
checkout and the command to `mise exec -- devloop/lm daemon --config devloop/config.toml`. Starting it under
mise supplies the project's tools and the `RUSTUP_TOOLCHAIN`, `RUSTUP_HOME` and `CARGO_HOME`
required by its sessions.

Use `devloop/lm` from the repository root for client commands as well. It selects the installed binary and puts
its directory first on the daemon's `PATH` for child commands. The dev database and socket are under
`~/.local/state/loop-manager-dev/loop-manager/`, separate from the ordinary installation.

For an authorized upgrade, install the new artifact between epics with `mise run devloop-install`, then restart the
registered service through its manager. Installation replaces the file atomically. Subsequent CLI invocations use
it immediately, and the daemon picks it up on restart.

## File an epic

A ready bead with the label `auto` starts a run; a bead without it starts nothing. It does not matter when the bead
got the label: filed with it or freed by its blockers with it, the bead starts a run as it becomes ready (the trigger
`epic-ready`); ready without it, it starts one as it gains it (`br label add <id> auto`, the trigger `epic-labeled`).
A label added to a bead that is blocked or held waits until the bead is ready. Other edits, and other labels, start
nothing, nor does a bead a run already holds.

```sh
cd devloop
br create "Short title of the epic" -t epic -l auto --description-file <file>   # or -d "<text>"
```

Write the description as a plan is written: the documents to read, the result, and a checkable "done when". A long
epic points to a PRD committed in the repository. An epic that waits for another is filed with a dependency
(`br dep add <epic> <what it waits for>`) and becomes ready when that one is closed.

By hand, without the label: `devloop/lm run start devloop repo epic --input epic=<bead id>`.

## Watch it

From the repository root, `mise install` installs the pinned viewer. Run `mise run bv` to open the devloop
tracker with a group for each status present, including `rework`. From this directory use `bv --recipe default`
for the same view. The built-in Kanban board has four fixed columns.

```sh
devloop/lm run list                 # the runs, newest first: state, step, formula
devloop/lm run show <run>           # one run: its round, the outcomes of its steps, why it failed
devloop/lm run watch <run>          # one run live, with the messages of its session
devloop/lm events                   # every event, live
devloop/lm trigger list             # `epic-ready`, `epic-labeled` and whether they are on
cd devloop
br show <id>                          # the epic, its status and labels
br comments <id>                      # every summary, review, question and note, oldest first
br list --status rework               # the epics a review sent back
br list -l needs-owner                # the epics with a question to you
```

Each test invocation keeps its own log: the step's `LOG:` line gives the path. The loop-manager commands above
emit structured replies in TOON. For scripts, `devloop/lm run show <run> --json` returns the run as JSON.

## Answer `needs-owner`

The label alone does not pause a run. A discretionary `DECISION` is a notification: work goes on, and closing the
epic does not mark the decision accepted. To acknowledge or change it, add a comment whose first line is
`ANSWER: <id>` and whose following lines contain your answer (`br comments add <bead> -f <file>`).

A blocking `QUESTION` ends the model session and enters `code_owner` or `review_owner`, an `await` step with a
seven-day timeout. Inspect it and answer through the existing external-decision API:

```sh
devloop/lm run list --awaiting
devloop/lm run show <run>
devloop/lm run resolve <run> code_owner pass --summary '<your answer>'
# For a question from the reviewer, use review_owner instead of code_owner.
```

The following exec step records `ANSWER: <id>` with the resolution's summary and optional data on the epic, then
returns to a new coder with the answer in its remarks and comments. A restart preserves the wait. Resolving with
`fail` ends the run; cancelling abandons it. A timeout fails the run rather than choosing an answer for you.

Do not remove `needs-owner` until all questions and decisions have answers: there may be several. The snapshot
below extracts the outstanding concerns without a model. An answer alone in a comment does not resolve an active
await step; use `run resolve` for that wait.

## The result for a caller

```sh
devloop/lm run wait <run>
devloop/lm run show <run>
```

The last step's `data.stdout` is a bounded TOON handoff: the epic, status, summary and review previews, outstanding
question and decision counts and previews, per-crate line changes when they fit, and the full report path.
A calling agent or tool should read that report to relay all outstanding concerns, even when the run passed.
`devloop/results/<run>/report.toon` holds this run's full summaries, review, test digests and changes to the code.
It also lists unresolved concerns across the epic. `comments.toon` holds the complete comment snapshot. Extraction
matches the explicit concern ids; it does not infer answers from prose, a removed label or a passing review.
Only a later matching `ANSWER` resolves a concern.

`module_lines` gives the net change in physical Rust lines for each changed crate under `crates/`, from the first
coder's `BASE` (`code_base`) to `code_head`. Positive numbers mean growth. The `tests` column covers files in
`tests/`, files named `tests.rs`, and the entire `loop-manager-testkit` and `loop-manager-acceptance` crates.
Other Rust files contribute to `code`. Both columns skip `generated/` directories. An empty list means the two
revisions have no Rust file changes in scope. A missing first-round base or unavailable commit range gives `null`.

Full test logs and run-scoped notes are copied to `devloop/artifacts/<run>/`. This local archive survives a
`cargo clean` but is ignored by git: a fresh clone has the report and comments, not the full logs and notes.

To inspect an epic before it finishes, including one waiting on you or whose run failed, from the repository root:

```sh
PATH="$HOME/.local/opt/loop-manager-dev/bin:$PATH" \
  BEADS_DIR="$PWD/devloop/.beads" sh devloop/scripts/report.sh snapshot <run> <bead>
```

This prints the full current handoff as TOON without closing the epic, committing or pushing.

## When a run fails

A run fails when its rounds run out, when an owner wait times out or is rejected, when a review cannot be written
twice, when finalization cannot complete (see "The push"), or when the infrastructure fails past its retries. The epic is
`open` again and, since one failed run is enough (`bead_failed_runs = 1` in `config.toml`), loop-manager closes the
gate `bead:devloop/<id>`: nothing more starts on that epic until you open it. Read the last comments, answer or
change the epic, then:

```sh
devloop/lm gate list
devloop/lm gate open bead:devloop/<id>
devloop/lm run list --state queued   # with `auto`, a run of the epic may wait here for the gate
```

If no run of the epic waits, start one with `run start` (see "File an epic").

## The push

The CI and finalization commands inherit the daemon's environment. Each round's commits stay local until
`report` closes the epic, flushes the tracker and commits the handoff. This final step pushes the entire epic to
the configured upstream in one update. The push authenticates through the daemon's SSH agent or Git credentials.
A failed push is retryable; repeating finalization reuses the local commit. If
finalization ultimately fails, the daemon releases the epic to `open`, as for another failed run; inspect its state
before recovery, since a local report is not proof of a successful push.

## The sessions

The coder and reviewer launch Claude in the repository checkout. Claude reads the [project settings](../.claude/settings.json)
there for both sessions. That file sets the subagent depth, enables the Rust Analyzer LSP and Ponytail plugins,
and selects the roles that Ponytail matches. The coder's native subagents share its session settings.

`[settings]` in `loop-manager.toml` supplies the three toolchain variables above and sets the Claude prompt cache
to 5 minutes for each session and its subagents. Every round starts a new session, and the long test run happens
outside it. Both roles attach `context-budget` and `dcg`, which refuses a destructive shell command
before it runs; what `dcg` needs on the host is in `addons/dcg/README.md`. `devloop/lm config explain --config
devloop/config.toml --project devloop --formula epic --rig repo` shows what each step gets and from which layer.

## The addons

The coder also attaches `conventional-commits`. Its Git commit-msg hook checks the subject and bead suffix before each commit in the working repository. See [the addon](../addons/conventional-commits/README.md) for requirements.

Both roles also attach [lessons](../addons/lessons/README.md). When an agent finishes, it can leave a short
lesson on its epic. An empty answer leaves no comment.

All four addons are declared by path (`path = "../addons/<name>"`), as no release tag holds the versions
this pack needs, and a declaration by git would make `config check` in `mise run ci` go to the network on a cold
cache. Once a release tag holds them, declare each as `git = "<the repository>"`, `rev = "<the tag>"` and
`dir = "addons/<name>"`.

## Changing this directory

Anyone may change it, the agents of an epic included. The daemon runs `scripts/` from the working tree at the moment
a step needs them, so an edit of `gate.sh` or `tests.sh` holds from the next step on. The config, the formula and
the prompts are held in memory: an edit of them holds only after `devloop/lm config reload`, and a running run
keeps the ones it started with. `mise run ci` runs `scripts/selftest.sh` and `config check` on this directory.
