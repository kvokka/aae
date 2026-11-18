# devloop: loop-manager develops loop-manager

A loop-manager project whose rig is this repository. The owner files a task as a bead; a run of the formula `task`
has a coder do it, runs the CI gate, has a reviewer review it, and sends the work back to the coder until the review
scores 9 or more. Nothing here is part of the product: it is a pack and a config, read by `loop-manager` as any
other.

```text
devloop/
  config.toml           the orchestrator config of the loop's own daemon
  loop-manager.toml     the project: the rig `repo` (the repository, `..`), the addon, the trigger
  agents/               coder and reviewer (steps of the formula); helper, subcoder, expert (subagents of the coder)
  formulas/task.toml    code -> ci -> review -> gate
  scripts/              the check scripts of `code` and `review`, the gate, and their self-test
  .beads/               the tracker of the tasks (br), committed with the code
```

## A run

| Step | What it does | When it does not pass |
|---|---|---|
| `code` | the coder does the task, commits, writes `.loop/runs/<run>/report.md`; `scripts/check-report.sh` wants the report's first three lines and a tree with nothing uncommitted outside `.loop/` | the coder gets the script's output and goes on, up to 3 times |
| `ci` | `mise run ci` on HEAD | back to `code` with the tail of the output |
| `review` | the reviewer writes `.loop/runs/<run>/review.md`; `scripts/check-review.sh` wants its `SCORE:` and `VERDICT:` lines and an unchanged tree | the reviewer gets the script's output and goes on, up to 3 times |
| `gate` | `scripts/gate.sh` commits `.loop/` and `devloop/.beads/`, pushes, and passes on `SCORE` of 9 or more with `VERDICT: complete` | back to `code`; the review is the remark |

A run has 4 rounds (`rounds` in `formulas/task.toml`); a red `ci` spends one, and so does a review under the bar. A
run that passes closes its bead. A run that fails releases it: the bead is ready again and gets a new run with new
sessions, which read the reports and reviews the earlier runs left. After 3 failed runs (`bead_failures`) loop-manager
closes the gate `bead:devloop/<id>`, and nothing more starts on that bead until you open it
(`loop-manager gate list`, `loop-manager gate open <key>`).

One run at a time works in the repository (`max_writers = 1` of the rig): the others wait in `queued`. The session
of the coder and of the reviewer is compacted at a window of 300000 tokens, and the run fails at the fifth compaction
of one session (the addon `addons/context-budget`, attached in the two `agent.toml`).

## Before the first start

- `loop-manager`, `claude`, `br`, `jq`, `git` and `mise` are on the `PATH` of the shell that starts the daemon, and
  `loop-manager doctor --config devloop/config.toml` is content.
- `git push` works from that shell without a question: see "The push".
- The loop this pack replaces is switched off: see "Moving over". Until then its hooks in `.claude/settings.json`
  run inside every claude session that works in this repository, the sessions of this pack included.

## Start it

```sh
loop-manager daemon --config devloop/config.toml
```

The daemon keeps its state in the XDG state directory and listens on its default socket. To keep it apart from
another daemon of yours, give it its own `XDG_STATE_HOME` and `LOOP_MANAGER_SOCKET`, as `demos/*/run.sh` do, and
export the same `LOOP_MANAGER_SOCKET` in the shell you run the commands below from.

## File a task

A ready bead with the label `auto` starts a run; a bead without it starts nothing.

```sh
cd devloop
br create "Short title of the task" -l auto -d "What to do, by which documents, and when it is done"
```

Write the description as a plan package is written: the documents, the result, the checkable "done when". The size
of a task is what one coder session does, with its subagents. A task that waits for another is filed with a
dependency (`br dep add <task> <what it waits for>`) and becomes ready when that one is closed.

By hand, without the label: `loop-manager run start devloop repo task --input task=<bead id>`, or
`--input 'task=<the text of the task>'`, which files the bead.

## Watch it

```sh
loop-manager run list                 # the runs, newest first: state, step, formula
loop-manager run show <run>           # one run: its round, the outcomes of its steps, why it failed
loop-manager run watch <run>          # one run live, with the messages of its session
loop-manager events                   # every event, live
loop-manager trigger list             # `task-ready` and whether it is on
```

What a run leaves for you is in the repository, committed and pushed by the gate after every reviewed round:
`.loop/runs/<run>/report.md` and `review.md` (the history of a run's rounds is the git history of these files),
`notes/` beside them, and `.loop/JOURNAL.md`, where the agents write what you need to know or decide. A coder that
cannot go on without you writes the blocker there and fails its run.

## The push

The exec steps and the check scripts get the whole environment of the daemon (`[exec] inherit_env = true` in
`loop-manager.toml`), not `PATH`, `HOME` and `LANG` alone, so the `git push` of the gate authenticates as your shell
does: through the ssh agent (`SSH_AUTH_SOCK`), a token (`GH_TOKEN`, `GITHUB_TOKEN`) or a credential helper. Check
it in the shell that will start the daemon: `git push --dry-run`. A key with a passphrase and no agent cannot work:
nothing can ask for it, and the gate fails instead of waiting. When the review passes and the push does not, the
step is tried again three times, 30 s apart, and then the run fails; the commit stays local and the next push takes
it along.

## Who may change this directory

Only you. The daemon holds the config in memory until `loop-manager config reload`, but it runs the scripts under
`scripts/` from the working tree at the moment a step needs them: a coder that edits `scripts/gate.sh` changes the
check of its own round. The reviewer therefore reports any change under `devloop/`, other than `.beads/`, as a
mandatory item that waits for you, and a run with such a change does not pass by itself.

If that is not enough, pin the pack. `packs` in `loop-manager.toml` takes `{ git = "<url>", rev = "<commit or
tag>" }`, where the url is anything `git clone` takes, an absolute path of a local repository or `file://` included,
and loop-manager then reads the pack from a checkout in its cache that no agent works in. A pack from git is a
repository with `pack.toml`, `agents/` and `formulas/` at its root (there is no key for a subdirectory), so the pin
needs a repository of its own for `agents/`, `formulas/` and `scripts/`, not this one. The check scripts are found
from the formula's directory and so come from the pinned checkout; the command of the step `gate` is found from the
rig, so in a pinned pack make it a check script too (`step_check_script_path` on a step whose command is `true`).

## Moving over

The loop this pack replaces lives in `.claude/` and `scripts/loop/`. The two do not work side by side: its hooks
would run in the sessions of this pack, and both would commit to `.loop/`. To move over, between two cycles of the
old loop:

1. Remove `.claude/agents/` and `.claude/hooks/`, and the `hooks` table from `.claude/settings.json` (its `env` and
   `enabledPlugins` stay: the sessions of this pack inherit them).
2. Remove `scripts/loop/`.
3. In `CLAUDE.md` and `docs/agents/loop.md`, replace what describes cycles (`loopctl`, `scripts/loop/checks`,
   `.loop/cycles/`, `.loop/state.json`, `.loop/handoffs/`, the 300k and 420k thresholds, "the harness of the loop")
   with a pointer to this file; the roles of this pack tell their agents to skip those parts until then.
4. File what is left of `.loop/PLAN.md` as beads.

After a fresh clone br has no database yet: `cd devloop && br init --prefix dev` makes it, and br fills it from
`.beads/issues.jsonl`.
