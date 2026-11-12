# Experiments (spec §16)

Results of the throwaway experiments that precede the core. Code lives in `experiments/` (a separate Cargo
workspace outside the product budgets, ≤ 1 500 lines in total, checked by `scripts/budget.sh`). Every number carries
a `MEASURE` marker. Logs are sanitized samples (`<home>`, `<work>` placeholders; long `tools`/`skills` arrays are
replaced by their length) and serve as fixtures for the scripted harness.

Common setup for the claude experiments: `claude -p --input-format stream-json --output-format stream-json
--verbose --setting-sources "" --strict-mcp-config --model haiku`, clean environment (only `PATH`, `HOME`, `LANG`,
`USER`, `TERM`), `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, subscription login, every
process in its own process group, a fresh empty work directory.

## claude protocol (experiment 1a)

Code: `experiments/01-claude-protocol` (`src/lib.rs` is the hand-written client, `src/main.rs` the scenarios
`basic`, `resume`, `permission`, `interrupt`). Logs: `experiments/01-claude-protocol/logs/`.

<!-- MEASURE id=claude-protocol-1a date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/01-claude-protocol/logs note="haiku, subscription login, n=1 per scenario" -->

| Question | Answer |
|---|---|
| "reply OK" round trip | Works. A turn takes 0.9–1.0 s from writing the prompt to `result`. `system/init` repeats on every turn with the same `session_id` as `--session-id`. |
| Output before the first prompt | Nothing in 3 s. `init` comes only after a prompt is written, so the driver never waits for `init` before the prompt. |
| Native prompt acknowledgement | Yes: `{"type":"command_lifecycle","command_uuid":<uuid of our user message>,"state":"queued"\|"started"\|"completed"}`. `queued` arrives 6–9 ms after the write and `started` within 15 ms, `completed` right after `result`. Emitted with and without `--replay-user-messages`; `init.capabilities` lists `msg_lifecycle_v1`. The `isReplay` echo of `--replay-user-messages` is not an acknowledgement: it arrives 0.9 s later, just before the reply. |
| Idempotency key | The `uuid` field of the user message we write becomes the `uuid` of the user record in the transcript (`<config>/projects/<dir>/<session>.jsonl`). Writing a message whose `uuid` is already in the session yields `command_lifecycle{state:"completed"}` at once and no turn. So the key is the message `uuid` (derive it deterministically from the attempt key); after a daemon crash the transcript is searched for it. Limit: only a *finished* turn was observed. If the daemon dies after `queued` and the turn is cut, the user record is already in the transcript without an answer; a resend with the same `uuid` will most likely also complete silently and the attempt would hang. So after a restart the adapter checks the transcript for an answer after our `uuid` and, if there is none, sends a continuation with a new `uuid` (WP-27, contract tests WP-28). |
| `--resume` with stream-json input | Works: the context survives (the resumed session recalled a word from the first one), `session_id` is unchanged, `init` after the prompt as usual. |
| Model after resume | Not reset. Without `--model`, a fresh session started on the account default (`claude-opus-5-5`), a resumed haiku session started on `claude-haiku-5-5`. The adapter still passes the model explicitly. (The no-`--model` runs used an unreachable `ANTHROPIC_BASE_URL`, so no non-haiku request was made; `init` already names the model.) |
| Permission request | `--permission-prompt-tool stdio`: `session_state_changed{requires_action}` and `control_request{subtype:"can_use_tool"}` arrive together; the answer is a `control_response` (shape below), then `{running}`. A deny with a message reaches the model as `tool_result{is_error:true, content:<message>}` with `tool_result_meta[].non_execution_kind:"permission-rule"`; the model went on with the next step. |
| Turn cancellation | `{"type":"control_request","request_id":…,"request":{"subtype":"interrupt"}}` → `control_response{subtype:"success", response:{still_queued:[]}}` in 16 ms, then `result{subtype:"error_during_execution", is_error:true, terminal_reason:"aborted_tools", stop_reason:"tool_use"}` 43 ms after the request. The running Bash command and its children were gone 1 s later. The next prompt in the same process worked. |

<!-- MEASURE id=claude-ack-latency date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/01-claude-protocol/logs/basic.ndjson note="command_lifecycle queued 6–9 ms, started 7–15 ms after the write; n=2 turns" -->
<!-- MEASURE id=claude-native-interrupt date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/01-claude-protocol/logs/interrupt.ndjson note="n=1, during a foreground Bash python sleep; control_response 16 ms, result 43 ms" -->

Wire shapes the adapter relies on (full samples in the logs):

```json
{"type":"user","uuid":"<our id>","session_id":"","parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"text","text":"…"}]}}
{"type":"command_lifecycle","command_uuid":"<our id>","state":"queued","session_id":"…","uuid":"…"}
{"type":"control_response","response":{"subtype":"success","request_id":"<from control_request>","response":{"behavior":"deny","message":"…"}}}
{"type":"control_response","response":{"subtype":"success","request_id":"<from control_request>","response":{"behavior":"allow","updatedInput":{…}}}}
{"type":"control_request","request_id":"<ours>","request":{"subtype":"interrupt"}}
```

Consequences for the claude adapter (WP-27):

- "Prompt accepted" = `command_lifecycle{queued}` with our `command_uuid`; a timeout without it is an adapter
  failure, not a turn.
- An interrupted turn ends with `is_error:true` and `terminal_reason:"aborted_tools"` (or another
  `error_during_execution`); after our own `interrupt` it maps to `turn.ended{stop_reason: cancelled}`, never to a
  failure.
- `command_lifecycle` is a stream type the context notes did not list; unknown types must be ignored, not fatal.

## claude processes and background work (experiment 1b)

Code: `experiments/02-claude-process` (`orphans`, `orphans subreaper`, `background`). Logs:
`experiments/02-claude-process/logs/`.

<!-- MEASURE id=claude-process-1b date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/02-claude-process/logs note="haiku, n=1 per scenario" -->

### Descendants after `kill -9` (invariant 7)

The Bash tool runs each command through the user's shell in a **new session**: the shell has `pgid = sid = its own
pid`, its children share that group. `kill -KILL -<claude pgid>` therefore kills claude only; the shell and both
python processes survived and were re-parented to PID 1. With `PR_SET_CHILD_SUBREAPER` set on the experiment process
they were re-parented to it instead. The environment of the claude process (a marker variable) was inherited by
every descendant, so a scan of `/proc/<pid>/environ` found all of them, and killing the tagged processes left none.

cgroup v2 on the owner's host: `/sys/fs/cgroup` is mounted read-only, PID 1 is `tini`, no systemd, no delegated
subtree — a per-session cgroup is unavailable without root. On GitHub runners it was not tried.

**Chosen method** for "no orphans":

1. Every session process in its own process group (`kill(-pgid)` for the harness itself).
2. A per-attempt marker variable in the session environment; on cleanup the daemon kills every process carrying
   the marker (`/proc/<pid>/environ`; macOS is out of scope for the first release, spec §1.1). The same sweep runs during
   recovery after a daemon crash.
3. Linux: the daemon is a child subreaper, so descendants that dropped the marker still become its children and are
   reaped. The daemon must reap only pids it does not manage itself (peek with `waitid(P_ALL, WNOWAIT)` first), or
   it steals exit statuses of its own session processes.

cgroups are not needed for the first release. The remaining gap: a descendant that clears its environment and is
orphaned while the daemon itself is down (it goes to PID 1, not to the restarted daemon).

<!-- MEASURE id=claude-bash-own-session date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/02-claude-process/logs/orphans.ndjson note="n=2 (with and without subreaper); 3 descendants survived kill(-pgid) each time" -->

### Heartbeat of a long tool

`tool_progress{heartbeat:true}` at 30.0 s and 60.0 s after the `tool_use` of a 100 s foreground Bash command —
the 30 s interval of the context notes holds.

<!-- MEASURE id=claude-tool-heartbeat-1b date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/02-claude-process/logs/orphans.ndjson note="n=1, Bash in the main thread" -->

### "Idle without background" (both signals in one run)

One prompt started a background Bash (20 s) and a background subagent (which itself ran a 10 s foreground Bash),
with `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1`. Timeline (seconds from spawn):

| t | event | state | background tasks |
|---|---|---|---|
| 0.20 | `session_state_changed` | running | — |
| 2.06–2.24 | `background_tasks_changed`, `task_started` ×2 | running | bash, agent |
| 3.28 | `result` (our prompt) | **running** | bash, agent |
| 14.27 | agent `task_notification`, `background_tasks_changed` | running | bash |
| 14.32–15.36 | self-started turn, `result{origin.kind:"task-notification"}` | running | bash |
| 15.37 | `session_state_changed` | **idle** | **bash still running** |
| 22.10 | bash `task_notification`, `background_tasks_changed{tasks:[]}` | idle | none |
| 22.20–23.34 | `{running}`, self-started turn, `result{origin.kind:"task-notification"}` | running | none |
| 23.34 | `session_state_changed` | idle | none |

Findings: the state stays `running` after `result` while a background agent lives, but turns `idle` while a
background shell command still runs; one prompt produced three `result`s; and "state idle + empty task list" held
for 0.1 s at 22.10 right before the harness woke itself up.

**Rule for the adapter**: the session is idle without background work when (a) the last `session_state_changed`
is `idle`, (b) the last `background_tasks_changed` had an empty list (or none arrived since spawn), and (c) no
`task_notification` arrived after the last `result`. Condition (c) closes the wake-up window seen at 22.10.

<!-- MEASURE id=claude-idle-two-signals date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/02-claude-process/logs/background.ndjson note="n=1; state idle with a live background shell, 0.1 s idle+empty window before a self-started turn" -->

## br as an event source (experiment 3)

Code: `experiments/05-br-events` (one scenario, fresh workspace with an isolated `HOME`, the `notify` crate on the
`.beads/` directory, `rusqlite` read-only on `beads.db`). Output: `experiments/05-br-events/logs/run.txt`.

<!-- MEASURE id=br-events-exp3 date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="20 creates, 9 reads, 16 concurrent claims, 4 attribution cases; database of 20 beads; two runs agree" -->

| Question | Answer |
|---|---|
| Signal | Every mutating command rewrites `issues.jsonl` by rename exactly once; `notify` reports it as two events, `Modify(Name(To))` and `Modify(Name(Both))`. 20/20 sequential creates and 16/16 concurrent claims gave one rename each — no loss, no extra. Reads (`list`, `show`, `ready`) gave none; the database files see open/close/write traffic on reads too, so they are not a signal. |
| Watcher cost | 110 ms of CPU for the whole watching process over steps 1–4 (20 creates, 9 reads, 16 concurrent claims, 4 attribution cases): `notify` plus a read-only `events` query per signal. |
| Latency | The rename lands 23–62 ms after the command starts (median 41 ms) and always before the command exits. |
| Change content | The `events` table, read after the cursor (`WHERE id > ?`), with the database opened read-only per signal and closed at once: 0.6–0.7 ms per read including open/close. It held exactly one row per create and two per claim (`status_changed` + `assignee_changed`), 32/32 rows attributed to the right claimer under concurrency. A full snapshot (`br list --all --json`) costs 8–9 ms at 20 beads and grows linearly (60 ms at 5 000 beads per the context notes) and carries no actor. |
| Attribution | `--actor X` and the environment variable `BD_ACTOR=X` both land in `events.actor`. `BR_ACTOR` does nothing; `BR_AGENT_NAME`, `BR_HARNESS`, `BR_MODEL` are **not** recorded in 0.7.6 (columns stay empty) despite the help text. |

<!-- MEASURE id=br-rename-latency date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="n=20 sequential creates, 20-bead database; grows with database size (auto-flush rewrites the whole file)" -->
<!-- MEASURE id=br-watcher-cpu date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="getrusage of the experiment process over steps 1–4, n=1; includes the scenario driver itself, so an upper bound" -->
<!-- MEASURE id=br-events-read-cost date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="rusqlite bundled, open read-only + query + close, mean of 100, ~60 rows" -->

**Chosen method** for `bead.*` (WP-31):

1. Watch the `.beads/` directory (not the file: the rename replaces the inode). Any rename of `issues.jsonl` means
   "something changed"; signals are coalesced, and a missed or duplicate signal costs nothing because of step 2.
2. On a signal, open `beads.db` read-only, read `events` after the stored cursor, close. Map rows to `bead.*` events;
   `bead.ready` is derived by re-reading `br ready` and diffing with the last set. On start and after recovery, read
   from the stored cursor without waiting for a signal.
3. Attribution: every session gets `BD_ACTOR=<attempt actor>` in its environment, and aae's own calls pass
   `--actor`; `events.actor` then names the run behind every change (correlation, spec §9.5).
4. Guard against the private schema: the exact `br` version is pinned (spec §13) and a contract test reads
   `events` on that version.
