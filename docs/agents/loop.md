# The autonomous loop

aae is developed by a loop of coder → reviewer under a master (`overseer`), with no operator. The protocol of a
cycle is in the agent definitions (`.claude/agents/`) and in `scripts/loop/loopctl`; this page holds what is common
to the roles.

## No operator

- Questions are not asked in the chat. Everything the owner needs (findings, questions, decisions on
  [assumptions](assumptions.md), blockers, violations) is appended to `.loop/JOURNAL.md`; past entries are never
  changed. For a question, write which option you took until the owner answers.
- A departure from a [decision](decisions.md) is not an "implementer's decision", even when written to the journal:
  it is a mandatory item of the review, marked "waits for the owner".
- Do not do more than was asked. What the owner did not ask for is not done, and nothing is built for later.

## Checks

- While working: targeted tests. Once, at the end, on the committed HEAD: `scripts/loop/checks NN`, which runs
  `mise run ci` and writes `.loop/cycles/NN/checks.md`. The reviewer reads that file and does not run the gate again
  when its `HEAD:` line is the current HEAD. GitHub CI runs the same and is not waited for.
- Acceptance scenarios, live models and the demos: only when the plan package or the owner's journal entry names
  them ([test rules](rules-tests.md)).
- Before the last answer stop everything you started in the background and wait for it to end.

## Context budget

- Soft limit 300k tokens, hard limit 420k: at 300k a hook asks you to wrap up, at 420k the agent is stopped and its
  parent gets an automatic dump from `.loop/handoffs/`. Working agents have no compaction: plan to finish by ~250k.
- A plan package is S (up to 60k of working context) or M (up to 150k); one M or two S per cycle.
- Grep before Read; Read with `offset` and `limit`; never read `Cargo.lock`, `target/` or a long log whole. The big
  documents (`docs/design/contracts.md`, `docs/design/config.md`) are read by section: `grep -n '^##' <file>`.
- Heavy reading and long runs go to subagents, at most three at once. A brief is self-contained: the goal, the
  files, the limits, where to put the result, the format of the answer, the time by which to hand in what is done.
  A long result goes to a file, the answer is a short summary.

## The expert

- For the hard parts (the core, the invariants, recovery, harness protocols, a disputed design choice, a bug after
  two failed attempts) call `expert`. It also writes the plan of work: say so in the brief ("planning, not
  implementation"), give the goal, the documents by section, the files, and the constraint that each piece fits
  100–150k of context with a checkable "done when".
- The expert plans the large part; you take as many pieces as fit your budget, the rest stays in the plan for the
  next cycle. Pieces it marks as parallel go to `subcoder`s on disjoint files.
- If the expert says how to do a thing, follow it; a departure needs a reason in the report.

## The plan and br

The plan is `.loop/PLAN.md`; an expert's plan of a package is an epic in br with a child task per piece.

- The working directory is the root of the repository. `.beads/` is committed with the code
  (`br sync --flush-only`, then `git add .beads`).
- Always machine output: `--json` (or `--format toon`); `RUST_LOG=error br …` for a clean stdout.
- Start of work: `br ready --json`, `br show <id> --json`.
- File a task: `br create "<title>" -t task -p 2 -d "<description>" --parent <epic-id> --json`; a dependency:
  `br dep add <task> <what it waits for>`.
- Take it: `br update <id> --claim --actor "$AGENT_NAME" --json`; close it:
  `br close <id> --reason "Completed: <what, and what proves it>" --json`.
- Never run a bare `bv` in automation (it is interactive); only `bv --robot-*`.

## The harness of the loop

`.claude/`, `scripts/loop/` and `CLAUDE.md` are not changed inside a cycle (a hook blocks it, `loopctl` reverts
it). A proposal about them is a journal entry.

- The owner changes the harness between cycles and commits it before a cycle opens: `loopctl` reverts every change
  of these paths since the base of the open cycle, whoever made it, and `scripts/loop/checks` refuses a tree with
  uncommitted changes outside `.loop/`.
- Never hide a file of `.loop/` from git (`git update-index --assume-unchanged` or `--skip-worktree`): a commit
  hook that stashes unstaged changes then rolls the state and the journal back, and a cycle opens twice.
- The owner prunes `.loop/` by hand from time to time: old cycles go, and what of the journal still holds moves
  to `docs/` or stays as a short entry. What was pruned is in the git history.
