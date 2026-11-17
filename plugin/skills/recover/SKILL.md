---
name: recover
description: Sort out the aftermath of aae runs that failed for good. Use when the user asks what failed in aae, why a run failed, or how to recover, retry, continue or abandon a failed run. Finds the failed runs and their sessions, shows the failure kinds, offers the ways to go on, and runs the one the user picks with aae commands.
---

# Recover failed aae runs

A run that failed for good is the user's to sort out: no recovery fits every failure. Your job is to show what
failed, lay out the ways to go on, and run the one the user chooses. Decide nothing yourself: never start, cancel or
gate anything the user has not picked. The full guide is
[Recovering a failed run by hand](https://github.com/kvokka/aae/blob/main/docs/guide/recovery.md).

Every command below talks to the running daemon; if one answers that it cannot reach it, ask the user to start
`aae daemon` (or name its socket with `--socket`) and stop there.

## 1. See what failed

```sh
aae run list --state failed --json
aae run list --state failed --project <project> --limit 20 --json
```

The list is newest first. Ask which runs matter if there are many. For each run the user cares about:

```sh
aae run show <run> --json
aae run list --parent <run> --json
aae gate list --json
```

`aae run list --parent` lists the children of a run whose formula has a `drain` or `formula` step;
`aae gate list` shows whether the bead's gate `bead:<project>/<bead>` is closed.

Show the user, per run: `id`, `project`, `rig`, `formula`, `bead`, `class` (`agent`, `check`, `hollow_done`,
`infrastructure`), `reason`, `workdir`, and per attempt its `kind` (`new_session` or `resume`), `failure_kind` and
`session`. The `session` is the harness's own reference (claude `{session_id, config_dir, workdir}`, codex
`{thread_id, codex_home}`); offer to read the conversation with the harness's own tools when the cause is unclear.

What the failure kind suggests, as a hint for the user and not a decision:

- `auth_required`, `billing`, `quota_exhausted`: the account needs the user first (log in, pay, wait for the
  reset); after that the session can be continued.
- `context_overflow`, `runaway_turn`, `transcript_divergence`: the session is spent; a new session fits better.
- `stall`, `harness_crash`, `adapter_crash`, `daemon_crash`, `overloaded`, `rate_limited`, `unknown`: the session
  may be fine to continue; read its end first.
- `config_error`, `tool_failure`, `workspace_failure`: fix the config, the MCP server or the workspace first.
- class `check` or `hollow_done`: the agent finished but its work did not pass; a new session with a note, or a
  look at the work, fits better than a plain continue.

## 2. Offer the ways to go on

A failed run has let go of its bead: the bead is open again and its failure count went up. Once the count reaches
`bead_failures` (default 3) the gate `bead:<project>/<bead>` is closed and no run takes the bead until it is opened.

Offer these, for the user to pick one per run:

1. **Continue the session.** A new run continues the failed run's last session in the same workdir. The formula's
   first step must be `ai` on the harness of that session.
2. **A new session on the same task.** A new run of the formula on the same bead, in a new session.
3. **A new session with a note** saying the work was begun before and its state is unknown. This needs a formula
   with an input for the note that its agent's prompt template uses; look for one with `aae formula list --json`.
   If there is none, point the user to the guide's section on it and stop: writing one is the user's change to
   their config.
4. **Abandon it.** The failed run needs nothing more. To keep every run off the bead, close its gate; to drop the
   task for good, the user closes the bead in the tracker.

## 3. Run the chosen way

Take `project`, `rig`, `formula` and the task input's name (often `task`) from `aae run show`; leave `<rig>` out when
the run has none. The note formula's input names are its own (`aae formula list --json`). If the bead's gate is
closed and the user wants the bead taken again, open it first (that resets the failure count):

```sh
aae gate open bead:<project>/<bead>
```

1. Continue the session:

   ```sh
   aae run start <project> <rig> <formula> --continue-from <run> --input task=<bead>
   ```

2. A new session:

   ```sh
   aae run start <project> <rig> <formula> --input task=<bead>
   ```

3. A new session with a note, on the formula the user picked:

   ```sh
   aae run start <project> <rig> <note-formula> --input task=<bead> --input 'note=This task was started before by run <run>; its state is unknown. Find out what was done, then finish it.'
   ```

4. Abandon it:

   ```sh
   aae gate close bead:<project>/<bead> --reason 'abandoned'
   ```

   A run that is not over yet (`queued`, `parked`, `interrupted`) is stopped instead with:

   ```sh
   aae run cancel <run> --reason 'abandoned'
   ```

`aae run start` prints the new run's id. Offer to follow it and tell the user how it ended:

```sh
aae run watch <new-run>
aae run wait <new-run> --timeout 2h
```
