# lessons

An addon of loop-manager for claude and codex agents: when an agent has finished its work on a bead, the same session
is asked, in the context it just used and while its prompt cache is warm, for the lessons that would have saved
another agent at least 1,000 tokens on similar work. A useful answer goes on the run's bead as one comment; an empty
answer, the usual one, leaves nothing. The addon ends there: what is done with the notes later is not its part.

## Install

In your project's `loop-manager.toml`, declare the Git source and attach the addon to the project's agents.
If you already have a `[settings]` table, append the attachment to its `addons` array.

```toml
[[addon]]
git = "https://github.com/kvokka/loop-manager.git"
rev = "addons/lessons/0.1.0"
dir = "addons/lessons"

[settings]
addons = [{ name = "lessons" }]
```

Run `loop-manager config check`, then `loop-manager config reload` to apply the change on the project's daemon.
To enable the addon for a single agent, put the attachment in that agent's `agent.toml` and keep the Git
declaration in the project file.

It has no params.

## How it works

The hook `hooks/lessons` runs on the harness's `Stop`, at the end of each turn of the session's own agent. It does
nothing in a run without a bead, for a subagent (an input with `agent_id`), or before the agent has declared its
outcome.

1. **The turn that finished the work ends.** The current attempt of the run has a declared outcome (`outcome` of
   `loop-manager run show "$LOOP_MANAGER_RUN"`), `pass` or `fail`. The hook answers `{"decision": "block", "reason":
   <request.md>}`: the harness does not end the turn but sends the request to the model as the next message of the
   same conversation. The outcome stays declared: the request asks for no tool and no change to it.
2. **The answer to the request ends.** The input has `stop_hook_active: true`; its `last_assistant_message` is the
   answer. An answer that is empty or only `NONE` publishes nothing. Any other answer goes on the bead of the run
   (`LOOP_MANAGER_BEAD`) with `br comments add`, as the session's tracker actor:

   ```text
   LESSONS: <run>

   - When <the case>, <what works>.
   ```

3. **Nothing more.** Each session is asked once and publishes once: a later stop of the session, as after a remark of
   a `run.finishing` check, asks nothing again, and the stop after the answer asks for no more lessons. A new session
   of the run (another attempt or step) is asked on its own when it declares an outcome.

[`request.md`](request.md) is the request: lessons from this session only, at least 1,000 tokens saved, each a short
note in ASD-STE100 Simplified Technical English that tells when it applies and what works, at most five, and `NONE`
when there is none.

The answer is a turn of its own: its tokens count in the run's usage and in the context of the session. It runs on
the clock of the step: the step's `timeout` and the silence watchdog go on through it, so an outcome declared at the
edge of the timeout can leave no time for the answer. When the daemon's session ends before the answer does (a crash,
a cancel, a timeout), there is no comment.

A session that a hook stopped with `continue: false`, as `context-budget` does past its `hard` limit, is not asked:
neither claude nor codex runs the `Stop` hook after it, though the outcome is declared.

The addon's data directory keeps, per session, the request (`<session>.asked`), the publication (`<session>.done`) and
the comment's text (`<session>.lessons.md`).

## On codex

The same hook and the same input fields: codex's `Stop` takes `decision: block` with a `reason` and runs it as the
next prompt of the turn, and its next `Stop` carries `stop_hook_active` and `last_assistant_message`.

## Needs on the host

`jq`, `br` (it finds the tracker by the session's `BEADS_DIR`, as the agent's own calls do) and `loop-manager` (to
read the run) on the `PATH` of the harness's hooks.

## Checking it

- The config: `loop-manager config check --config <file>` loads the declaration, and
  `loop-manager config explain --config <file> --project <p> --agent <a>` shows the `Stop` hook with the paths filled
  in for the agent.
- A run: after a run of an attached agent on a bead, `br comments <bead>` shows the `LESSONS` comment, or none.
- The tests: `crates/loop-manager/tests/e2e/lessons.rs` runs a real daemon with the real `claude` and the real
  `codex app-server` on fake providers. It checks that the request goes to the model on the conversation of the
  finished turn, that a useful answer reaches the run's bead once and no other bead, that an empty answer leaves no
  comment, that the request leaves the outcome as declared, that no request follows the answer, and that a session
  `context-budget` stopped is not asked. It drives the hook by hand for the rest: no outcome, a subagent, a run on no
  bead, the stops after the answer, and a stop of an asked session that is not the end of its answer.

See the [changelog](CHANGELOG.md) for version history.
