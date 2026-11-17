---
name: awaiting
description: Show the aae runs that wait for a decision from outside and pass on the user's decision. Use when the user asks what waits for them in aae, what waits for a review or an approval, or wants to accept, send back or cancel a run that stopped on an await step. Lists the waiting runs with what they did so far, and gives the outcome or cancels the run the user names.
---

# Runs that wait for a decision

A formula may stop between its steps on an `await` step and wait for an outcome from outside: a human, a script or a
bridge. Your job is to show the user what waits and to pass on what the user decides. Decide nothing yourself: never
give an outcome or cancel a run the user has not named, and use the user's own words for the summary.

Every command below talks to the running daemon; if one answers that it cannot reach it, ask the user to start
`aae daemon` (or name its socket with `--socket`) and stop there.

## 1. See what waits

```sh
aae run list --awaiting --json
aae run list --awaiting --project <project> --json
```

For each run the user cares about:

```sh
aae run show <run> --json
```

Show the user, per run: `id`, `project`, `formula`, `bead` (the task or the epic it works on), `step` (the `await`
step it waits on), `round` (one more each time a check sent it back) and, from `steps`, the outcome of the step
before the one it waits on: its `result`, `summary` and `data` (an exec step keeps its output in `data.stdout`,
where a script may leave, say, the address of a pull request). If there are many runs, ask which ones matter.

## 2. Pass on the decision

Offer these, for the user to pick one per run:

1. **Accept** the work: the run goes on to its next step, or passes when the step is its last.

   ```sh
   aae run resolve <run> <step> pass --summary '<what the user said>'
   ```

2. **Send it back** with the user's remarks: a step with `back_to` returns the run to that earlier step, which gets
   the summary as its remarks, and this spends one of the formula's rounds; a step without `back_to` fails the run.
   Say which of the two it is before you run it: the step is a check when
   `aae config explain --project <project> --formula <formula>` says so under it (`a check: on fail, back to ...`).

   ```sh
   aae run resolve <run> <step> fail --summary '<the remarks of the user>'
   ```

3. **Cancel** the run: nothing more of it runs, and whatever the project starts on a cancelled run (a cleanup, say)
   starts.

   ```sh
   aae run cancel <run> --reason '<why, in the words of the user>'
   ```

`<step>` is the step the run waits on, `step` of `aae run show`. A run that no longer waits on that step refuses the
outcome and nothing changes: someone else decided first. Show the user the answer and the run's state after it.
