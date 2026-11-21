---
name: awaiting
description: Explain what a waiting loop-manager run needs and submit the user's decision.
---

# Handle a waiting run

Use the user's selected daemon. Read CLI replies in TOON. To find the active wait and its context:

```sh
loop-manager run list --awaiting
loop-manager run list --awaiting --project <project>
loop-manager run show <run>
```

## Understand the question

Identify the current step, the preceding outcome and its summary, data or artifact references. Read the
formula's instructions to learn what the external outcome means. An approval, a choice of input and a request
for changes can all use an await step. If there is a bead, locate its tracker via the project's directory:

```sh
loop-manager project list --project <project>
BEADS_DIR='<project dir>/.beads' br show <bead> --format toon
```

Use the br command for projects with that tracker. Read the task, relevant comments and later corrections.
Follow referenced artifacts from the run's workdir when needed. Explain what is blocked, the available choices
and their consequences. Keep each decision tied to its run and step.

## Submit the user's choice

Check that the run still waits on the same step. Act on the user's chosen outcome and use their words in the
summary. Check the formula's routing with `loop-manager config explain --project <project> --formula <formula>`.
A pass advances the formula. A fail on a step with `back_to` returns the user's remarks to the target step and
uses a round. A fail without that route ends the run. Cancellation ends it as cancelled.

```sh
loop-manager run resolve <run> <step> pass --summary '<answer>'
loop-manager run resolve <run> <step> fail --summary '<remarks>'
loop-manager run cancel <run> --reason '<reason>'
```

Confirm the resulting state. If someone else already resolved the wait, report the current outcome. When a run
you follow finishes, use [recap](../recap/SKILL.md). For an expired wait, use [recover](../recover/SKILL.md#an-expired-external-wait).

## Observe waits when requested

A waiting run survives daemon restarts. For requested notifications, `loop-manager events --json` emits
`run.awaiting` with the run and deadline. Resume with `loop-manager events --from <seq> --json` and refresh
the current run list after a gap. Projects may also record nonblocking concerns in task comments.
A `bead.updated` event with a comments change signals when to reread the task. Use the project's convention
to interpret it. Resolving a wait requires the user's decision.
