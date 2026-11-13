# Experiments (spec §16)

Results of the throwaway experiments that precede the core. Code lives in `experiments/` (a separate Cargo
workspace outside the product budgets, ≤ 1 500 lines in total, checked by `scripts/budget.sh`). Every number carries
a `MEASURE` marker. Logs are sanitized samples (`<home>`, `<work>` placeholders; long `tools`/`skills` arrays are
replaced by their length) and serve as fixtures for the scripted harness. Once an experiment is recorded, its
scenario code may leave the tree to free the line limit; the section then names the commit that still holds it
(`git show <commit>:<path>`), and the logs stay.

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
2. A per-attempt marker variable in the session environment; on cleanup the daemon kills every process carrying
   the marker (`/proc/<pid>/environ`; macOS is out of scope for the first release, spec §1.1). The same sweep runs during
   recovery after a daemon crash. Known macOS differences until it is checked: no sweep, no subreaper, and a process
   is told by its pid alone (no start time), so a reused pid passes for the old process.
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
`.beads/` directory, `rusqlite` read-only on `beads.db`), removed from the tree after recording; last in commit
`1629c3a`. Output: `experiments/05-br-events/logs/run.txt`.

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

## codex app-server (experiment 2)

Code: `experiments/03-codex-appserver` (`src/lib.rs`: client over stdio or a WebSocket on a unix socket and a fake
Responses API provider, kept for experiments 4–5; `src/main.rs`: scenarios `threads stdio|unix` and `reconnect`,
removed from the tree after recording, last in commit `1629c3a`; `src/bin/gen.rs`: type generator). Output:
`experiments/03-codex-appserver/logs/run.txt`, message logs next to it. No live model: every
turn goes to the fake provider, `HOME` and `CODEX_HOME` are fresh directories, the process environment holds only
`PATH`, `HOME`, `CODEX_HOME`, `SHELL`.

<!-- MEASURE id=codex-appserver-exp2 date=2026-11-12 ver=codex-0.167.2 env=linux-arm64-orbstack src=experiments/03-codex-appserver/logs/run.txt note="fake provider, n=1 per scenario and transport in the recorded run; 2–3 earlier development runs agreed" -->

| Question | Answer |
|---|---|
| Transport | Both work and carry the same messages (compare `threads-stdio.ndjson` and `threads-unix.ndjson`). **Chosen: stdio.** It needs no WebSocket code (handshake, masking, ping) on a transport the vendor calls experimental and unsupported, and no socket directory with `0700`. The one thing only the socket gives — the server outliving its client — is not needed in the first release (spec §5.6); with stdio the server ends with the daemon (below), so no stale app-server is left holding thread writer locks. |
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
same as on 0.166.0 (spec §7.3). `initialize` answered 150–307 ms after spawn on both transports (77 ms on 0.166.0
per the context notes, other setup).

**Types.** `codex app-server generate-json-schema --out DIR` writes the bundle;
`cargo run -p codex-appserver --bin gen -- DIR/codex_app_server_protocol.v2.schemas.json` takes the hand-picked roots
listed in `gen.rs`, collects the definitions they reach and generates `src/generated/protocol.rs` with `typify`
(9 roots → 83 definitions → 5.1k lines, excluded from the line budgets). The four types the scenarios parse
(`ThreadStartResponse`, `ThreadResumeResponse`, `TurnCompletedNotification`, `HookCompletedNotification`)
deserialized every message received. `InitializeResponse` is only in the non-v2 bundle
(`codex_app_server_protocol.schemas.json`); the client reads it as plain JSON. For WP-29: the same generator as a dev
tool of the adapter crate, the output
committed under `generated/`, regenerated on every codex version bump and checked by the contract tests.

**Fake provider** (sketch for WP-26): an HTTP server on `127.0.0.1` given to codex as
`[model_providers.fake] base_url = "http://…/v1", wire_api = "responses"` in `CODEX_HOME/config.toml`. A turn is
answered with `content-length` SSE of three events — `response.created`, `response.output_item.done` with a
`message` item of one `output_text`, `response.completed` with `usage` — and `connection: close`. Unknown model
names only cause a `warning` "Model metadata for `<model>` not found".

Consequences for the codex adapter (WP-29) and the instance registry (WP-18):

- One app-server per fingerprint over stdio, spawned and owned by the daemon; the hook set is not part of the
  fingerprint, the process environment is.
- Every session-level hook needs `bypass_hook_trust: true` on its thread; per-session data reaches hooks through
  the command line or `session_id`, never through `shell_environment_policy`.
- A daemon restart ends the process and all its turns; recovery is `thread/resume` in a new process plus the
  history check of the idempotency key.

## Finisher from a session (experiment 6, partly)

Code: `experiments/06-finisher` (`src/bin/finish.rs` stands in for the finisher CLI: it connects to the unix socket
named in `AAE_SOCKET` and reports `AAE_ATTEMPT_TOKEN`, `AAE_PROC_TOKEN` and its arguments; `src/main.rs` plays the
daemon side). codex runs on the fake provider of experiment 2, which answers `RUN:<cmd>` with an `exec_command`
call; claude runs on haiku with `--permission-mode bypassPermissions`. Output: `experiments/06-finisher/logs/`.

<!-- MEASURE id=finisher-exp6 date=2026-11-12 ver=claude-2.1.322,codex-0.167.2 env=linux-arm64-orbstack src=experiments/06-finisher/logs/run.txt note="n=1 per round; codex without a live model, claude haiku with a subscription login; codex sandbox not exercised (see below)" -->

| Question | Answer |
|---|---|
| Token in the environment, claude | The Bash tool sees the environment of the `claude` process: the finisher reached the socket with the token of that process. After `--resume` in a new process with another token, the finisher reported the new one — the token belongs to the process, not to the session. |
| Token in the environment, codex | A per-thread `config.shell_environment_policy.set` value reaches the command, and so does a process variable (both have `TOKEN` in the name — the default excludes did not drop them). |
| codex after resume in a new process | **The per-thread config is not restored**: `thread/resume` with only `threadId` ran the command without the per-thread token (the process variable was there). `thread/resume` with the same `config` restored it, with the new value. So the adapter passes the full thread config on every resume (closes the open point of the context notes, §3.5). |
| codex after resume in the same process | **A thread app-server still holds keeps its config**: after `thread/unsubscribe` a thread whose last turn failed (status `systemError`) stayed loaded, `thread/resume` rejoined it, and its command ran with the token of the first attempt (an idle thread was reloaded by the same resume and got the new one). With `-c thread_unload_delay_secs=0` the last unsubscribe closes the thread within milliseconds (`thread/closed`; a resume in between is refused: "is closing; retry thread/resume after the thread is closed"), and a thread with a turn in flight stays loaded. So the adapter starts app-server with that flag and resumes only once `thread/unsubscribe` answers `notLoaded` (codex 0.168.0). |
| codex sandbox | **Not checked.** The sandbox needs unprivileged user namespaces (bubblewrap); the owner's host container forbids them (`unshare -U` → "Operation not permitted"), so every codex command here ran in `danger-full-access`. Whether a `workspace-write` command can connect to the daemon socket and write the tracker database outside the work directory stays open. |

Consequences: the attempt token goes into the process environment for claude (a process per session anyway) and
into `shell_environment_policy.set` of the thread for codex, re-sent with every `thread/resume`. Hooks do not see
the per-thread value (experiment 2), so a hook that must call the finisher gets the token on its command line.

## Config and credential delivery (experiments 4 and 5)

Code: `experiments/04-config-delivery` (`src/bin/claude.rs`: scenarios `flags`, `sources`, `auth`;
`src/bin/codex.rs`: scenarios `mcp`, `rig`, `process`, and `codex mcp TAG ROOT` — a stdio MCP server with one tool
`tag_<TAG>` that records its pid, used by both harnesses). Output: `logs/claude/run.txt`, `logs/codex/run.txt`,
message logs next to them. claude: haiku, clean environment as above, a fresh rig per run; the run that inherits
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
not exclude that user's file; run the daemon as that user, as the service files of spec §13 do.
Open: refresh of a delivered OAuth token after expiry (spec §6.4).

### codex

| Channel | Scope | Works? | Evidence and notes |
|---|---|---|---|
| stdio MCP: `thread/start.config.mcp_servers.<n> = {command, args}` | thread | yes | each of two threads saw only its own tool in the provider request; one server process per thread (plus one short-lived start that exited at once); `mcpServerStatus/list{threadId}` names the thread's server |
| HTTP MCP: `config.mcp_servers.<n> = {url, http_headers}` | thread | yes | codex POSTed `/mcp` with the custom header (`x-aae-agent`) and `user-agent: codex-mcp-client/0.167.2` |
| MCP server lifetime | — | — | `thread/unsubscribe` does not stop the thread's server; when app-server exits on stdin EOF, every server it started is gone (no orphans) |
| Skills: `$CODEX_HOME/skills/<name>/SKILL.md` | process | yes | scope `user` in `skills/list`, named in the provider request |
| Skills: `skills/extraRoots/set` | process | yes | an extra root outside rig and homes; scope `user` |
| Skills: `$HOME/.agents/skills` with a separate `CODEX_HOME` | process | **read** | a separate `CODEX_HOME` does not cut the host's `~/.agents/skills`; only another `HOME` does |
| Skills per thread | — | no | no field in `thread/start`; per thread only via the rig (`.agents/skills` in `cwd`) |
| Rig: `.codex/config.toml`, `AGENTS.md`, `.agents/skills` in `cwd` | thread | yes | all three reached the provider request with a fresh `CODEX_HOME`, with and without a `trust_level = "trusted"` entry for the `cwd` |
| Hooks: `-c hooks.<Event>=[…]` at spawn | process | yes, with the bypass | fired with `source: sessionFlags` |
| Hooks: `hooks.json` in `CODEX_HOME` | process | yes, with the bypass | fired with `source: user` |
| `bypass_hook_trust` | thread | thread only | set in `config.toml` of `CODEX_HOME` it has no effect (no hook ran); set in `thread/start.config` it lets process-level and user hooks run too |
| Host config (`config.toml` of `CODEX_HOME`: instructions, MCP) | process | yes | marker and the host MCP tool reached the thread; the MCP server started per thread |
| Provider key: `env_key` of the provider, value in the app-server environment | process | yes | `authorization: bearer <key>` at the provider |
| Provider key per thread | thread | name only | `config.model_providers.<p>.env_key` in `thread/start` is applied (a missing variable fails the turn: "Missing environment variable"), but the value is still read from the process environment; `experimental_bearer_token` in the thread config was ignored |
| ChatGPT subscription login in a separate `CODEX_HOME` | process | not checked | the host has no codex login (spec §6.4 stays open) |

### Consequences

- claude (WP-27): the adapter delivers everything with flags and files in the state directory: `--settings`
  (file or inline), `--plugin-dir` for skills and plugin hooks, `--mcp-config`; and repeats the whole set on every
  `--resume`. It does not pass `--strict-mcp-config`: that would cut the rig's `.mcp.json` too, and with
  inheritance off the setting sources and the own config directory already cut the host's servers. With inheritance on, the host's personal MCP servers reach the session as well, as spec §6.4 intends. Host inheritance off = `--setting-sources project,local` (not `""`): the rig's
  own config keeps working — the spec §6.3 assumption holds for claude. With an isolated `CLAUDE_CONFIG_DIR` the
  adapter delivers `CLAUDE_CODE_OAUTH_TOKEN` (or `ANTHROPIC_API_KEY`).
- codex (WP-29): MCP, hooks (`config.hooks`), instructions and `bypass_hook_trust` go per thread; skills, the
  provider key value and the host layer are per process. The instance fingerprint (spec §7.3) therefore holds
  `CODEX_HOME`, `HOME`, the skill roots and the provider and its key; MCP and hooks are not in it. Host
  inheritance off = own `CODEX_HOME` **and** own `HOME` (or the host's `~/.agents/skills` leak in); the rig's
  `.codex/`, `AGENTS.md` and `.agents/skills` keep working — the spec §6.3 assumption holds for codex as well.
  Without a key of its own, an isolated process logs in as the host does: the host `auth.json` is linked into its
  `CODEX_HOME`, and a model provider the host config selects comes as the first `-c` flags with its `env_key`
  variable passed from the daemon's environment (`aae-codex/src/part.rs::host_provider`).
