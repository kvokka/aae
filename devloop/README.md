# devloop: loop-manager develops loop-manager

A loop-manager project whose rig is this repository. The owner files an epic as a bead; a run of the formula `epic`
has a coder work on it, runs the full test gate, has a reviewer review the result, and sends the work back to the
coder until the review scores 9 or more with the verdict `complete`. Nothing here ships with the binary: it is a pack
and a config, read by `loop-manager` as any other.

```text
devloop/
  config.toml           the orchestrator config of the loop's own daemon
  loop-manager.toml     the project: the addons, the settings of the sessions, the rig `repo` (`..`), the trigger
  agents/               coder and reviewer (steps of the formula); helper, subcoder, expert (subagents of the coder)
  formulas/epic.toml    code -> tests -> review -> gate -> report, with conditional owner waits
  scripts/              test, gate, owner-answer and report steps; self-tests in `mise run ci`
  results/<run>/        the completed run's report and comment snapshot, in git
  artifacts/<run>/      full logs and notes, local and ignored by git
  .beads/               the tracker of the epics (br), committed with the code; policy.yaml lists its statuses
```

## A run

- `code`: a new coder session works on the epic, commits as `<epic id>: …` and leaves a comment `CODER: done`,
  `partial` or `blocked` with `BASE:` and `HEAD:`. When only the owner can unblock it, the session finishes with
  `pass` and `data.needs_owner = true`: `code_owner` waits for the answer. A session stopped at its context limit
  still goes on to the next step.
- `tests`: `scripts/tests.sh` runs `mise run ci` on the tree the coder left, writes the whole log to
  a new directory under `target/devloop/runs/<run>/tests.*/` and prints its `LOG:` path and a digest ending in
  `RESULT: pass` or `RESULT: FAIL`. Red tests still pass this step:
  tests go to the reviewer, not back to the coder.
- `review`: the reviewer leaves a comment `REVIEW` with `SCORE: x/10`, `VERDICT:` and `HEAD:`, and gives the score
  and the verdict as the data of its `finish`. A review that fails or is stopped is written once more by a new
  session; a second failure fails the run. A blocking owner question takes the equivalent `review_owner` wait.
- `gate`: `scripts/gate.sh` commits `devloop/.beads/` and pushes, then passes on a score of 9 or more with the
  verdict `complete`. Otherwise the epic gets the status `rework` and the run goes back to `code` with what the gate
  printed: the score, the verdict and the `RESULT:` of the tests.
- `report`: without a model, `scripts/report.sh` extracts the coder's summary, the review, test digests and unresolved
  owner concerns, closes the epic and commits and pushes its closed tracker state with the report. Its TOON output
  is the run's result for a calling agent or external tool.

A run has 50 rounds (`rounds` in `formulas/epic.toml`). A review under the bar or a return after an owner answer starts
the next round. The reviewer requires green tests for `complete`; the gate trusts its verdict. All the working
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

## Before the first start

- Use a binary built from this checkout, with idempotent tracker closure, for both the daemon and the commands its
  scripts call. `cargo build --locked -p loop-manager`, then `export PATH="$PWD/target/debug:$PATH"` from the
  repository root makes that binary available to the daemon and its children.
- `loop-manager`, `claude`, `br`, `dcg`, `jq`, `toon` 0.2.5, `git`, `mise` and `cargo-nextest` are on the shell's `PATH` when it
  starts the daemon, and `loop-manager doctor --config devloop/config.toml` is content.
- That shell is one where mise has activated this repository: the sessions take `RUSTUP_TOOLCHAIN`, `RUSTUP_HOME`
  and `CARGO_HOME` from it, and without them every session fails with `config_error` naming the variable.
- The branch is `main`, and `git push --dry-run` works from that shell without a question: see "The push".
- The loop this pack replaces is not running: the two cannot work in one checkout at once (see "Moving over").

## Start it

```sh
loop-manager daemon --config devloop/config.toml
```

The daemon keeps its state in the XDG state directory and listens on its default socket. To keep it apart from
another daemon of yours, give it its own `XDG_STATE_HOME` and `LOOP_MANAGER_SOCKET`, as `demos/*/run.sh` do, and
export the same `LOOP_MANAGER_SOCKET` in the shell you run the commands below from.

## File an epic

A ready bead with the label `auto` starts a run (the trigger `epic-ready`); a bead without it starts nothing.

```sh
cd devloop
br create "Short title of the epic" -t epic -l auto --description-file <file>   # or -d "<text>"
```

Write the description as a plan is written: the documents to read, the result, and a checkable "done when". A long
epic points to a PRD committed in the repository. An epic that waits for another is filed with a dependency
(`br dep add <epic> <what it waits for>`) and becomes ready when that one is closed.

By hand, without the label: `loop-manager run start devloop repo epic --input epic=<bead id>`.

## Watch it

```sh
loop-manager run list                 # the runs, newest first: state, step, formula
loop-manager run show <run>           # one run: its round, the outcomes of its steps, why it failed
loop-manager run watch <run>          # one run live, with the messages of its session
loop-manager events                   # every event, live
loop-manager trigger list             # `epic-ready` and whether it is on
cd devloop
br show <id>                          # the epic, its status and labels
br comments <id>                      # every summary, review, question and note, oldest first
br list --status rework               # the epics a review sent back
br list -l needs-owner                # the epics with a question to you
```

Each test invocation keeps its own log: the step's `LOG:` line gives the path. When reading structured command
output into an agent's context, use `loop-manager run show <run> --json | toon --encode` (bash or zsh with
`set -o pipefail`). Keep JSON for programs that parse it. `toon` is pinned as
`"github:Dicklesworthstone/toon_rust" = "0.2.5"` in `mise.toml`.

## Answer `needs-owner`

The label alone does not pause a run. A discretionary `DECISION` is a notification: work goes on, and closing the
epic does not mark the decision accepted. To acknowledge or change it, add a comment whose first line is
`ANSWER: <id>` and whose following lines contain your answer (`br comments add <bead> -f <file>`).

A blocking `QUESTION` ends the model session and enters `code_owner` or `review_owner`, an `await` step with a
seven-day timeout. Inspect it and answer through the existing external-decision API:

```sh
loop-manager run list --awaiting
loop-manager run show <run>
loop-manager run resolve <run> code_owner pass --summary '<your answer>'
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
loop-manager run wait <run>
loop-manager run show <run>
```

The last step's `data.stdout` is a bounded TOON handoff: the epic, status, summary and review previews, outstanding
question and decision counts and previews, and the full report path. A calling agent or tool should read that
report to relay all outstanding concerns, even when the run passed. The files in git are
`devloop/results/<run>/report.toon` (full summaries, review, test digests, unresolved concerns and code revision) and
`comments.toon` (the complete comment snapshot). Extraction matches the explicit concern ids; it does not infer
answers from prose, a removed label or a passing review. Only a later matching `ANSWER` resolves a concern.

Full test logs and run-scoped notes are copied to `devloop/artifacts/<run>/`. This local archive survives a
`cargo clean` but is ignored by git: a fresh clone has the report and comments, not the full logs and notes.

To inspect an epic before it finishes, including one waiting on you or whose run failed, from the repository root:

```sh
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
loop-manager gate list
loop-manager gate open bead:devloop/<id>
loop-manager run list --state queued   # with `auto`, a run of the epic may wait here for the gate
```

If no run of the epic waits, start one with `run start` (see "File an epic").

## The push

The steps `tests`, `gate` and `report` get the whole environment of the daemon (`inherit_env = true` in
`formulas/epic.toml`), not `PATH`, `HOME` and `LANG` alone, so `mise run ci` sees what your shell sees and the
`git push` of the gate authenticates as your shell does: through the ssh agent (`SSH_AUTH_SOCK`), a token
(`GH_TOKEN`, `GITHUB_TOKEN`) or a credential helper. A key with a passphrase and no agent cannot work: nothing can
ask for it, and the gate fails instead of waiting. The gate commits `devloop/.beads/` alone and pushes after every
review, whatever it says; what the coder left uncommitted stays as it was. When the review passes and the push does
not, the step is tried again three times, 30 s apart, and then the run fails; the commit stays local and the next
push takes it along. After the gate passes, `report` closes the epic while preserving its assignee, flushes the
tracker and commits and pushes the closed state with the handoff. A failed final push is retryable too. Repeating
the same finalization does not add another commit; the adapter's later close by the same owner is a no-op. If
finalization ultimately fails, the daemon releases the epic to `open`, as for another failed run; inspect its state
before recovery, since a local report is not proof of a successful push.

## The sessions

`[settings]` in `loop-manager.toml` holds for every agent of the pack and its subagents: the three toolchain
variables above, `BR_OUTPUT_FORMAT = "toon"` (br answers in TOON; `--json` still gives JSON), a subagent depth of 2,
and a prompt cache of 5 minutes instead of the hour a subscription gets by default (`native.claude.promptCacheTtl`
and `subagentPromptCacheTtl`): every round is a new session and the long test run is a step outside any session. The
pack runs on claude only. Both roles attach `context-budget` and `dcg`, which refuses a destructive shell command
before it runs; what `dcg` needs on the host is in `addons/dcg/README.md`. `loop-manager config explain --config
devloop/config.toml --project devloop --formula epic --rig repo` shows what each step gets and from which layer.

## The addons

`context-budget` and `dcg` are declared by path (`path = "../addons/<name>"`), as no release tag holds the versions
this pack needs, and a declaration by git would make `config check` in `mise run ci` go to the network on a cold
cache. Once a release tag holds them, declare each as `git = "<the repository>"`, `rev = "<the tag>"` and
`dir = "addons/context-budget"` (or `"addons/dcg"`).

## Changing this directory

Anyone may change it, the agents of an epic included. The daemon runs `scripts/` from the working tree at the moment
a step needs them, so an edit of `gate.sh` or `tests.sh` holds from the next step on. The config, the formula and
the prompts are held in memory: an edit of them holds only after `loop-manager config reload`, and a running run
keeps the ones it started with. `mise run ci` runs `scripts/selftest.sh` and `config check` on this directory.

## Moving over

The loop this pack replaces lives in `.claude/`, `scripts/loop/` and `.loop/`. The two do not work side by side:
they share one checkout and one branch. While `.claude/settings.json` has its hooks, they run in the sessions of
this pack too: a few shell commands are refused, and a subagent gets a second note on its context that names the
old limits and is stopped at 420000 tokens with a dump in `.loop/handoffs/`. To move over, between two cycles of the
old loop:

1. Remove `.claude/agents/` and `.claude/hooks/`, and the `hooks` table from `.claude/settings.json` (its `env` and
   `enabledPlugins` stay: the sessions of this pack inherit them).
2. Remove `scripts/loop/`.
3. In `CLAUDE.md` and `docs/agents/loop.md`, replace what describes cycles (`loopctl`, `scripts/loop/checks`,
   `.loop/cycles/`, `.loop/state.json`, `.loop/handoffs/`, the 300k and 420k thresholds, "the harness of the loop")
   with a pointer to this file; the roles of this pack tell their agents to skip those parts until then.
4. File what is left of `.loop/PLAN.md` as epics.

After a fresh clone br has no database yet: `cd devloop && br init --prefix dev` makes it, and br fills it from
`.beads/issues.jsonl`.
