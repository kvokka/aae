---
name: recover
description: Diagnose failed loop-manager runs and carry out the recovery the user chooses.
---

# Recover failed loop-manager runs

Inspect the failed run on the user's selected daemon. Explain the cause and apply the user's chosen recovery.
Read structured replies in TOON. When a replacement run you follow finishes, use [recap](../recap/SKILL.md).
See the [recovery guide](https://github.com/kvokka/loop-manager/blob/main/docs/guide/recovery.md) for CLI details.

## 1. See what failed

```sh
loop-manager run list --state failed
loop-manager run list --state failed --project <project> --limit 20
```

The list is newest first. Ask which runs matter if there are many. For each run the user cares about:

```sh
loop-manager run show <run>
loop-manager run list --parent <run>
loop-manager gate list
```

`loop-manager run list --parent` lists the children of a run whose formula has a `drain` or `formula` step;
`loop-manager gate list` shows whether the bead's gate `bead:<project>/<bead>` is closed.

Identify the run, project, formula and any associated rig or bead. For diagnosis, inspect `class` (`agent`, `check`, `hollow_done`,
`infrastructure`), `reason`, `workdir`, and per attempt its `kind` (`new_session` or `resume`), `failure_kind` and
`session`. The `session` is the harness's own reference (claude `{session_id, config_dir, workdir}`, codex
`{thread_id, codex_home}`). Start with task comments, step results and referenced artifacts; a transcript is a
fallback when those do not explain the cause. Reading context does not require attaching to or resuming the session.

Inspect `reason` before interpreting the broad `class` or an attempt's `failure_kind`. In particular,
`await_timeout` is an expired external wait, not a model failure or a failed review; follow the procedure below.

What the failure kind suggests, as a hint for the user and not a decision:

- `auth_required`, `billing`, `quota_exhausted`: the account needs the user first (log in, pay, wait for the
  reset); after that the session can be continued.
- `context_overflow`, `runaway_turn`, `transcript_divergence`: the session is spent; a new session fits better.
- `stall`, `harness_crash`, `adapter_crash`, `daemon_crash`, `overloaded`, `rate_limited`, `unknown`: the session
  may be fine to continue; read its end first.
- `config_error`, `tool_failure`, `workspace_failure`: fix the config, the MCP server or the workspace first.
- Other class `check` or `hollow_done` failures: the work did not pass its completion conditions; a new session with a note, or a
  look at the work, fits better than a plain continue.

### An expired external wait

`state: failed`, `class: check`, `reason: await_timeout` means no external outcome arrived before the await deadline.
Show it as "The deadline for an external answer expired (await_timeout)", with the step and question it concerned.
It need not appear in an attempt's `failure_kind`: there was no harness running in that step. By contrast, the CLI
command `loop-manager run wait <run> --timeout 2h` ending with exit 124 only ends that client's wait; it does not fail
the underlying run.

Recover the question using [awaiting's context procedure](../awaiting/SKILL.md#understand-the-question).
Explain the decision to the user. The expired run is terminal. For a retry, carry the user's answer through
the project's documented task or formula input. For runs with a bead, check its gate and preserve the answer
before reopening it. The timeout counts toward the project's configured failure threshold.
Check whether an existing queued run will proceed when the gate opens before creating another run on the same bead.
Do not recommend continuing a worker session merely because it was the last session before the wait; the missing
input was an external decision, and the workflow must receive that answer through its task or inputs.

## 2. Offer the ways to go on

For a run that held a bead, failure releases it and increments its failure count. Once the count
reaches `bead_failed_runs` (default 3) the gate `bead:<project>/<bead>` is closed and no run takes the bead until it is opened.

Offer these, for the user to pick one per run:

1. **Continue the session.** A new run continues the failed run's last session in the same workdir. The formula's
   first step must be `ai` on the harness of that session.
2. **A new session on the same task.** Start the formula with the original inputs and a fresh session.
3. **A new session with a note** saying the work was begun before and its state is unknown. This needs a formula
   with an input for the note that its agent's prompt template uses; look for one with
   `loop-manager formula list`. If there is none, point the user to the guide's section on it
   and stop: writing one is the user's change to their config.
4. **Abandon it.** The failed run needs nothing more. To keep every run off the bead, close its gate; to drop the
   task for good, the user closes the bead in the tracker.

## 3. Run the chosen way

Use the failed run's project, formula, inputs and optional rig. In these examples, substitute the chosen
formula's input names and values. Omit the rig or task input when the run has none. If the bead's gate is closed and the user wants the bead taken
again, open it first (that resets the count):

```sh
loop-manager gate open bead:<project>/<bead>
```

1. Continue the session:

   ```sh
   loop-manager run start <project> <rig> <formula> --continue-from <run> --input <task-input>=<bead>
   ```

2. A new session:

   ```sh
   loop-manager run start <project> <rig> <formula> --input <task-input>=<bead>
   ```

3. A new session with a note, on the formula the user picked:

   ```sh
   loop-manager run start <project> <rig> <note-formula> --input <task-input>=<bead> --input '<note-input>=This task was started before by run <run>; its state is unknown. Find out what was done, then finish it.'
   ```

4. Abandon it:

   ```sh
   loop-manager gate close bead:<project>/<bead> --reason 'abandoned'
   ```

   A run that is not over yet (`queued`, `parked`, `interrupted`) is stopped instead with:

   ```sh
   loop-manager run cancel <run> --reason 'abandoned'
   ```

`loop-manager run start` prints the new run's id. Offer to follow it and tell the user how it ended:

```sh
loop-manager run watch <new-run>
loop-manager run wait <new-run> --timeout 2h
```
