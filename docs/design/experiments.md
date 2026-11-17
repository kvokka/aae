# Experiments

Results of the throwaway experiments that precede the core. Every number carries a `MEASURE` marker. The experiments are
recorded here and their code has left the tree; each section names the commit that still holds it
(`git show <commit>:<path>`). Their logs stay in `experiments/`: sanitized samples (`<home>`, `<work>` placeholders;
long `tools`/`skills` arrays are replaced by their length) kept as the record of what each harness printed; no test
reads them. A new experiment goes to `experiments/` as a separate Cargo workspace outside the product budgets, ≤ 5 000
lines in total, checked by `scripts/budget.sh`.

A cheap experiment comes before the code that relies on it. The set and where each stands:

| # | Experiment | Why | Status |
|---|---|---|---|
| 1 | claude stream-json client: "reply OK", resume, cancel, permission request, prompt acknowledgement; `kill -9` in a command and the fate of every descendant; background subagent and shell, "idle without background" from both signals | no Rust client exists, the protocol is hand-written | done (1a, 1b below) |
| 2 | codex app-server client: transport, two threads in one process, config and hooks per thread, reconnect | the instance registry | done (below) |
| 3 | br as an event source: watching, cost, reliability, attribution by actor | tracker triggers, correlation | done (below) |
| 4 | delivery of hooks, skills and MCP into a session without writing into the rig, with and without host inheritance | config delivery | done (below) |
| 5 | credentials in a session with host inheritance off | config delivery | done for claude and codex API keys; codex subscription login not checked (below) |
| 6 | finisher from a session: token in the environment, resume, the codex sandbox | finisher | done except the sandbox: deferred until after the release, on a host with user namespaces or macOS; until then the codex adapter sets `danger-full-access` ([config](config.md)) |
| 7 | opencode, by the open questions [below](#opencode-experiment-7) | next release | deferred to the next release, with the opencode adapter |
| 8 | background work after the outcome: what stops a command a session left running, without touching other sessions | criterion 34 | done for codex and claude on fake providers (below) |
| 9 | compaction on both harnesses (hooks, thresholds, stopping a session), codex roles and skills per thread, claude `--agents` from a file | the context-budget addon, native subagents | done on fake providers; compaction of a codex multi-agent v2 child not checked (below) |

Not checked by any experiment: a real subscription quota running out and a login expiring in the middle of a session
(both seen only on fake APIs; the claude half of criterion 10 stays open, and with an API key claude gives no reset
time at all); long turns, many subagents, tens of parallel sessions and runs of hours (the experiments ran "reply OK"
turns and at most 4 sessions); a cold start on the role models (measured on haiku); macOS and x86_64 (every
experiment ran on Linux aarch64).

Common setup for the claude experiments: `claude -p --input-format stream-json --output-format stream-json
--verbose --setting-sources "" --strict-mcp-config --model haiku`, clean environment (only `PATH`, `HOME`, `LANG`,
`USER`, `TERM`), `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, subscription login, every
process in its own process group, a fresh empty work directory.

## claude protocol (experiment 1a)

Code: `experiments/01-claude-protocol` (`src/lib.rs` is the hand-written client, `src/main.rs` the scenarios
`basic`, `resume`, `permission`, `interrupt`), removed from the tree after recording; last in commit `3f59356`.
Logs: `experiments/01-claude-protocol/logs/`.

<!-- MEASURE id=claude-protocol-1a date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/01-claude-protocol/logs note="haiku, subscription login, n=1 per scenario" -->

| Question | Answer |
|---|---|
| "reply OK" round trip | Works. A turn takes 0.9–1.0 s from writing the prompt to `result`. `system/init` repeats on every turn with the same `session_id` as `--session-id`. |
| Output before the first prompt | Nothing in 3 s. `init` comes only after a prompt is written, so the driver never waits for `init` before the prompt. |
| Native prompt acknowledgement | Yes: `{"type":"command_lifecycle","command_uuid":<uuid of our user message>,"state":"queued"\|"started"\|"completed"}`. `queued` arrives 6–9 ms after the write and `started` within 15 ms, `completed` right after `result`. Emitted with and without `--replay-user-messages`; `init.capabilities` lists `msg_lifecycle_v1`. The `isReplay` echo of `--replay-user-messages` is not an acknowledgement: it arrives 0.9 s later, just before the reply. |
| Idempotency key | The `uuid` field of the user message we write becomes the `uuid` of the user record in the transcript (`<config>/projects/<dir>/<session>.jsonl`). Writing a message whose `uuid` is already in the session yields `command_lifecycle{state:"completed"}` at once and no turn. So the key is the message `uuid` (derive it deterministically from the attempt key); after a daemon crash the transcript is searched for it. Limit: only a *finished* turn was observed. If the daemon dies after `queued` and the turn is cut, the user record is already in the transcript without an answer; a resend with the same `uuid` will most likely also complete silently and the attempt would hang. So after a restart the adapter checks the transcript for an answer after our `uuid` and, if there is none, sends a continuation with a new `uuid` ([contracts 6.4](contracts.md#64-prompt-delivery-and-recovery)). |
| `--resume` with stream-json input | Works: the context survives (the resumed session recalled a word from the first one), `session_id` is unchanged, `init` after the prompt as usual. |
| Model after resume | Not reset. Without `--model`, a fresh session started on the account default (`claude-opus-5-5`), a resumed haiku session started on `claude-haiku-5-5`. The adapter still passes the model explicitly. (The no-`--model` runs used an unreachable `ANTHROPIC_BASE_URL`, so no non-haiku request was made; `init` already names the model.) |
| Permission request | `--permission-prompt-tool stdio`: a `session_state_changed` to the waiting state and `control_request{subtype:"can_use_tool"}` arrive together; the answer is a `control_response` (shape below), then `{running}`. The adapter does not use this: it runs claude with `--permission-prompts none` (contracts 4.6). A deny with a message reaches the model as `tool_result{is_error:true, content:<message>}` with `tool_result_meta[].non_execution_kind:"permission-rule"`; the model went on with the next step. |
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

Consequences for the claude adapter:

- "Prompt accepted" = `command_lifecycle{queued}` with our `command_uuid`; a timeout without it is an adapter
  failure, not a turn.
- An interrupted turn ends with `is_error:true` and `terminal_reason:"aborted_tools"` (or another
  `error_during_execution`); after our own `interrupt` it maps to `turn.ended{stop_reason: cancelled}`, never to a
  failure.
- Unknown stream types (`command_lifecycle` was one, undocumented) are ignored, not fatal.

## claude processes and background work (experiment 1b)

Code: `experiments/02-claude-process` (`orphans`, `orphans subreaper`, `background`), removed from the tree after
recording; last in commit `1629c3a`. Logs:
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
2. A per-attempt marker variable in the session environment; on cleanup the daemon kills every process carrying the
   marker (`/proc/<pid>/environ`; macOS is out of scope for the first release). The same sweep runs during
   recovery after a daemon crash. Known macOS differences until it is checked: no sweep, no subreaper, and a process is
   told by its pid alone (no start time), so a reused pid passes for the old process.
3. Linux: the daemon is a child subreaper, so descendants that dropped the marker still become its children and are
   reaped. The daemon must reap only pids it does not manage itself, or it steals exit statuses of its own session
   processes: it takes the zombies among its children from `/proc` and skips those it spawned. A peek with
   `waitid(P_ALL, WNOWAIT)` does not do: it returns the same zombie again and again, and while that one is the
   daemon's own child the orphans behind it are not seen.

cgroups are not needed for the first release. The remaining gap: a descendant that clears its environment and is
orphaned while the daemon itself is down (it goes to PID 1, not to the restarted daemon).

<!-- MEASURE id=claude-bash-own-session date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/02-claude-process/logs/orphans.ndjson note="n=2 (with and without subreaper); 3 descendants survived kill(-pgid) each time" -->

### Heartbeat of a long tool

`tool_progress{heartbeat:true}` at 30.0 s and 60.0 s after the `tool_use` of a 100 s foreground Bash command —
a 30 s interval.

<!-- MEASURE id=claude-tool-heartbeat date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/46a note="tool_progress in stream-json; the stall watchdog threshold depends on the interval" -->
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
`task_notification` arrived after the last `result`. Condition (c) closes the wake-up window seen at 22.10. A
`task_notification{status:"stopped"}` does not count for (c): a task stopped by `stop_task` wakes no turn
(experiment 8).

<!-- MEASURE id=claude-idle-two-signals date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/02-claude-process/logs/background.ndjson note="n=1; state idle with a live background shell, 0.1 s idle+empty window before a self-started turn" -->

### A background subagent after the result

claude 2.1.326 against a fake Messages API (no model): the main session calls `Agent` with `run_in_background: true`,
the subagent runs a foreground `Bash` of 20 s (75 s in a third run), then answers. Timeline of one run (s from spawn):

| t | event |
|---|---|
| 0.28 | `background_tasks_changed{[local_agent]}`, `task_started{local_agent, is_backgrounded}` |
| 0.31 | the subagent's `assistant` (`parent_tool_use_id` of the `Agent` call) with its `tool_use` |
| 0.34 | `result` of the prompt; no `session_state_changed`, the state stays `running` |
| 3.34 | `task_started{local_bash, owned_by_subagent}` (never in `background_tasks_changed`) |
| 3.34–20.36 | nothing at all: no `tool_progress`, no `stream_event` |
| 20.36–20.42 | the command's `task_notification`, the subagent's `user` and `assistant`, the agent's `task_notification`, `background_tasks_changed{[]}` |
| 20.43–20.44 | self-started turn (`init` … `result{origin.kind: "task-notification"}`), then `session_state_changed{idle}` |

Findings: after the `result` the session is silent for as long as the subagent's command runs, and the state does not
leave `running` until the self-started turn ends. So the turn-end rule alone keeps the run under the plain `silence`
budget; the adapter reports idle with background right after a `result` with a live `local_agent`, and the
subagent's steps out of a turn are no activity.

<!-- MEASURE id=claude-background-subagent date=2026-11-17 ver=claude-2.1.326 env=linux-arm64-orbstack src=.loop/cycles/112/notes/wp99-stand.md note="fake provider, no model; n=2 with a 20 s command, n=1 with 75 s" -->

## br as an event source (experiment 3)

Code: `experiments/05-br-events` (one scenario, fresh workspace with an isolated `HOME`, the `notify` crate on the
`.beads/` directory, `rusqlite` read-only on `beads.db`), removed from the tree after recording; last in commit
`1629c3a`. Output: `experiments/05-br-events/logs/run.txt`.

<!-- MEASURE id=br-events-exp3 date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="20 creates, 9 reads, 16 concurrent claims, 4 attribution cases; database of 20 beads; two runs agree" -->

| Question | Answer |
|---|---|
| Signal | Every mutating command rewrites `issues.jsonl` by rename exactly once; `notify` reports it as two events, `Modify(Name(To))` and `Modify(Name(Both))`. 20/20 sequential creates and 16/16 concurrent claims gave one rename each — no loss, no extra. Reads (`list`, `show`, `ready`) gave none; the database files see open/close/write traffic on reads too, so they are not a signal. |
| Watcher cost | 110 ms of CPU for the whole watching process over steps 1–4 (20 creates, 9 reads, 16 concurrent claims, 4 attribution cases): `notify` plus a read-only `events` query per signal. |
| Latency | The rename lands 23–62 ms after the command starts (median 41 ms) and always before the command exits. |
| Change content | The `events` table, read after the cursor (`WHERE id > ?`), with the database opened read-only per signal and closed at once: 0.6–0.7 ms per read including open/close. It held exactly one row per create and two per claim (`status_changed` + `assignee_changed`), 32/32 rows attributed to the right claimer under concurrency. A full snapshot (`br list --all --json`) costs 8–9 ms at 20 beads and grows linearly (60 ms at 5 000 beads in an earlier measurement) and carries no actor. |
| Attribution | `--actor X` and the environment variable `BD_ACTOR=X` both land in `events.actor`. `BR_ACTOR` does nothing; `BR_AGENT_NAME`, `BR_HARNESS`, `BR_MODEL` are **not** recorded in 0.7.6 (columns stay empty) despite the help text. |

<!-- MEASURE id=br-rename-latency date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="n=20 sequential creates, 20-bead database; grows with database size (auto-flush rewrites the whole file)" -->
<!-- MEASURE id=br-watcher-cpu date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="getrusage of the experiment process over steps 1–4, n=1; includes the scenario driver itself, so an upper bound" -->
<!-- MEASURE id=br-events-read-cost date=2026-11-12 ver=br-0.7.6 env=linux-arm64-orbstack src=experiments/05-br-events/logs/run.txt note="rusqlite bundled, open read-only + query + close, mean of 100, ~60 rows" -->

**Chosen method** for `bead.*`:

1. Watch the `.beads/` directory (not the file: the rename replaces the inode). Any rename of `issues.jsonl` means
   "something changed"; signals are coalesced, and a missed or duplicate signal costs nothing because of step 2.
2. On a signal, open `beads.db` read-only, read `events` after the stored cursor, close. Map rows to `bead.*` events;
   `bead.ready` is derived by re-reading `br ready` and diffing with the last set. On start and after recovery, read
   from the stored cursor without waiting for a signal.
3. Attribution: every session gets `BD_ACTOR=<attempt actor>` in its environment, and aae's own calls pass
   `--actor`; `events.actor` then names the run behind every change (correlation, [contracts 5.1](contracts.md#51-operations)).
4. Guard against the private schema: the exact `br` version is pinned ([tracker-br](tracker-br.md#versions)) and a contract test reads
   `events` on that version.

## codex app-server (experiment 2)

Code: `experiments/03-codex-appserver` (`src/lib.rs`: client over stdio or a WebSocket on a unix socket and a fake
Responses API provider, kept for experiments 4–5; `src/main.rs`: scenarios `threads stdio|unix` and `reconnect`,
removed from the tree after recording, last in commit `1629c3a`; `src/bin/gen.rs`: type generator; the rest
last in commit `3f59356`). Output:
`experiments/03-codex-appserver/logs/run.txt`, message logs next to it. No live model: every
turn goes to the fake provider, `HOME` and `CODEX_HOME` are fresh directories, the process environment holds only
`PATH`, `HOME`, `CODEX_HOME`, `SHELL`.

<!-- MEASURE id=codex-appserver-exp2 date=2026-11-12 ver=codex-0.167.2 env=linux-arm64-orbstack src=experiments/03-codex-appserver/logs/run.txt note="fake provider, n=1 per scenario and transport in the recorded run; 2–3 earlier development runs agreed" -->

| Question | Answer |
|---|---|
| Transport | Both work and carry the same messages (compare `threads-stdio.ndjson` and `threads-unix.ndjson`). **Chosen: stdio.** It needs no WebSocket code (handshake, masking, ping) on a transport the vendor calls experimental and unsupported, and no socket directory with `0700`. The one thing only the socket gives — the server outliving its client — is not needed in the first release (no reconnection after a daemon restart, [invariants](invariants.md#recovery)); with stdio the server ends with the daemon (below), so no stale app-server is left holding thread writer locks. |
| Two threads in one process | Two `turn/start` sent back to back on two threads both completed (72–119 ms with the fake provider). Each provider request carried only its own thread's `model` and `developerInstructions`. |
| Config per thread | `cwd`, `model`, `developerInstructions` and `config` (`shell_environment_policy.set`, `hooks`, `bypass_hook_trust`) given in `thread/start` apply to that thread only. |
| Hooks per thread | **Work.** `thread/start.config.hooks` takes the same value as the `hooks` key of `hooks.json`; the hooks fire with `run.source: "sessionFlags"`, and threads of one process run their own sets: A (`UserPromptSubmit` + `Stop`) ran both, B (`UserPromptSubmit` only) ran just that one, and neither set fired on the other thread's turn. Hook stdin carries `session_id` = thread id. Two conditions: `config.bypass_hook_trust: true` is required — thread C got the full set without it and ran no hook (no `hook/*` notification, no warning), while thread D with the same set and the bypass ran both; with it every `thread/start` emits a `configWarning` and a `warning` "`--dangerously-bypass-hook-trust` is enabled", which the adapter ignores. **Hooks run with the process environment**: `shell_environment_policy.set` of the thread is not visible to them. So hooks are a per-thread part of the config, the environment of hooks is a per-process part; per-thread values for a hook go into its command line (it is per thread anyway) or are looked up by `session_id`. |
| Reconnect after a client restart (unix socket) | The server and the running turn survive the disconnect. The new client sees the thread in `thread/loaded/list`; `thread/resume` returns it as `active`; the second client then gets `turn/completed{completed}` of the turn the first one started, and its own next turn works. |
| Client end on stdio | Closing stdin in the middle of a turn (the provider holds the answer for 6 s) makes app-server exit with status 0 within 21 ms; the turn is lost. |
| Idempotency key | `clientUserMessageId` is **not** deduplicated: the same value sent again on an idle thread started a new turn and a new provider request, and the history (`thread/read{includeTurns:true}`) then held two `userMessage` items with that `clientId`. So the key is the `clientUserMessageId` (derived from the attempt key) and the check is the adapter's: after a restart it reads the thread history for its `clientId` before sending; a turn with an answer after it means "delivered", an `interrupted` one means "send a continuation with a new key" — the same rule as for claude above. |

<!-- MEASURE id=codex-appserver-rss date=2026-11-12 ver=codex-0.167.2 env=linux-arm64-orbstack src=experiments/03-codex-appserver/logs/run.txt note="VmRSS of app-server, fake provider, one short turn per thread, stdio and unix runs; SHELL=/bin/sh and an empty HOME, so no rc files" -->
<!-- MEASURE id=codex-appserver-initialize date=2026-11-12 ver=codex-0.167.2 env=linux-arm64-orbstack src=experiments/03-codex-appserver/logs/run.txt note="from before spawn to the initialize response, 150–307 ms over 10 runs of both transports, empty CODEX_HOME" -->
<!-- MEASURE id=codex-stdio-eof-exit date=2026-11-12 ver=codex-0.167.2 env=linux-arm64-orbstack src=experiments/03-codex-appserver/logs/run.txt note="n=1, stdin closed 1 s into a turn whose provider answer was delayed by 6 s" -->

Memory on 0.167.2: 89–91 MB empty, 147–150 MB with two threads after one turn each, 155–159 MB with four — the
same as on 0.166.0: 93 MB empty, +39–62 MB for the first thread, ≈ 5–10 MB for each next one.

<!-- MEASURE id=codex-appserver-rss date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/15 note="RSS, up to 4 threads, turns reply OK; not checked on long histories: a thread's history lives in the process memory" -->

`initialize` answered 150–307 ms after spawn on both transports (77 ms on 0.166.0 in an earlier
measurement with another setup).

**Types.** `codex app-server generate-json-schema --out DIR` writes the bundle;
`cargo run -p codex-appserver --bin gen -- DIR/codex_app_server_protocol.v2.schemas.json` takes the hand-picked roots
listed in `gen.rs`, collects the definitions they reach and generates `src/generated/protocol.rs` with `typify`
(9 roots → 83 definitions → 5.1k lines, excluded from the line budgets). The four types the scenarios parse
(`ThreadStartResponse`, `ThreadResumeResponse`, `TurnCompletedNotification`, `HookCompletedNotification`)
deserialized every message received. `InitializeResponse` is only in the non-v2 bundle
(`codex_app_server_protocol.schemas.json`); the client reads it as plain JSON. The adapter does not take this
route: generated types are not needed, it reads by hand the few fields it uses, and the contract suite
([harness updates](../harness-updates.md)) catches a changed field on every codex update. The generator is kept only
in the commit named above.

**Fake provider** (the sketch of `crates/aae-testkit/src/openai.rs`): an HTTP server on `127.0.0.1` given to codex as
`[model_providers.fake] base_url = "http://…/v1", wire_api = "responses"` in `CODEX_HOME/config.toml`. A turn is
answered with `content-length` SSE of three events — `response.created`, `response.output_item.done` with a
`message` item of one `output_text`, `response.completed` with `usage` — and `connection: close`. Unknown model
names only cause a `warning` "Model metadata for `<model>` not found".

Consequences for the codex adapter and the instance registry:

- One app-server per fingerprint over stdio, spawned and owned by the daemon; the hook set is not part of the
  fingerprint, the process environment is.
- Every session-level hook needs `bypass_hook_trust: true` on its thread; per-session data reaches hooks through
  the command line or `session_id`, never through `shell_environment_policy`.
- A daemon restart ends the process and all its turns; recovery is `thread/resume` in a new process plus the
  history check of the idempotency key.

## Finisher from a session (experiment 6, partly)

Code: `experiments/06-finisher` (`src/bin/finish.rs` stands in for the finisher CLI: it connects to the unix socket
named in `AAE_SOCKET` and reports `AAE_ATTEMPT_TOKEN`, `AAE_PROC_TOKEN` and its arguments; `src/main.rs` plays the
daemon side; last in commit `3f59356`). codex runs on the fake provider of experiment 2, which answers `RUN:<cmd>`
with an `exec_command` call; claude runs on haiku with `--permission-mode bypassPermissions`. Output:
`experiments/06-finisher/logs/`.

<!-- MEASURE id=finisher-exp6 date=2026-11-12 ver=claude-2.1.322,codex-0.167.2 env=linux-arm64-orbstack src=experiments/06-finisher/logs/run.txt note="n=1 per round; codex without a live model, claude haiku with a subscription login; codex sandbox not exercised (see below)" -->

| Question | Answer |
|---|---|
| Token in the environment, claude | The Bash tool sees the environment of the `claude` process: the finisher reached the socket with the token of that process. After `--resume` in a new process with another token, the finisher reported the new one — the token belongs to the process, not to the session. |
| Token in the environment, codex | A per-thread `config.shell_environment_policy.set` value reaches the command, and so does a process variable (both have `TOKEN` in the name — the default excludes did not drop them). |
| codex after resume in a new process | **The per-thread config is not restored**: `thread/resume` with only `threadId` ran the command without the per-thread token (the process variable was there). `thread/resume` with the same `config` restored it, with the new value. So the adapter passes the full thread config on every resume (an open point until then). |
| codex after resume in the same process | **A thread app-server still holds keeps its config**: after `thread/unsubscribe` a thread whose last turn failed (status `systemError`) stayed loaded, `thread/resume` rejoined it, and its command ran with the token of the first attempt (an idle thread was reloaded by the same resume and got the new one). With `-c thread_unload_delay_secs=0` the last unsubscribe closes the thread within milliseconds (`thread/closed`; a resume in between is refused: "is closing; retry thread/resume after the thread is closed"), and a thread with a turn in flight stays loaded. So the adapter starts app-server with that flag and resumes only once `thread/unsubscribe` answers `notLoaded` (codex 0.168.0). |
| codex sandbox | **Not checked.** The sandbox needs unprivileged user namespaces (bubblewrap); the owner's host container forbids them (`unshare -U` → "Operation not permitted"), so every codex command here ran in `danger-full-access`. Whether a `workspace-write` command can connect to the daemon socket and write the tracker database outside the work directory stays open. |

Consequences: the attempt token goes into the process environment for claude (a process per session anyway) and
into `shell_environment_policy.set` of the thread for codex, re-sent with every `thread/resume`. Hooks do not see
the per-thread value (experiment 2), so a hook that must call the finisher gets the token on its command line.

## Config and credential delivery (experiments 4 and 5)

Code: `experiments/04-config-delivery` (`src/bin/claude.rs`: scenarios `flags`, `sources`, `auth`;
`src/bin/codex.rs`: scenarios `mcp`, `rig`, `process`, and `codex mcp TAG ROOT` — a stdio MCP server with one tool
`tag_<TAG>` that records its pid, used by both harnesses; last in commit `3f59356`). Output: `logs/claude/run.txt`,
`logs/codex/run.txt`, message
logs next to them. claude: haiku, clean environment as above, a fresh rig per run; the run that inherits
the host keeps no log (it reads the operator's own config), only counts. codex: the fake provider of experiment 2,
fresh `HOME` and `CODEX_HOME`, no live model; the "host" is an imitation `CODEX_HOME`, the real `~/.codex` is not
touched.

<!-- MEASURE id=config-delivery-claude date=2026-11-12 ver=claude-2.1.322 env=linux-arm64-orbstack src=experiments/04-config-delivery/logs/claude/run.txt note="haiku, n=1 per case, 9 sessions; host counts depend on the operator's config" -->
<!-- MEASURE id=config-delivery-codex date=2026-11-12 ver=codex-0.167.2 env=linux-arm64-orbstack src=experiments/04-config-delivery/logs/codex/run.txt note="fake provider, n=1 per scenario" -->

### claude

| Channel | Works? | Evidence and notes |
|---|---|---|
| Hooks: `--settings <file outside the rig>` | yes | `UserPromptSubmit` and `Stop` fired once each |
| Hooks: `--settings '<inline JSON>'` | yes | the same, no file at all |
| Hooks: `hooks/hooks.json` inside a `--plugin-dir` plugin | yes | fired next to the `--settings` ones |
| Skills: `--plugin-dir` (`.claude-plugin/plugin.json` + `skills/<name>/SKILL.md`) | yes | `init.skills` holds `aae-p:p-skill` |
| MCP: `--mcp-config <file>` and `--mcp-config '<inline JSON>'` (twice in one run) + `--strict-mcp-config` | yes | both `connected` in `init.mcp_servers` |
| Flags on `--resume` | **not kept** | resume without the flags: same session, but no plugin skill, no MCP, no hook fired. The adapter passes the full flag set on every resume |
| `--setting-sources ""` | cuts host and rig | no rig hook, skill, `CLAUDE.md` or `.mcp.json`; 19 built-in skills and 4 plugins stay |
| `--setting-sources project` (and `project,local`) | **rig works, host cut** | rig hook fired, `rig-skill` listed, the model quoted the `CLAUDE.md` marker, rig `.mcp.json` server `connected` (`enableAllProjectMcpServers` in the rig settings); host skills and MCP absent (19 skills, 0 other MCP — as with `""`) |
| No source limit (`user,project,local`, host inherited) | rig works, host added | +5 skills, +1 MCP from the host |
| Own empty `CLAUDE_CONFIG_DIR`, nothing delivered | refused | result `is_error: true`, "Not logged in · Please run /login", 290 ms after spawn |
| Own `CLAUDE_CONFIG_DIR` + `CLAUDE_CODE_OAUTH_TOKEN` (subscription access token of the host) | yes | turn completed, `apiKeySource: "none"` |

The user-level `~/.claude/CLAUDE.md` is dropped by `--setting-sources project,local` with an own
`CLAUDE_CONFIG_DIR`, but claude also reads `CLAUDE.md` and `.claude/CLAUDE.md` up the ancestors of its working
directory, so a rig under `HOME` still reached the host's file (acceptance scenario 12, claude 2.1.322). Without the
host layer the adapter therefore sets `claudeMdExcludes` to `~/.claude/CLAUDE.md` and `~/.claude/rules/**` in the
`--settings` file; scenario 12 checks that the isolated session no longer sees it. `~` here is the `HOME` of the
daemon process: a daemon run with another `HOME` than the user whose rigs it serves (a system service, for one) does
not exclude that user's file; run the daemon as that user, as the service files do ([install](../guide/install.md#run-the-daemon)).
Open: refresh of a delivered OAuth token after expiry ([config](config.md#host-inheritance)).

### codex

| Channel | Scope | Works? | Evidence and notes |
|---|---|---|---|
| stdio MCP: `thread/start.config.mcp_servers.<n> = {command, args}` | thread | yes | each of two threads saw only its own tool in the provider request; one server process per thread (plus one short-lived start that exited at once); `mcpServerStatus/list{threadId}` names the thread's server |
| HTTP MCP: `config.mcp_servers.<n> = {url, http_headers}` | thread | yes | codex POSTed `/mcp` with the custom header (`x-aae-agent`) and `user-agent: codex-mcp-client/0.167.2` |
| MCP server lifetime | — | — | `thread/unsubscribe` does not stop the thread's server; when app-server exits on stdin EOF, every server it started is gone (no orphans) |
| Skills: `$CODEX_HOME/skills/<name>/SKILL.md` | process | yes | scope `user` in `skills/list`, named in the provider request |
| Skills: `skills/extraRoots/set` | process | yes | an extra root outside rig and homes; scope `user` |
| Skills: `$HOME/.agents/skills` with a separate `CODEX_HOME` | process | **read** | a separate `CODEX_HOME` does not cut the host's `~/.agents/skills`; only another `HOME` does |
| Skills per thread | thread | enable and disable only | `thread/start.config.skills.config` turns skills of the process roots on and off for the thread ([experiment 9](#codex-roles-and-skills-per-thread)); a skill outside the roots comes per thread only via the rig (`.agents/skills` in `cwd`) |
| Rig: `.codex/config.toml`, `AGENTS.md`, `.agents/skills` in `cwd` | thread | yes | all three reached the provider request with a fresh `CODEX_HOME`, with and without a `trust_level = "trusted"` entry for the `cwd` |
| Hooks: `-c hooks.<Event>=[…]` at spawn | process | yes, with the bypass | fired with `source: sessionFlags` |
| Hooks: `hooks.json` in `CODEX_HOME` | process | yes, with the bypass | fired with `source: user` |
| `bypass_hook_trust` | thread | thread only | set in `config.toml` of `CODEX_HOME` it has no effect (no hook ran); set in `thread/start.config` it lets process-level and user hooks run too |
| Host config (`config.toml` of `CODEX_HOME`: instructions, MCP) | process | yes | marker and the host MCP tool reached the thread; the MCP server started per thread |
| Provider key: `env_key` of the provider, value in the app-server environment | process | yes | `authorization: bearer <key>` at the provider |
| Provider key per thread | thread | name only | `config.model_providers.<p>.env_key` in `thread/start` is applied (a missing variable fails the turn: "Missing environment variable"), but the value is still read from the process environment; `experimental_bearer_token` in the thread config was ignored |
| ChatGPT subscription login in a separate `CODEX_HOME` | process | not checked | the host has no codex login ([config](config.md#host-inheritance) keeps it open) |

### Consequences

- claude: the adapter delivers everything with flags and files in the state directory: `--settings` (file or
  inline), `--plugin-dir` for skills and plugin hooks, `--mcp-config`; and repeats the whole set on every `--resume`. It
  does not pass `--strict-mcp-config`: that would cut the rig's `.mcp.json` too, and with inheritance off the setting
  sources and the own config directory already cut the host's servers. With inheritance on, the host's personal MCP
  servers reach the session as well, as [config](config.md#host-inheritance) intends. Host inheritance off = `--setting-sources project,local` (not
  `""`): the rig's own config keeps working — the rig-config rule of `config.md` holds for claude. With an isolated
  `CLAUDE_CONFIG_DIR` the adapter delivers `CLAUDE_CODE_OAUTH_TOKEN` (or `ANTHROPIC_API_KEY`).
- codex: MCP, hooks (`config.hooks`), instructions, `bypass_hook_trust` and which skills are on go per thread; the
  skill roots, the provider key value and the host layer are per process. The instance fingerprint
  ([contracts](contracts.md#8-instance-and-service-registry)) therefore holds
  `CODEX_HOME`, `HOME`, the skill roots and the provider and its key; MCP and hooks are not in it. Host
  inheritance off = own `CODEX_HOME` **and** own `HOME` (or the host's `~/.agents/skills` leak in); the rig's
  `.codex/`, `AGENTS.md` and `.agents/skills` keep working — the rig-config rule of `config.md` holds for codex too.
  Without a key of its own, an isolated process logs in as the host does: the host `auth.json` is linked into its
  `CODEX_HOME`, and a model provider the host config selects comes as the first `-c` flags with its `env_key`
  variable passed from the daemon's environment (`aae-codex/src/part.rs::host_provider`).

## Background work after the outcome (experiment 8, criterion 34)

Code: two throwaway Python scripts outside the product tree (`.loop/cycles/98/notes/wp83_codex.py`,
`wp83_claude.py`, process helpers in `wp83_proc.py`), each with its own fake provider: codex gets a Responses API that
answers a `RUN:<cmd>` prompt with one `exec_command` call (`yield_time_ms: 1000`) and then the text "Started.";
claude gets a Messages API that answers it with one `Bash` call (`run_in_background: true` unless noted) and then
the text. No live model. The left command is `python3 -m http.server --bind 127.0.0.1 0`, the case seen in demo 2.
Every session carries `AAE_ATTEMPT_TOKEN` as the adapters set it (codex: `shell_environment_policy.set` of the thread;
claude: the process environment); a second session (codex: thread B of the same app-server running `sleep 300`;
claude: a decoy `sleep 300` with another token) must survive every mechanism. After each case the script lists every
process carrying the markers from `/proc`.

<!-- MEASURE id=bg-after-outcome-exp8 date=2026-11-16 ver=claude-2.1.325,codex-0.168.0 env=linux-arm64-orbstack src=.loop/cycles/98/notes/wp83-experiment.md note="fake providers, no model; n=2 for codex clean/unsubscribe and claude stop_task/SIGTERM, n=1 for the other cases" -->

### codex

The turn ends (`turn/completed{completed}`) while the command still runs; `thread/backgroundTerminals/list`
(experimental API, the adapter already initializes with `experimentalApi: true`) shows it with its `processId`
(`osPid` is still `null` in 0.168.0). app-server runs every command in a **new session** (`sid = pgid = pid` of the
command's shell, parent app-server); a shell that stays (`cmd; true`) keeps its children in that session.

| Mechanism, on thread A after its turn | A's command | thread B's command | What the client sees |
|---|---|---|---|
| `thread/backgroundTerminals/clean{threadId}` | gone, with the shell's children | alive | `{}`, then `item/completed` of the command within 10 ms |
| `thread/backgroundTerminals/terminate{threadId, processId}` | gone | alive | `{terminated: true}`, then `item/completed` |
| `thread/unsubscribe` (with `thread_unload_delay_secs=0`) | gone | alive | `unsubscribed`, `thread/status/changed{notLoaded}`, `thread/closed`; **no** `item/completed` |
| `turn/interrupt` on the ended turn | alive | alive | error `-32600` "no active turn to interrupt" |
| EOF on stdin (app-server exits) | gone | gone | exit code 0 |
| kill of every process with A's token (outside codex) | gone | alive | `item/completed` of the command, as if it ended |

A command that detaches itself (`setsid … >/dev/null 2>&1 &`) ends its `exec_command` at once: the turn has no
background, `backgroundTerminals/list` is empty, `clean` does nothing, and the server lives on re-parented to PID 1 (to
the daemon, the child subreaper, in the product). It still carries A's token, and the token sweep finds and kills it.
A plain `cmd &` with its output on the command's terminal died with the terminal (nothing was left).

### claude

The turn ends with `result` and `session_state_changed{idle}` while the background task runs (as in experiment 1b).
The Bash tool's shell runs in a new session; the server is its child in that session.

| Mechanism, after the turn | The task's processes | claude | What the stream shows |
|---|---|---|---|
| `control_request{subtype:"stop_task", task_id}` | gone, shell and child | alive, next prompt possible | `task_updated{killed}`, `task_notification{status:"stopped"}`, `background_tasks_changed{tasks:[]}`, `control_response{success}`, `idle`; **no** self-started turn in 4 s |
| `control_request{subtype:"interrupt"}` | alive | alive | `control_response{still_queued:[]}`, `idle` |
| `control_request{subtype:"end_session"}` | gone | exits 0 | `task_notification{stopped}`, empty task list, EOF; the session cannot take a next prompt |
| EOF on stdin | alive | **keeps running** while the task lives | nothing; claude waits for its background task |
| SIGTERM to claude | gone | exits 143 | `task_notification{stopped}`, empty task list, EOF |
| kill of the task's processes (outside claude) | gone | alive | `task_notification{status:"failed"}` and a **self-started turn** (`result`, a model request) |

`task_id` comes from `task_started` or `background_tasks_changed`. A `setsid … &` started by a foreground Bash
escapes as with codex (PID 1 here, the daemon in the product) and is found only by the token sweep. The decoy of the
other token survived every case.

### Recommended mechanism

Once the outcome is declared and the turn has ended, the adapter stops what the session left, in two steps:

1. **Through the harness**, so it accounts for the stop: codex — `thread/backgroundTerminals/clean{threadId}` (one
   call per thread; only that thread's commands go; the command's `item/completed` follows and the stream already
   turns it into "idle without background"); claude — `stop_task` for every task of the last
   `background_tasks_changed`. Neither ends the session, so a veto remark ([contracts 6.3](contracts.md#63-finisher)) can still go to it as the next prompt.
   Not `unsubscribe` or `end_session`: they close the session, and `unsubscribe` gives no `item/completed`.
2. **Then the token sweep**: kill every process whose `/proc/<pid>/environ` holds this attempt's
   `AAE_ATTEMPT_TOKEN` and whose session id differs from the harness process's (app-server, or claude itself, which
   carries the token too). This catches commands that detached themselves; other sessions are never touched, the
   token is per attempt.

Limits and things to get right:

- claude: a `task_notification` after the last `result` keeps the session in "background" by rule (c) of experiment 1b
  (`crates/aae-claude/src/stream.rs`, `notified_since_result`). After the adapter's own `stop_task` no turn follows,
  so a `task_notification{status:"stopped"}` of a task the adapter stopped must not count as a wake-up, or the
  session waits for the background budget anyway. A task killed from outside (`failed`) does wake claude into a
  model turn — another reason to stop tasks through `stop_task`, not by signals.
- `thread/backgroundTerminals/*` is experimental in codex; the contract suite has to cover it on every update. If it
  goes away, `unsubscribe` is the fallback (the session then needs a resume for a next prompt).
- The sweep relies on `/proc` (Linux only, like the orphan sweep) and on the command keeping its environment; a
  process that clears it is caught only by the subreaper as an orphan, not stopped.
- Not checked: a background subagent of claude (`local_agent`) under `stop_task`; whether codex per-thread MCP
  servers carry the thread's token (the session-id filter of step 2 is meant to spare them, since they are not
  started with `setsid`); macOS.

What the product does (contracts 4.1, 4.6, 6.3): step 1 as recommended, through the driver call `stop_background`,
which the core makes once the outcome is declared and the turn has ended with background, from the idle event or from
a finisher that comes after the turn. claude stops only `local_bash` tasks: a background subagent is never stopped
(criterion 4a), and while one lives the state stays `running`, so the call does not come at all. Rule (c) of
experiment 1b skips a `stopped` notification. Step 2 comes when the session ends, not with the stop: a command that
detached itself holds no step, so the run does not wait for it. With claude it goes with the session's process on
the orphan sweep by `AAE_INSTANCE`; with codex `close_session` kills every process with the thread's attempt token
(the app-server does not carry one, the other threads have tokens of their own).

## Compaction and native subagents (experiment 9)

Code: throwaway Python scripts in a temporary directory, not kept: a fake Messages API and a fake Responses API in the
shape of `aae-testkit`, except that the `usage.input_tokens` a reply reports is set per run (the testkit fakes always
report 10 tokens, too few for a compaction); a hook script that logs its stdin and prints a prepared answer; an
app-server client. claude 2.1.326 runs as the adapter starts it (`-p`, stream-json both ways, `--setting-sources
project,local`, hooks in `--settings <file>`, `--permission-mode bypassPermissions`, model `claude-haiku-5-5`, whose
window is 1 000 000 by `modelUsage.contextWindow`) with the environment of the adapter tests and its own
`CLAUDE_CONFIG_DIR`; codex 0.169.0 as in experiment 2, several threads in one app-server, hooks per thread with
`bypass_hook_trust`. No live model: a reply reporting many input tokens makes the harness compact, and the fakes answer
the summary request (claude: `CRITICAL: Respond with TEXT ONLY`, codex: `CONTEXT CHECKPOINT` in the input) with a
fixed summary.

<!-- MEASURE id=compaction-claude-exp9 date=2026-11-18 ver=claude-2.1.326 env=linux-arm64-orbstack src=.loop/cycles/123/notes/wp116.md note="fake Messages API with a set usage, n=1 per case; thresholds bracketed by runs 2000 tokens apart, not bisected further" -->
<!-- MEASURE id=compaction-codex-exp9 date=2026-11-18 ver=codex-0.169.0 env=linux-arm64-orbstack src=.loop/cycles/123/notes/wp116.md note="fake Responses API with a set usage, unknown model names (fallback metadata, multi-agent v1 by default), n=1 per case" -->

### claude compaction

| Question | Answer |
|---|---|
| `PreCompact` and `PostCompact` from `--settings` in `-p` | Both fire, on an automatic compaction (`trigger: "auto"`) and on a `/compact` user message (`"manual"`). `PreCompact` stdin: `session_id`, `prompt_id`, `hook_event_name`, `trigger`, `custom_instructions`; `PostCompact`: the same with `compact_summary`. In a subagent both carry `agent_id` and `agent_type`; in the main thread neither. Next to them fire `SubagentStop` with `agent_type: ""` (the summarizing fork; no `SubagentStart` for it) and `SessionStart` with `source: "compact"`. |
| Attempt or compaction | A compaction needs at least two message groups. Over the threshold in a session's first exchange, `PreCompact` fires, then `system/status{compact_result:"failed", compact_error:"too_few_groups"}`, no `PostCompact`, and the turn goes on; the attempt repeats before the next model request (a subagent fired `PreCompact` three times for one compaction). `PostCompact` counts compactions, `PreCompact` counts attempts. |
| What the stream shows | Main thread: `system/status{status:"compacting"}`, `system/status{compact_result:"success"}`, `system/compact_boundary{compact_metadata:{trigger, pre_tokens, post_tokens, …}}`, a user message "This session is being continued…"; the turn continues and ends with its own `result`. A manual `/compact` ends with a `result` of `num_turns: 0`. A subagent's compaction shows none of these, only the hook messages of `SessionStart:compact`. No `hook_started`/`hook_response` messages come for `PreCompact` or `PostCompact`. |
| Threshold per session | `autoCompactWindow` (tokens, 100000 to 1000000) in `--settings`, at the top level or as `modelSettings.<model>.autoCompactWindow`; the variable `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (the binary says it takes precedence over the setting); `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` (a percent). A window of 100000 compacted at 68000 input tokens and not at 66000 (the window less ≈ 33k); a percent of 10, 5 and 1 on the 1M window compacted at 99000, 50000 and 11000 and not at 97000, 48000 and 9000; with a window of 100000, a percent of 50 compacted at 45000, not at 30000. Without either: no compaction at 190000. A window of 50000 is ignored without a word. |
| Threshold per subagent | `autoCompactWindow` in the agent's `--agents` definition: the subagent compacted at 150000, without it not. A window in the session's `modelSettings` applies to subagents that have none of their own. |
| `--agents` with a file path in `-p` | Works: `--agents /abs/agents.json` and a path relative to the working directory both listed the agent in `init.agents` and ran it. A missing file: exit 1, "Error: --agents file not found: … (a value that is not a JSON object is read as a file path)". |

Stopping from a hook (window 100000, 150000 tokens; a hook answers `{"continue": false, "stopReason": "…"}` or exits 2):

| Hook | Effect | `result` | aae adapter |
|---|---|---|---|
| `PreCompact`, `PostCompact`, `SessionStart` (`compact`): `continue:false` | none: the compaction and the turn go on | `subtype:"success"` | `completed` |
| `PreCompact`: exit 2 | the compaction is skipped (`status:null` without `compact_result`, no `PostCompact`), the turn goes on with the full context | `success` | `completed` |
| `PreToolUse` of the main thread: `continue:false` | the tool still runs, then the turn ends | `subtype:"success"`, `is_error:false`, `terminal_reason:"hook_stopped"`, `stop_reason:"tool_use"`, `result:""`; the `stopReason` text appears nowhere | `completed` |
| `UserPromptSubmit`: `continue:false` | no model request | `system/informational{prevent_continuation:true, prompt_submit_outcome:{kind:"stopped_by_hook", reason}}`, then `subtype:"success"`, `num_turns:0`, `result:"Operation stopped by hook: <reason>"` | `completed` |
| `PreToolUse` in a subagent (stdin with `agent_id`): `continue:false` | only the subagent ends (`task_updated{completed}`); the main thread goes on | `success` | `completed` |

### codex compaction

| Question | Answer |
|---|---|
| Threshold per thread | `thread/start.config.model_auto_compact_token_limit` applies to that thread only: with 50000 the thread compacted after a reply of 150000 tokens, a thread of the same process without it did not. The key in the process `config.toml` applies to every thread; a thread's own value wins over it (900000 on the thread: no compaction). No floor: a limit of 2000 compacted after 3003 tokens, 4000 did not. |
| Where it compacts | In the middle of a turn, after the tool call whose reply crossed the limit: `item/completed{contextCompaction}`, then the turn goes on. `thread/compact/start{threadId}` compacts on request, as a turn of its own. |
| `PreCompact` and `PostCompact` as thread hooks | Both fire from `config.hooks` with the bypass (`source: sessionFlags`). Stdin: `session_id` (the thread id), `turn_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`, `trigger` (`"auto"` or `"manual"`); `PostCompact` has no summary. |
| Compaction of a child thread | A multi-agent v1 child inherits its parent's per-thread limit and hooks: it compacted with the parent's 50000, and its hooks came with `session_id` = the **parent** thread, `agent_id` = the child, `agent_type` = the role. `model_auto_compact_token_limit` in the role's `config_file` is ignored (50000 there with none on the parent: no compaction; 900000 there under a process limit of 50000: the child compacted). A v2 child was not checked. |
| `PreCompact`: `continue:false` | `hook/completed{run:{status:"stopped", entries:[{kind:"stop", text:<stopReason>}]}}`, no compaction, the turn ends `interrupted` with `error: null`; every next turn of the thread ends the same way while it stays over the limit. In a child, only the child's turn ends; the parent goes on. |
| `PostCompact`: `continue:false` | The compaction is done, the turn ends `interrupted`; the next turn runs. |
| `PreCompact`: exit 2 | The hook is `failed`, the compaction happens anyway. |
| Other hooks: `continue:false` | `PreToolUse`: unsupported (`failed`, "PreToolUse hook returned unsupported continue:false"), the command runs. `UserPromptSubmit`: `stopped`, the turn ends `completed` without a model request. |
| aae adapter | A turn `interrupted` that the adapter did not interrupt maps to `cancelled`, without a failure (`crates/aae-codex/src/stream.rs`). |

### codex roles and skills per thread

The per-thread findings of codex 0.168.0 checked again on 0.169.0 (process `config.toml` disabling skills `sk-a` and
`sk-b` of `$CODEX_HOME/skills`; roles given per thread as `config.agents.<role> = {description, config_file}`):

| Question | Answer on 0.169.0 |
|---|---|
| Allow-list of skills per thread | Works: `config.skills.config = [{name: "sk-a", enabled: true}]` gave that thread `sk-a` only, `[{path: <abs>/sk-b/SKILL.md, enabled: true}]` `sk-b` only, a thread without the key none. |
| Roles per thread | Visible to that thread only: its `spawn_agent.agent_type` lists the role and the built-ins (`default`, `explorer`, …); a thread with no role at all (none in the process either) gets no `agent_type` parameter, and a spawn naming the role fails with "unknown agent_type 'r'". |
| Keys of a role file that apply | `model`, `developer_instructions` (they replace the parent's), `model_reasoning_effort` (the child's request had `reasoning.effort: "low"`), `[[skills.config]]` (a skill the role disables is gone in the child). `model_auto_compact_token_limit` does not apply (above). An unknown key such as `bypass_hook_trust` drops the whole role: "Ignoring malformed agent role definition … unknown field `bypass_hook_trust`". |
| `features.multi_agent_v2` per thread | `config.features.multi_agent_v2 = true` gives that thread the `collaboration` tools (`spawn_agent` requires `task_name` and `message`; also `agent_type`, `fork_turns`, `model`, `reasoning_effort`), the others keep `multi_agent_v1`; a per-thread role applied to the v2 child. `features.multi_agent = false` on a thread leaves it without spawn tools. With the fake's unknown model names v1 is the default. |

### codex subagent roles (experiment 10)

Code: throwaway Python scripts in a temporary directory, not kept: a fake Responses API that logs every request and
answers a `CALL <tool> <json>` in the last user message with that tool call, and an app-server client (several
threads in one process, roles per thread as in experiment 9, three skills linked into `CODEX_HOME/skills`).

<!-- MEASURE id=codex-subagent-roles-exp10 date=2026-11-18 ver=codex-0.169.0 env=linux-arm64-orbstack src=.loop/cycles/127/notes/wp123-plan.md note="fake Responses API, unknown model names (fallback metadata), n=1 per case; nesting in v2 and the live models' default version not checked" -->

| Question | Answer |
|---|---|
| Selectors of `skills.config` | `name` is the `name` of the skill's front matter, not the name of its directory or link. `path` is the `SKILL.md` file: the link under `CODEX_HOME/skills`, the link's target and a path through another symlink and `..` all matched; the directory alone did not. |
| Disabling a skill per thread | `config.skills.config = [{path, enabled: false}]` hides it in that thread only, and in its children. |
| `[[skills.config]]` of a role file | `enabled = false` hides the skill in the child. `enabled = true` brings nothing back: not a skill the parent thread disabled, nor one the process disabled (`config.toml` or a `-c` flag), nor one the role of the calling child disabled. A child called by a child saw only what neither role disabled. |
| The role file | Only a file carries a role's settings: `developer_instructions` given in `config.agents.<role>` itself is ignored without a warning, and a role with an empty file or no `config_file` is the parent's copy under another name. An absolute path outside `CODEX_HOME` works. The file is read at `thread/start`, at `thread/resume` and for every child; a missing one drops the role with a warning ("Ignoring malformed agent role definition: agents.<role>.config_file must point to an existing file") and the thread starts. A file in `CODEX_HOME/agents/` is also read as a role of its own and warns "must define a non-empty `name`". `developer_instructions = ""` drops the role ("cannot be blank"). A role name with a dot (`kit.qa`) works. |
| Resume | `thread/resume` with the same `config.agents` gives the roles again; without it a spawn fails with "unknown agent_type". |
| `agents` of the process and of the thread | A thread's `config.agents` object replaces the `agents` table given by `-c` flags (`agents.max_depth`, `agents.max_threads` were lost) and merges with the one of `config.toml`; limits repeated inside the thread's object apply. |
| Multi-agent v1 (an unknown model's default) | Tools in the namespace `multi_agent_v1`; `spawn_agent{agent_type, message}` (it takes `task_name` and `fork_turns` too) returns `{agent_id, nickname}` at once. The child starts without history: its developer message begins with the role's `developer_instructions` in place of the parent's, the task is its user message. A child has no spawn tools unless `agents.max_depth` is over 1. Six children stayed open, the seventh was refused: "agent thread limit reached". |
| Multi-agent v2 (`features.multi_agent_v2 = true` on the thread or as a `-c` flag) | Tools in the namespace `collaboration`; `spawn_agent{task_name, message, agent_type, fork_turns}` returns `{task_name: "/root/<name>"}`. With `fork_turns` unset the child's request is the parent's history, then developer messages: "You are now a newly spawned child agent…", the role's `developer_instructions` alone, the team text, the child's own skill list; with `"none"` it is as in v1. The task is an `agent_message` item whose `encrypted_content` a fake provider gets as plain text. The role's model and effort apply. The child had no `collaboration` tools, with `agents.max_depth = 3` too. `codex features list`: `multi_agent_v2` stable and off by default, `multi_agent` deprecated and on. |
| Notifications of a child thread | They come on the same connection with the child's thread id (`turn/started`, `item/*`, `turn/completed`, `warning`); the end of a child started no turn of the parent. |

### Subagent definitions and nesting

From the owner's research session (claude on a fake provider and on haiku, codex on `gpt-6-luna`; scripts not kept),
not run again here:

<!-- MEASURE id=subagents-nesting-owner date=2026-11-18 ver=claude-2.1.326,codex-0.169.0 env=linux-arm64-orbstack src=.loop/research/2026-11-18-plugins note="owner's session: claude definitions on a fake provider, nesting live on haiku (8 runs) and gpt-6-luna (11 runs); n=1 per case" -->

| Question | Answer |
|---|---|
| claude `--agents` with `--append-system-prompt` | `init.agents` lists the agents; the main thread gets the list as "Available agent types for the Agent tool: …"; its system prompt is the default one plus the appended text. |
| Fields of a claude agent in `-p` | `prompt` (the subagent's system prompt is the SDK base plus it; the parent's `--append-system-prompt` does not reach it), `model`, `effort`, inline `mcpServers` (started for the subagent only), `hooks` (fire only on its calls; its `Stop` comes as `SubagentStop`), `skills` (preloaded whole; names of `--plugin-dir` skills, with and without the plugin prefix), `maxTurns`. Agents from a plugin's `agents/` lose `hooks` and `mcpServers`. |
| claude hooks in subagents | Hooks of `--settings` fire inside subagents with `agent_id` and `agent_type`. |
| Limiting claude agents | `permissions.deny: ["Agent(x)"]` hides the type and blocks the call, inside subagents too; `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1` leaves only the defined agents. A subagent with the `Agent` tool may call any defined type, itself included; it gets no list of types. A skill per agent: `disallowedTools: ["Skill(x)"]` removes the whole Skill tool; a `PreToolUse` hook on `Skill` in the definition denies one. |
| claude resume | `--resume` without the same `--agents` fails with "Agent type 'worker' not found". |
| claude nesting | Depth 2 by default (the second level has no `Agent`); `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` sets it (1, 2, 3, 5 held); the main thread's `Agent` call is asynchronous by default, nested ones synchronous. |
| codex nesting | The live model took multi-agent v2 (`spawn_agent` + `wait_agent`); `agents.max_depth` has no effect in v2; the ceiling is the thread count (`agents.max_threads`: the default stopped at depth 3 with "agent thread limit reached", 15 reached depth 4); roles of the main thread resolve at every depth; the parent waits for the child. |

<!-- MEASURE id=subagents-definition-shape date=2026-11-18 ver=claude-2.1.326 env=linux-arm64-orbstack src=.loop/cycles/126/notes/wp122-plan.md note="one-off fake Messages API, no model, scripts not kept; n=1" -->

| Question | Answer |
|---|---|
| Shape of a claude definition the adapter writes | A type name with a dot (`kit.qa`) or a colon is taken: `init.agents == ["kit.qa"]`, the main thread gets "- kit.qa: <description>", `Agent{subagent_type: "kit.qa"}` runs it with its `prompt`. `mcpServers` is a list of one-server maps (`[{"<name>": {type, command, args, env}}]`); a map fails with "expected array, received object", exit 1. `prompt` is required, an empty string is taken; `description`, `model`, `effort` and `hooks` (the `--settings` shape) together are taken. With `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1` only the defined types are listed; the native `fork` stays. |

### Consequences

For the context-budget addon:

- **Threshold.** claude: `modelSettings.<model>.autoCompactWindow` in the session's `--settings` file (the adapter
  already writes it per session), and `autoCompactWindow` in each agent of `--agents` — or one value for the session
  and its subagents through `modelSettings` alone. Its floor is 100000 and a smaller value is dropped silently, so a
  smaller budget goes through `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` in the process environment (a process per session) as
  a percent of the window. codex: `model_auto_compact_token_limit` in `thread/start.config`, any size; children inherit
  it, a role file cannot change it.
- **Counting.** `PostCompact` counts compactions on both harnesses (`PreCompact` counts attempts, which repeat on
  claude). The stdin names the session (`session_id`; on codex the parent's thread for a child) and, inside a
  subagent or child, `agent_id` and `agent_type`; `trigger` tells automatic from manual. A count on `SubagentStop`
  would also see the summarizing fork of claude (`agent_type: ""`).
- **Stopping past `max_compactions`.** No compaction hook stops a claude session (`continue:false` is ignored there);
  a main-thread `PreToolUse` or `UserPromptSubmit` answering `continue:false` does, and the adapter sees a successful
  `result` (`terminal_reason: "hook_stopped"` for `PreToolUse`), today a completed turn. On codex `continue:false` from
  `PostCompact` (or `PreCompact`) ends the turn `interrupted`, today a cancelled turn; in a child it ends only the
  child. On both, then, the run ends only if aae tells the hook's stop from a normal end: the addon's hook leaves its
  verdict where the core reads it (the run's bead or the finisher), or the adapters map `hook_stopped` and an
  unrequested `interrupted` to a stop of their own.

For codex native subagents: per-thread roles (`config.agents.<role>`) and per-thread skill allow-lists
(`config.skills.config` over skills of the process roots) work on 0.169.0 as on 0.168.0; a role file carries
`model`, `developer_instructions`, `model_reasoning_effort` and `[[skills.config]]`, not the compaction limit (nor,
as seen on 0.168.0, hooks or MCP), and any unknown key drops the role. `features.multi_agent_v2` can be set per thread. The built-in roles stay
visible; a thread without roles has no `agent_type` to choose. A role's `[[skills.config]]` only disables: nothing
a thread, the process or a calling role disabled comes back, so a per-thread allow-list would also bind the children.

## opencode (experiment 7)

Not studied yet: it comes with the opencode adapter in the next release. opencode 2.1.1 has `opencode serve` (HTTP
API v2, a `--stdio` flag), a native `opencode acp`, a background service and a `--standalone` flag.
<!-- MEASURE id=opencode-cli-surface date=2026-11-12 ver=opencode-2.1.1 env=linux-arm64-orbstack src=opencode---help-output note="only the help was read, the harness was not run" -->

Questions the experiment answers before the adapter is written:

1. `serve` or `acp` as the transport; how many sessions one server holds.
2. What is set per session and what per process: model, MCP, env, cwd, permissions.
3. What failures look like: 429, quota, 5xx, an expired login; whether a reset time comes; whether it retries itself.
4. Resume after `kill -9`; what happens to an unfinished tool call.
5. opencode hooks and plugins and how they relate to the Claude hook dialect.
6. Inheritance of the user's config and the way to cut it.
7. A human joining a live session.
