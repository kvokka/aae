---
name: awaiting
description: Explain questions and decisions waiting in loop-manager, gather their task and report context, and pass on the user's answer. Use when the user asks what needs their attention, what a worker's question means, or wants to answer, accept, send back or cancel a run waiting on an await step.
---

# Runs that wait for a decision

A formula may stop between its steps on an `await` step and wait for an outcome from outside: a human, a script or a
bridge. Your job is to show the user what waits and to pass on what the user decides. Decide nothing yourself: never
give an outcome or cancel a run the user has not named, and use the user's own words for the summary.

The loop-manager commands below talk to the running daemon; if one answers that it cannot reach it, ask the user to start
`loop-manager daemon` (or name its socket with `--socket`) and stop there.

Read the CLI's structured replies directly in TOON. Add `--json` when passing a reply to a JSON parser.

## 1. See what waits

```sh
loop-manager run list --awaiting
loop-manager run list --awaiting --project <project>
```

For each run the user cares about:

```sh
loop-manager run show <run>
```

Show the user, per run: `id`, `project`, `formula`, `bead` (the task or the epic it works on), `step` (the `await`
step it waits on), `round` (one more each time a check sent it back) and, from `steps`, the outcome of the step
before the one it waits on: its `result`, `summary` and `data` (an exec step keeps its output in `data.stdout`,
where a script may leave, say, the address of a pull request). If there are many runs, ask which ones matter.

## 2. Understand the question before asking the user

An `await` is a saved workflow state: the worker's model session has ended, and no model or polling process waits
for the answer. The daemon advances when it receives `run resolve`; a restart preserves the wait. A closed issue
gate is different: opening a gate permits work to start or resume, but does not answer an `await` step.

`run show` gives the step results and the bead id, not the bead's description or comments. A `question_id` and a
short summary may only identify the question. Follow its references to recover the full context before presenting
it to the user. Discover the project's directory rather than assuming the run's `workdir` holds its tracker:

```sh
loop-manager project list --project <project>
```

For a project using br, its tracker is `<dir>/.beads`, where `dir` is the project entry's field. Read the task and
its comments there, with the bead id from the run:

```sh
BEADS_DIR='<project dir>/.beads' br show <bead> --format toon
```

Read the matching question, subsequent answers or corrections, recent coder and review reports, and any parent
task needed to understand the goal. Use referenced files, diffs and test artifacts in the run's `workdir` to resolve
ambiguity. A missing artifact is missing evidence, not permission to guess. Attaching to or resuming the worker's
session is unnecessary; reading its transcript is a fallback when these durable sources do not explain the issue.

Explain the task, the exact decision needed, why it blocks progress, the available options and their consequences.
Separate the worker's statements from your own deductions and recommendations. If the question is poorly phrased,
reconstruct it from the evidence rather than passing its shorthand on to the user. Keep related questions together
when useful, retaining each run, step and question id so an answer reaches the right wait.

For the `devloop` pack, read [its question and handoff procedure](references/devloop.md): the `QUESTION` / `DECISION`
comment convention, snapshot command and answer routing are pack-specific, not requirements on other formulas.

## 3. Pass on the decision

Offer these, for the user to pick one per run:

1. **Answer or accept**: the run goes on to its next step, or passes when the step is its last. Read the preceding
   outcome and any question or report it references first. A wait may ask for a decision needed to continue work;
   then the summary is the user's answer, not a statement that the whole task is complete. Explain what the next
   step does before resolving it.

   ```sh
   loop-manager run resolve <run> <step> pass --summary '<what the user said>'
   ```

2. **Send it back** with the user's remarks: a step with `back_to` returns the run to that earlier step, which gets
   the summary as its remarks, and this spends one of the formula's rounds; a step without `back_to` fails the run.
   Say which of the two it is before you run it: the step is a check when
   `loop-manager config explain --project <project> --formula <formula>` says so under it
   (`a check: on fail, back to ...`).

   ```sh
   loop-manager run resolve <run> <step> fail --summary '<the remarks of the user>'
   ```

3. **Cancel** the run: nothing more of it runs, and whatever the project starts on a cancelled run (a cleanup, say)
   starts.

   ```sh
   loop-manager run cancel <run> --reason '<why, in the words of the user>'
   ```

`<step>` is the step the run waits on, `step` of `loop-manager run show`. A run that no longer waits on that step
refuses the outcome and nothing changes: someone else decided first. Show the user the answer and the run's state after
it.

## Timeouts and optional observation

If the run ended with `reason: await_timeout`, the deadline for an external answer expired. Say that explicitly;
it does not mean the worker's code failed review. The run is no longer resolvable: use the `recover` skill's
[timeout procedure](../recover/SKILL.md#an-expired-external-wait) to preserve the answer before starting more work.
The deadline is carried by `run.awaiting`; `run show` does not currently expose it as a separate field.

This skill inspects current state on request. Start no background monitor or notification process unless the user
asks for one. The existing building blocks for a requested script are `loop-manager events --json` (one JSON event
per line), `run.awaiting` (the run and deadline), and the current-state queries above. Process each event as it
arrives. Resume from a journal cursor with `loop-manager events --from <seq> --json`. After a gap, reread current state.

The user's script can filter projects or task labels, notify on every question, or accumulate a batch. For
nonblocking concerns such as `devloop` decisions, br comments produce `bead.updated` with `changed: ["comments"]`;
read the comments to distinguish a new concern from an answer. The event does not contain their text. Read labels
from the tracker; `run.awaiting` is not a full task snapshot. Notification and answer submission are separate:
observing an event never authorizes a resolution. Query current state before acting, since a wait may have ended.
