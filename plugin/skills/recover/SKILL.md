---
name: recover
description: Sort out the aftermath of loop-manager runs that failed for good. Use when the user asks what failed in loop-manager, why a run failed, or how to recover, retry, continue or abandon a failed run. Finds the failed runs and their sessions, shows the failure kinds, offers the ways to go on, and runs the one the user picks with loop-manager commands.
---

# Recover failed loop-manager runs

A run that failed for good is the user's to sort out: no recovery fits every failure. Your job is to show what
failed, lay out the ways to go on, and run the one the user chooses. Decide nothing yourself: never start, cancel or
gate anything the user has not picked. The full guide is
[Recovering a failed run by hand](https://github.com/kvokka/loop-manager/blob/main/docs/guide/recovery.md).

The loop-manager commands below talk to the running daemon; if one answers that it cannot reach it, ask the user to start
`loop-manager daemon` (or name its socket with `--socket`) and stop there.

Read structured output as TOON: `toon --encode` converts the CLI's JSON before it enters your context. It needs
`toon` 0.2.5 (`github:Dicklesworthstone/toon_rust`, pinned in this repository's `mise.toml`). Run the pipelines in
bash or zsh with `set -o pipefail`; an upstream CLI error must not become a successful empty result. Keep JSON when
another program consumes it.

## 1. See what failed

```sh
loop-manager run list --state failed --json | toon --encode
loop-manager run list --state failed --project <project> --limit 20 --json | toon --encode
```

The list is newest first. Ask which runs matter if there are many. For each run the user cares about:

```sh
loop-manager run show <run> --json | toon --encode
loop-manager run list --parent <run> --json | toon --encode
loop-manager gate list --json | toon --encode
```

`loop-manager run list --parent` lists the children of a run whose formula has a `drain` or `formula` step;
`loop-manager gate list` shows whether the bead's gate `bead:<project>/<bead>` is closed.

Show the user, per run: `id`, `project`, `rig`, `formula`, `bead`, `class` (`agent`, `check`, `hollow_done`,
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

Recover the question and its task context using [awaiting's context procedure](../awaiting/SKILL.md#2-understand-the-question-before-asking-the-user).
Explain the decision to the user before proposing recovery. For `devloop`, follow the
[pack-specific timeout procedure](../awaiting/references/devloop.md#after-an-owner-wait-times-out).

An expired run is terminal: `run resolve` cannot answer it. Preserve the user's answer in the task's documented
handoff format before reopening its gate or starting another run. The timeout counts toward `bead_failed_runs`, so
inspect the bead's gate as well; `devloop` closes it after one failed run. Gate opening alone records no answer.
Check whether an existing queued run will proceed when the gate opens before creating another run on the same bead.
Do not recommend continuing a worker session merely because it was the last session before the wait; the missing
input was an external decision, and the workflow must receive that answer through its task or inputs.

## 2. Offer the ways to go on

A failed run has let go of its bead: the bead is open again and its count of failed runs went up. Once the count
reaches `bead_failed_runs` (default 3) the gate `bead:<project>/<bead>` is closed and no run takes the bead until it is opened.

Offer these, for the user to pick one per run:

1. **Continue the session.** A new run continues the failed run's last session in the same workdir. The formula's
   first step must be `ai` on the harness of that session.
2. **A new session on the same task.** A new run of the formula on the same bead, in a new session.
3. **A new session with a note** saying the work was begun before and its state is unknown. This needs a formula
   with an input for the note that its agent's prompt template uses; look for one with
   `loop-manager formula list --json | toon --encode`. If there is none, point the user to the guide's section on it
   and stop: writing one is the user's change to their config.
4. **Abandon it.** The failed run needs nothing more. To keep every run off the bead, close its gate; to drop the
   task for good, the user closes the bead in the tracker.

## 3. Run the chosen way

Take `project`, `rig`, `formula` and the task input's name (often `task`) from `loop-manager run show`; leave `<rig>`
out when the run has none. The note formula's input names are its own
(`loop-manager formula list --json | toon --encode`). If the bead's gate is closed and the user wants the bead taken
again, open it first (that resets the count):

```sh
loop-manager gate open bead:<project>/<bead>
```

1. Continue the session:

   ```sh
   loop-manager run start <project> <rig> <formula> --continue-from <run> --input task=<bead>
   ```

2. A new session:

   ```sh
   loop-manager run start <project> <rig> <formula> --input task=<bead>
   ```

3. A new session with a note, on the formula the user picked:

   ```sh
   loop-manager run start <project> <rig> <note-formula> --input task=<bead> --input 'note=This task was started before by run <run>; its state is unknown. Find out what was done, then finish it.'
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
