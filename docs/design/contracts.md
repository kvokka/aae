# Contracts (fixed before the core)

The interfaces that later code may not change without a break: the event and its version, the trigger filter and
input mapping, the decisions of orchestrator hooks and their timeouts, the outbox, the driver and tracker
interfaces, the run state machine, the issue gates and the instance registry. Spec references are by section
number (`§9.5`); references inside this document read "see 3.5". Experiment results are cited from [`experiments.md`](experiments.md), not repeated.

Conventions used below:

- JSON field names are `snake_case`; times are RFC 3339 UTC with milliseconds (`2026-11-12T14:05:00.123Z`);
  durations in config and decisions are strings with a unit (`"500ms"`, `"30s"`, `"45m"`, `"2h"`).
- Identifiers are opaque strings to every consumer. The daemon makes them time-sortable with a type prefix:
  `run_…`, `att_…`, `cor_…`, `ins_…`.
- A **default** below is a config parameter with that default value, chosen by the implementer (no report gives
  one); a **ceiling** is a core constant that config and policy cannot raise (invariant 16).
- Rust signatures are a shape, not code: crate layout, lifetimes and error plumbing are settled by the work
  package that writes the code, as long as the operations, types and semantics here stay.

## 1. Event

### 1.1 Envelope, schema version 1

One envelope for the JSONL journal, the API event stream (SSE) and the stdin of orchestrator hooks:

```json
{
  "schema": 1,
  "seq": 4182,
  "ts": "2026-11-12T14:05:00.123Z",
  "type": "run.state_changed",
  "project": "shop",
  "rig": "backend",
  "run": "run_01JC9V8M2T",
  "correlation": "cor_01JC9V7Z0A",
  "data": { "from": "starting", "to": "running", "attempt": 1, "generation": 1 }
}
```

| Field | Type | Meaning |
|---|---|---|
| `schema` | integer | envelope and payload schema version; `1` for the first release |
| `seq` | integer ≥ 1 | one monotonic counter per orchestrator, assigned in the transaction that commits the event; no gaps in the outbox, gaps in the journal only by rotation |
| `ts` | time | commit time (the managed clock in tests) |
| `type` | string | one of 1.3 below |
| `project`, `rig` | string or null | null for orchestrator-level events |
| `run` | string or null | the run the event is about; for `bead.*` the run whose attempt made the change (§10.2 attribution), else null |
| `correlation` | string or null | chain id: a run started by hand starts a chain, a run started by a trigger inherits the chain of the event that fired it; null for events outside any chain |
| `data` | object | payload of the type, see 1.3 |

Version rules:

- Adding an optional field to a payload or a new event type keeps `schema`. Removing or renaming a field,
  changing its type or meaning, or changing envelope fields raises `schema` by one.
- Consumers ignore unknown fields and unknown types. The daemon never emits a field it does not document here.
- A journal file holds only one schema version; the first line after a version change starts a new file.

### 1.2 Event view: the namespace of filters and templates

Filters and templates (see 2) see an event as one flat object: the envelope fields `seq`, `ts`, `type`, `project`,
`rig`, `run`, `correlation`, plus every key of `data` at the same level. The only payload key that reuses an envelope
name is `run`: where a payload carries a `RunView`, the view's `run` is that object and the run id is `run.id`;
elsewhere `run` is the envelope's run id. Examples: `bead.labels` and `bead.epic` of a `bead.ready` event,
`run.formula` of a `run.passed` event.

### 1.3 Payloads

Shared objects:

- `RunView`: `{id, formula, agent, step_kind: "prompt"|"exec", state, class, attempt, generation, bead, workdir,
  depth, parent_run, created_at, started_at}`. `agent` is null for `exec` steps, `class` is null until `failed`,
  `bead` is the bead id or null, `depth` is the trigger depth of §9.5 (0 for a run started by hand).
- `Failure`: `{kind, text, scope, retry_at, retried_by_harness, source, raw, account, provider_model}`: `kind` from
  see 4.4, `text` set only for `unknown`, `scope` one of `session | instance | account | provider_model | run | rig`,
  `retry_at` time or null, `source` one of `harness | adapter | core`, `raw` the harness text cut to 4 KiB,
  `account` and `provider_model` as reported by the adapter or null.
- `Bead`: see 5.2.
- `Gate`: `{key, until, reason, closed_by: "policy"|"operator"|"core", closed_at}`, `until` null = until removed
  by hand.
- `Attempts`: list of `{n, generation, kind: "new_session"|"resume", started_at, ended_at, failure_kind}` of the
  run, newest last, at most 20 entries.
- `Outcome`: `{result: "pass"|"fail", summary, data}`; `summary` ≤ 4 KiB, `data` any JSON ≤ 64 KiB.

Observed events (§5.4):

| Type | `data` |
|---|---|
| `daemon.started` | `{version, pid, interrupted_runs}`; `interrupted_runs` is how many runs recovery moved to `interrupted` |
| `daemon.stopping` | `{reason: "signal"\|"drain"\|"restart"}` |
| `config.reloaded` | `{config_generation, restarted_instances: [id]}` |
| `instance.starting`, `instance.ready`, `instance.stopped` | `{instance: {id, harness, fingerprint, pid, pgid}, reason}`; `pid`/`pgid` null before spawn, `reason` null except `stopped` (`idle`, `config`, `cancel`, `shutdown`, `recovery`, `operator`) |
| `instance.failed` | `{instance, failure: Failure}` |
| `service.starting`, `service.ready`, `service.stopped`, `service.failed` | same as `instance.*` with `service: {id, name, scope, pid, pgid}` instead of `instance` |
| `run.queued` | `{run: RunView, waiting_for: "concurrency"\|"gate"\|"workdir"\|"daemon_stopping"\|null, gate}`; re-emitted when the reason changes |
| `run.started` | `{run}`; once per run, when it first leaves `queued` |
| `run.state_changed` | `{from, to, attempt, generation, reason}` |
| `run.parked` | `{gate: Gate, then: "resume"\|"new_session"}` |
| `run.resumed` | `{gate_key}` |
| `run.passed` | `{run, outcome: Outcome}` |
| `run.failed` | `{run, class: "agent"\|"check"\|"hollow_done"\|"infrastructure", reason, outcome}`; `outcome` null unless the agent declared one |
| `run.cancelled` | `{run, reason}` |
| `trigger.fired` | `{trigger, cause_seq, new_run, inputs}`; `cause_seq` null for schedules |
| `trigger.skipped` | `{trigger, cause_seq, reason: "overlap"\|"input_unresolved"\|"depth"\|"nothing_ready"\|"rejected", detail}` |
| `trigger.switched` | `{trigger, enabled}`; the operator switched a trigger on a rig off or back on (`POST /v1/triggers/disable`, `/enable`): off, neither its schedule nor its events fire it, a hand firing still does, and the switch outlives restarts and reloads |
| `gate.closed` | `{gate: Gate}` |
| `gate.opened` | `{key, by: "timer"\|"operator"\|"policy"}` |
| `bead.created`, `bead.updated`, `bead.closed`, `bead.ready` | `{bead: Bead, actor, changed: [field]}`; `changed` empty except `updated` |
| `tracker.sync_failed` | `{op, bead, error, next_retry_at}` |
| `plugin.hook_failed` | `{plugin, hook_type, event_seq, error: "spawn"\|"timeout"\|"exit"\|"output", exit_code, stderr_tail}` |
| `resync.diverged` | `{subject: "process"\|"instance"\|"run"\|"bead"\|"gate", id, expected, observed}` (no automatic effect follows, invariant 20) |

Deciding events (§5.4; decisions: see 3). Each carries `deadline`, the time after which the core applies the
"no plugin decision" column:

| Type | `data` |
|---|---|
| `run.preparing` | `{run, workdir, env_keys: [name], prompt_append, deadline}`; `env_keys` lists names only, never values |
| `run.requires_action` | `{run, request: {id, kind: "permission"\|"question", tool, input, text}, deadline}` |
| `run.attempt_failed` | `{run, failure: Failure, attempts: Attempts, gates: [Gate], keys: [string], agent_failures_1h: [{run, kind, at}], deadline}` |
| `run.turn_ended_without_outcome` | `{run, stop_reason, remarks, attempts: Attempts, deadline}` |
| `run.finishing` | `{run, outcome: Outcome, remarks, deadline}` |

`remarks` is the number of remarks already returned in this attempt. `gates` lists every closed gate whose key
applies to the run; `keys` lists the gate keys of the run (7), so a policy can name the account, model, agent or rig
gate in its decision. Credentials and environment values never enter an event: the journal is not a secret store.

## 2. Filter and template language

### 2.1 Paths

A path is a dotted chain of object keys into the event view (see 1.2) or into the run context of a prompt template:
`bead.epic`, `project`; the word `data` never appears in a path. No indexing, no wildcards. A path that a payload of the event type
cannot have is a config error at load time (the payload schemas of 1.3 are known to the loader); so is a `where` path
that ends on an object, which no matcher can equal.

### 2.2 `where`

`where` is a table of `path = matcher`; all entries must match (AND). An empty or absent `where` matches every
event of the type.

| Matcher | Matches when |
|---|---|
| scalar (string, integer, boolean) | the value at the path equals it; if the value is an array, it contains it |
| array of scalars | the value equals any element (for an array value: contains any element) |

A path that is absent or null in the event does not match. There is no negation, no comparison and no regular
expression in the first release; a trigger that needs more uses an `exec` step that decides. Example — the
`label = "auto"` of the spec §9.5 sample is written `where = { "bead.labels" = "auto" }`.

### 2.3 Substitution (`inputs` and prompt templates)

One language for trigger `inputs` (§9.5) and prompt templates (§9.1): `{{ path }}` with optional spaces, nothing
else — no filters, conditions or loops.

- In `inputs` the root is `event` (`{{ event.bead.epic }}`); a schedule trigger has no event, so any placeholder
  there is a config error.
- A string that is exactly one placeholder takes the JSON type of the value (an integer stays an integer, an
  object stays an object). A placeholder inside a longer string must resolve to a scalar and is rendered as text.
- Tables and arrays in `inputs` are mapped recursively.
- An unresolved path in `inputs` (absent or null at fire time) skips the firing:
  `trigger.skipped{reason: "input_unresolved", detail: <path>}`.
- In a prompt template the roots are `inputs`, `bead` (when the run has one), `rig`, `run` and `project`; an
  unresolved path is an error before the run starts (§9.1) and fails the run with class `infrastructure`.
- `{{` that is not a placeholder is written `{{"{{"}}`.

A bead input is either a bead id (string) or `{ next_ready = true, epic = <id or absent> }` (§10.3).

## 3. Orchestrator hooks: decisions and timeouts

### 3.1 Invocation

Built-in plugins (`default-policy`, `worktree`) are in-process executors of the same contract: same input value,
same decision value, same timeouts. External hooks are declared in `plugin.toml`:

```toml
[[hook]]
on = "run.finishing"           # one event type, observed or deciding
command = ["bin/check", "--strict"]   # relative to the plugin directory
timeout = "30s"
```

- stdin: the event envelope (see 1.1) as one JSON line, then EOF.
- cwd: the plugin directory. Environment: `PATH`, `HOME`, `LANG`, plus `AAE_SOCKET` (API socket),
  `AAE_PLUGIN_DIR` and `AAE_PLUGIN_PARAMS` (the `config` table of the plugin's `[[plugin]]` declaration as a JSON
  object, `{}` without one). Nothing else is inherited from the daemon.
- A program path with a `/` is relative to the plugin directory; a bare name is looked up on `PATH`.
- Own process group; at the timeout `SIGTERM` to the group, `SIGKILL` 2 s later.
- stdout is read up to 64 KiB (more is an `output` failure); the last 4 KiB of stderr are kept.
- Result: exit 0 with empty stdout — no objection and no decision; exit 0 with one JSON object on stdout — the
  decision; exit 2 — block with the reason taken from stderr; any other exit, a signal, a timeout, invalid JSON or
  a decision not allowed for the event — a hook failure, reported as `plugin.hook_failed`.
- Timeout: default 30 s, ceiling 300 s per invocation.
- Hooks run after commit, outside the core loop; the result enters the core as a message. Hooks of different
  events run concurrently; the hooks of one run are serialized in `seq` order on two queues: deciding events on
  one, observed events on the other (see 3.5), so an observer never delays a decision.
- Delivery is at least once: a hook cut by a daemon crash runs again after restart (see 3.4). Hooks must be
  idempotent; `seq` identifies the event.

### 3.2 Decisions

`after` is a duration (default `"0s"`). A decision value that is not listed for the event is a hook failure.

| Event | Allowed decisions | Exit 2 means | Hook failure means |
|---|---|---|---|
| `run.preparing` | `{"decision":"proceed","workdir":"/abs/path","env":{"NAME":"value","DROP_ME":null},"prompt_append":"text"}` — every field but `decision` optional; `{"decision":"deny","reason":"…"}` | deny | deny (veto hook, §11.3) |
| `run.requires_action` | `{"decision":"allow","updated_input":{…}}` (`updated_input` optional); `{"decision":"deny","message":"…"}`; `{"decision":"answer","text":"…"}` (only for `kind: "question"`) | deny with the stderr text as message | no decision from this plugin |
| `run.attempt_failed` | `{"decision":"resume","after":"30s"}`; `{"decision":"new_session","after":"0s"}`; `{"decision":"park","gate":{"key":"…","until":"<time>"\|null,"reason":"…"},"then":"resume"\|"new_session"}`; `{"decision":"fail","reason":"…"}`; `gate` is required on `park` and optional on the other three (the gate is closed in the same transaction) | `fail` with the stderr reason | no decision from this plugin |
| `run.turn_ended_without_outcome` | `{"decision":"continue","message":"…"}`; `{"decision":"fail","reason":"…"}` | `fail` | no decision from this plugin |
| `run.finishing` | `{"decision":"accept"}`; `{"decision":"veto","reason":"…"}`; `{"decision":"remark","message":"…"}` | veto | veto with reason `hook_failed:<plugin>` (§11.3) |

Hooks on observed events may only exit 0 or fail; their failures are reported and otherwise ignored.

### 3.3 Several plugins on one event

Plugins are ordered by connection order (orchestrator level first, then project level, each in declaration
order); the bundled plugins come after all of them (docs/design/config.md, `[builtin.<name>]`).

- `run.preparing`: hooks run one after another; each receives the event with the earlier modifications applied
  (`workdir`, `env_keys`, `prompt_append`). `env` entries merge by name, `null` removes; `prompt_append` texts are
  concatenated with a blank line. The first `deny` stops the chain and fails the run with class `check`.
- `run.finishing`: every hook runs. Any `veto` → `failed{check}` with the first veto reason; otherwise any
  `remark` → one remark, messages joined with a blank line in plugin order; otherwise accept. A `veto` on a declared
  `fail` changes nothing: the run fails with class `agent`.
- `run.requires_action`, `run.attempt_failed`, `run.turn_ended_without_outcome`: the first decision in plugin order
  wins; later hooks are not started once a decision exists.
- Every hook done and no decision: the hook runner sends an explicit "no decision" message, and the "no plugin
  decision" column of §5.4 applies at once — start as is; wait for a human answer through the API until
  `human_timeout` (default 1 h), then answer `deny`; `failed{class: infrastructure}`; `failed{class: hollow_done}`;
  accept. A hook that timed out is a hook failure (3.2), not "no decision": on `run.preparing` and
  `run.finishing` it blocks.
- The `deadline` reached with no message from the runner: the hooks count as failed. On the veto events
  (`run.preparing`, `run.finishing`) that blocks — `failed{check}` with reason `hook_deadline`; on the other
  deciding events a hook failure is "no decision from this plugin", so the "no plugin decision" column applies.

`deadline` of a deciding event = commit time + the sum of the timeouts of its hooks + 5 s (one hook can never
hold a run longer than its own timeout; the runner reports before it unless it is lost). `run.requires_action` keeps waiting for a human after that deadline,
until `human_timeout`.

### 3.4 Decisions and restarts

The hook effect of a deciding event stays queued until its decision is committed (see 3.5). A decision message names the event
`seq` and the run generation of the event; the core applies it only if the run is still in the state that emitted
that event with that generation, else it is dropped as stale and nothing changes. After a restart the dispatcher
runs the hooks of every deciding event still pending; the run waits in its state meanwhile (the run was moved to
`interrupted` by recovery only if it had a live attempt, see 6.4).

### 3.5 Outbox lifetime

- Every committed event is an outbox row. The one durable consumer is the journal writer: it acknowledges the
  rows after the lines are written and flushed (`consumers` table), and the acknowledged rows are deleted in that
  transaction.
- Triggers and hooks are not outbox consumers; each is tied to the transaction that commits the event instead,
  which keeps the same at-most-once-per-`seq` guarantee in a stronger shape:
  - The trigger matcher matches in the transaction that commits the event, so the run it creates (or its
    `trigger.skipped`) commits with the event or not at all — the key `(trigger, seq)` holds by construction —
    and events the firing emits are matched in the same transaction. A schedule fires in the transaction of its
    deadline, which sets the next one.
  - The transaction that commits an event also queues the hooks on it as one effect. The key is `hooks:<run>` for
    a deciding event (a run waits on at most one) and `hooks:<run>:obs` for an observed one; each key runs its
    hooks in `seq` order, and a slow observed hook never holds a deciding one past its deadline (3.3). The effect
    is deleted only after every hook finished and the decision message was handled; a crash runs it again.
- There is no time-based expiry: a row lives until the journal writer acknowledges it, and a hook effect lives
  until its decision is handled, so a crash neither loses nor doubles a journal line, a trigger or a hook decision.
  Both are bounded in time (hook timeouts, journal write errors retried with the effect backoff), so the outbox
  drains. The depth and the age of the oldest row are part
  of the daemon status (API) and of `aae doctor`.
- API event streams are not outbox consumers: they read the journal from a `seq` (§5.3) and get the "gap" error
  when it is rotated away.

## 4. Driver interface

### 4.1 Shape

The core owns one driver per harness, behind one object-safe trait; the binary wires the claude and codex adapters
in, tests wire the scripted driver in (`crates/aae-testkit`). Futures are boxed; events go to the core through a
sink handed to `start_instance`. The adapter translates and executes; it never retries a turn, parks or decides
an outcome (§4, §7.2).

In code (`crates/aae-core/src/driver.rs`) `Capabilities.versions` joins with the first adapter (WP-27): until then
it has no producer.

```rust
trait Driver: Send + Sync {
    fn harness(&self) -> &'static str;                        // "claude", "codex"
    fn capabilities(&self, process: &ProcessPart) -> Capabilities;
    fn account(&self, process: &ProcessPart) -> Option<String>;
    fn split(&self, config: &EffectiveConfig, plan: &SessionPlan) -> Result<Split, Failure>;   // kind config_error

    // instance
    fn start_instance(&self, id: &str, process: &ProcessPart, sink: EventSink) -> BoxFuture<Result<InstanceHandle, Failure>>; // id from the registry (8)
    fn health(&self, instance: &InstanceHandle) -> BoxFuture<Health>;
    fn stop_instance(&self, instance: &InstanceHandle, grace: Duration) -> BoxFuture<()>;

    // session
    fn start_session(&self, instance: &InstanceHandle, session: &SessionPart) -> BoxFuture<Result<SessionRef, Failure>>;
    fn resume_session(&self, instance: &InstanceHandle, session: &SessionRef, part: &SessionPart) -> BoxFuture<Result<(), Failure>>;
    fn send_prompt(&self, instance: &InstanceHandle, session: &SessionRef, text: &str, key: PromptKey) -> BoxFuture<Result<(), Failure>>;
    fn cancel_turn(&self, instance: &InstanceHandle, session: &SessionRef) -> BoxFuture<Result<(), Failure>>;
    fn respond(&self, instance: &InstanceHandle, session: &SessionRef, request_id: &str, answer: &Answer) -> BoxFuture<Result<(), Failure>>;
    fn close_session(&self, instance: &InstanceHandle, session: &SessionRef) -> BoxFuture<()>; // transcript stays

    // transcript
    fn transcript(&self, instance: &InstanceHandle, session: &SessionRef, key: PromptKey) -> BoxFuture<Result<Transcript, Failure>>;
    fn live(&self, session: &SessionRef) -> Option<broadcast::Receiver<serde_json::Value>>;
}
```

### 4.2 Types

| Type | Content |
|---|---|
| `EffectiveConfig` | the merged harness-neutral categories of §6.2 (model, effort, env, MCP servers, harness hooks, skills, system prompt, permission mode, plugin parts, host inheritance flag) plus the uninterpreted `native` block of the harness; produced by config assembly with the provenance of every value |
| `SessionPlan` | `New { workdir, run, attempt }` or `Resume { session: SessionRef, workdir, run, attempt }`; the adapter needs `run`/`attempt` only to name files in the state directory and to set the attempt environment |
| `Split` | `{ process: ProcessPart, session: SessionPart, fingerprint: Fingerprint }`; the two parts are opaque serializable values of the adapter; `fingerprint` = SHA-256 of the harness name and the canonical JSON of `process` (see 8) |
| `InstanceHandle` | `{ id, pid, start_time, pgid }`; `start_time` is the kernel start time of `pid` (`/proc/<pid>/stat`), pid + start time identify the process (§5.2) |
| `SessionRef` | opaque JSON the adapter fills with everything resume needs; the core stores and returns it. claude: `{session_id, config_dir}`; codex: `{thread_id, codex_home}` (§7.1: the transcript lives in `CODEX_HOME`) |
| `PromptKey` | UUID v5 of `"aae:prompt:<run>/<attempt n>/<prompt n>"` in the URL namespace; the idempotency key of one prompt (see 6.4) |
| `Answer` | `Allow { updated_input }`, `Deny { message }`, `Text { text }` (decisions of `run.requires_action`) |
| `Transcript` | `Missing` or `Present { prompt: Absent \| Unanswered \| Answered }` — whether the session exists and where the prompt with `key` stands in it |
| `Health` | `Ok`, `Degraded { text }`, `Dead` |

The live tap (`live`) is the direct relay of §5.3: raw harness messages of a session, up to 256 buffered, a slow
reader loses messages; nothing of it enters the state or the journal.

### 4.3 Normalized events

Every event carries `instance`, `session` (null for instance-wide events) and `seq`, monotonic per instance from 1.
They are input messages of the core, not journal events; the core turns some of them into the run events of §1.3.

| Event | Fields | Core use |
|---|---|---|
| `prompt.acked` | `key` | prompt delivery `sent → acked`; ends `starting` |
| `turn.started` | — | watchdog: first event seen |
| `turn.ended` | `stop_reason: completed \| cancelled \| failed` | candidate end of the attempt's work; `failed` follows a `failure` event |
| `state` | `state: running \| idle \| requires_action`, `background: bool` | "idle without background" = `idle` and `background == false`; the adapter applies the harness rule |
| `permission.requested` | `request_id, kind: permission \| question, tool, input, text` | `requires_action`, `run.requires_action` |
| `usage` | `input_tokens, output_tokens, rate_limit: {utilization, resets_at} \| null` | gate inputs for the policy (via the next failure event) |
| `retrying` | `attempt, delay, text` | watchdog: activity; the harness is retrying by itself |
| `activity` | `tool_running: bool` | watchdog only: moves the silence deadline, kept in memory, no transaction; the adapter coalesces it to at most one per second per session |
| `failure` | `Failure` (see 1.3) | ends the attempt with `run.attempt_failed`; a failed single tool call is never sent (the harness hands it back to the model), `tool_failure` means a tool is gone for the whole session (4.4) |
| `exited` | `code, signal` | the instance process is gone: every session on it gets `harness_crash` (one session per instance) or `adapter_crash` (shared instance) unless a failure was already reported |

### 4.4 Failure kinds

The kinds of §7.1. Emitted by the adapter unless marked core; default `scope` in the last column (the adapter
may report a narrower or wider scope when the harness says so).

| Kind | Source | Scope |
|---|---|---|
| `quota_exhausted` | adapter | account |
| `rate_limited` | adapter | provider_model |
| `overloaded` | adapter | provider_model |
| `auth_required` | adapter | account |
| `billing` | adapter | account |
| `context_overflow` | adapter | session |
| `config_error` | adapter | session |
| `harness_crash` | adapter | session |
| `adapter_crash` | adapter | instance |
| `daemon_crash` | core | run |
| `stall` | core | session |
| `runaway_turn` | core | session |
| `tool_failure` | adapter | session (MCP server down for the whole session) |
| `transcript_divergence` | adapter, core (resume found no transcript) | session |
| `workspace_failure` | core | rig |
| `hollow_done` | core | run |
| `unknown` | adapter | session; `text` holds the harness wording |

Success traps (§7.2) are the adapter's: claude `subtype:"success"` with `is_error:true` is a failure, never
`turn.ended{completed}`; an exit with code 0 and no turn result is `harness_crash`.

### 4.5 Capabilities

`Capabilities { sessions_per_instance, instance_idle, versions }`:

- `sessions_per_instance`: claude 1 (a process per session, §7.3); codex the config value `max_sessions`
  (default 8).
- `instance_idle`: how long an instance without sessions lives on (8): claude 0, codex the config value
  `instance_idle_s` (default 60 s).
- Whether the harness retries 429 itself is the adapter's knowledge: it sets `Failure.retried_by_harness` on the
  failure (claude true, codex false; §7.2). Whether a quota failure carries `retry_at` shows on the failure itself
  (claude: subscription login only; codex: yes, from the rate-limit update).
- `versions`: the tested range of the harness binary, `[lowest, highest]` inclusive; outside it the daemon warns at
  start, and `aae doctor` does.

`account(process)` returns the account id the gates use (§5.7): a stable hash of the credential source of the
process part, never the credential itself.

### 4.6 Harness specifics the contract relies on

| Concern | claude | codex |
|---|---|---|
| Process and session | process per session; everything is in `ProcessPart`, so the fingerprint is unique per session; `start_session` and `resume_session` only bind the reference (the `--session-id`/`--resume` flag is part of the spawn) | one app-server per fingerprint over stdio; `ProcessPart` = `CODEX_HOME`, `HOME`, skills (links in its own `CODEX_HOME/skills`; refused on the host's home), provider and key variable, process environment; `SessionPart` = cwd, model, effort, instructions, MCP, hooks with `bypass_hook_trust`, command environment ([experiments](experiments.md#config-and-credential-delivery-experiments-4-and-5)) |
| Prompt key | user message `uuid` | `clientUserMessageId` |
| `prompt.acked` | `command_lifecycle{queued}` with our uuid | `turn/start` response with a turn id |
| Turn end | `result` of a turn, then "idle without background" by the three-condition rule ([experiments](experiments.md#idle-without-background-both-signals-in-one-run)) | `turn/completed` only; thread status `idle` does not end a turn (§7.5) |
| Cancel | `control_request{interrupt}`; the following `aborted_tools` result is `turn.ended{cancelled}` | `turn/interrupt` |
| Resume | full flag set again on every `--resume` | full thread config again on every `thread/resume`, after app-server closed the thread (`thread/unsubscribe` until `notLoaded`, at most 10 s, else `transcript_divergence`): a thread it still holds keeps the old config |
| Attempt environment | process environment | `shell_environment_policy.set` of the thread; hooks get per-session values on their command line |
| Transcript check | the session file under `config_dir`: user record with `key`, answer after it | `thread/read` with turns: `userMessage` with `clientId == key`, completed turn after it |

The failure classification tables (harness wording → kind) are owned by the adapters and pinned by the contract
tests of §7.8 (WP-28, WP-30).

## 5. Tracker interface

### 5.1 Operations

Twelve operations (§10.1); the trait in code carries those its callers use (the core: `claim`, `release`, `close`,
`ready(None)`), and the adapter adds the rest with the API that needs them. Every call names the actor: `aae:<run id>` when the daemon acts for a run, `aae` for
the daemon's own reads. Sessions and exec steps get `BD_ACTOR=aae:<run id>:<attempt n>` and the database path in
their environment ([experiments](experiments.md#br-as-an-event-source-experiment-3)). An actor of any other form is a
human or an outside tool and starts a new correlation chain (§9.5).

```rust
trait Tracker: Send + Sync {
    fn create(&self, actor: &str, bead: &NewBead) -> BoxFuture<Result<Bead, TrackerError>>;
    fn show(&self, id: &str) -> BoxFuture<Result<Bead, TrackerError>>;
    fn update(&self, actor: &str, id: &str, patch: &BeadPatch) -> BoxFuture<Result<Bead, TrackerError>>;
    fn claim(&self, owner: &str, id: &str) -> BoxFuture<Result<Bead, TrackerError>>;
    fn release(&self, owner: &str, id: &str) -> BoxFuture<Result<(), TrackerError>>;   // conditional
    fn close(&self, owner: &str, id: &str, reason: &str) -> BoxFuture<Result<(), TrackerError>>; // conditional
    fn ready(&self, epic: Option<&str>) -> BoxFuture<Result<Vec<Bead>, TrackerError>>;
    fn list(&self, filter: &BeadFilter) -> BoxFuture<Result<Vec<Bead>, TrackerError>>;
    fn add_dependency(&self, actor: &str, id: &str, depends_on: &str) -> BoxFuture<Result<(), TrackerError>>;
    fn comment(&self, actor: &str, id: &str, text: &str) -> BoxFuture<Result<(), TrackerError>>;
    fn changes(&self, after: i64) -> BoxFuture<Result<Vec<Change>, TrackerError>>;           // after a seq
    fn health(&self) -> BoxFuture<Health>;
}
```

`release` and `close` carry no generation (invariant 4): the condition `assignee == aae:<run id>` is the same check,
because a run id is never reused and a run owns at most one bead once. A claim answer carries the generation it was
made for and is dropped unless the run still owns the bead in that generation and still waits for it in `preparing`.

`changes` is pull-only; the adapter also owns a watcher that sends a "changed" message to the core, and the core
then calls `changes` from its stored cursor (and on start and after recovery without waiting for a signal). One
effect on the queue `tracker:<project>` reads `changes(cursor)` and then `ready(None)`; the core turns the writes
past the cursor into events, moves the cursor (table `tracker_cursors`, never back) and diffs the ready set. The
first read of a project's tracker takes its whole history as the starting cursor and raises no `bead.created|
updated|closed` for it; every bead ready at that read is new to the set and raises `bead.ready`. A write whose actor
is `aae:<run id>[:<attempt n>]` carries that run and its correlation; `bead.ready` carries those of the last such
write in the same read (the write that freed the bead is usually that one); the rig of a bead event is the bead's
`rig:` label, absent without one. The daemon gives a project a tracker when `<project>/.beads` exists, and every
session and exec step of the project gets `BEADS_DIR` (layer: project) and `BD_ACTOR=aae:<run id>:<attempt n>`.

### 5.2 Types and errors

- `Bead`: `{id, title, description, status: "open"|"in_progress"|"blocked"|"deferred"|"closed", priority, labels,
  assignee, epic, created_at, updated_at, closed_at}`. `epic` is computed by the adapter (br has no such field,
  §10.2). The rig of a bead is its `rig:<name>` label.
- `NewBead`: `{title, description, priority, labels, epic}`; `BeadPatch`: the same fields, all optional.
- `BeadFilter`: `{status, labels (all must match), epic, assignee}`, all optional.
- `Change`: `{seq, kind: "created"|"updated"|"closed", actor, changed: [field], bead: Bead}`; `seq` orders the
  writes and is the cursor of `changes(after)`, `bead` is the bead when the change is read; the core maps it to `bead.created`,
  `bead.updated`, `bead.closed`, and derives `bead.ready` by diffing `ready(None)` with the last ready set it keeps
  in the state (a bead is "ready" the first time it enters the set, §9.5).
- `TrackerError`: `NotFound`; `Claimed { holder }` (claim of a bead someone else holds — told by the state of the
  bead after the call, not by the exit code text, §10.2); `OwnerMismatch { holder }` (conditional release or close
  found another owner, nothing changed); `Unavailable { text }` (transient: retried by the effect queue with
  backoff, `tracker.sync_failed` on every failure); `Invalid { text }` (permanent).
- `release` and `close` are conditional on `assignee == owner`; br 0.7.6 has neither a conditional close nor a
  release, so the adapter reads, compares and writes. The window between read and write is open only to humans
  (the daemon is the only claimer, invariant 3). An owner found changed before the write is `OwnerMismatch`,
  reported as `resync.diverged`; a human write that lands inside the window shows up only as a `bead.updated` of
  the next `changes` batch, until the resync of the tracker compares owners (WP-20b).
- The state of the orchestrator is the truth of ownership; the assignee in br mirrors it (§10.3).

## 6. Run

### 6.1 States

| State | Meaning | Deadline (default) | On deadline |
|---|---|---|---|
| `queued` | not started: waits for a concurrency slot, a free workdir or an open gate; the reason is visible (`run.queued`) | `queue_timeout` 7 d | `failed{infrastructure, queue_timeout}` |
| `preparing` | the bead claim confirmed, `run.preparing` hooks running, workdir and environment being fixed, snapshot of the effective config written | `start_timeout` for the claim, then the hook deadline (see 3.3) | the hooks count as failed → deny → `failed{check, hook_deadline}` |
| `starting` | an attempt: instance acquired, session started or resumed, first prompt sent, waiting for `prompt.acked` | `start_timeout` 120 s | attempt failure `stall` |
| `running` | the agent works (or the `exec` command runs) | step `timeout` of the formula (default 45 m) per attempt → `runaway_turn`; silence budgets: first event after ack 5 min, model thinking 5 min, tool running 10 min, background work after the turn 30 min → `stall` | attempt failure |
| `requires_action` | waits for an answer to a permission request or question; not silence; the step `timeout` of `running` pauses here and resumes with the time it had left | `human_timeout` 1 h | answer `deny`, back to `running` |
| `finishing` | outcome declared and the session idle without background; `run.finishing` hooks running | hook deadline | the hooks count as failed → veto → `failed{check, hook_deadline}` (a declared `fail` stays `failed{agent}`) |
| `interrupted` | the current attempt ended with a failure (or by a daemon restart); `run.attempt_failed` is deciding | hook deadline, then the decision's `after` | no decision → `failed{infrastructure}` |
| `parked` | a started run waits for a gate; no harness process | `park_timeout` 7 d from parking, or the gate's `until` if later (a gate with `until` opens by its timer first) | `failed{infrastructure, park_timeout}` |
| `passed`, `failed{class}`, `cancelled` | terminal | — | — |

`interrupted` in §9.3 names the daemon restart only; here it is every attempt failure waiting for its decision,
so that the state machine has one place where `run.attempt_failed` is decided.

Ceilings of the core (invariant 16): 20 attempts per run, not counting the attempts whose failure the decision parked on an `account:` gate (spec §2.1 item 12: a wait on the account does not spend the ceiling; the attempt is marked `waited` in the state), 10 remarks per attempt (`continue` after a hollow turn
and `remark` after `run.finishing` both count). The eleventh fails the run with reason `remarks_ceiling` and the class
the event would give: `hollow_done` for `continue`, `check` for `remark`. Between turns (from idle without background
to the next prompt) the step `timeout` pauses like in `requires_action`. Trigger depth (§9.5): a trigger does not start a run deeper than 10 (`trigger.skipped{depth}`); the limit is fixed, with no setting.

### 6.2 Transitions

| From | On | To | Effects after commit |
|---|---|---|---|
| — | create (API, CLI, MCP, trigger) with an idempotency key | `queued` | — (with a bead input the run is recorded as the bead's owner in this transaction, invariant 3: a bead another live run holds refuses; `{ next_ready = true, epic }` takes the first bead of the last ready set, in the tracker's order, that is in the epic, of the run's rig or of no rig, without an owner and without a closed gate; none → the reply `{"nothing_ready": true}`, for a trigger `trigger.skipped{nothing_ready}`; the input then holds the bead id) |
| — | create with `continue_from: <run>` (a terminal run with a session; never implicit, §9.3) | `queued` | in the workdir of that run; the first attempt resumes that run's last session with the formula prompt; a later `resume` uses the run's own session, or that one while it has none; a harness other than the agent's → `failed{infrastructure}` |
| `queued` | slot free, workdir free, no gate of the run closed | `preparing` | with a bead: the tracker claim as owner `aae:<run>` (queue `tracker:<project>`), and the prompt renders once the claim returned the bead; `run.preparing` hooks |
| `preparing` | decision `proceed` (or no hooks) | `starting` | attempt 1: instance acquire, `start_session`, `send_prompt` |
| `preparing` | `deny`, template error, claim error `Claimed` / `NotFound` / `Invalid`, no claim answer within `start_timeout` | `failed{check}`, `failed{infrastructure}`, `failed{infrastructure, bead_claimed / bead_not_found / bead_invalid}`, `failed{infrastructure, tracker_unavailable}` | — |
| `starting` | `prompt.acked` of the first prompt | `running` | — |
| `running` | `permission.requested` | `requires_action` | `run.requires_action` hooks |
| `requires_action` | answer from a hook or a human, or `human_timeout` | `running` | `respond` |
| `running` | turn ended, idle without background, outcome declared | `finishing` | `run.finishing` hooks |
| `running` | turn ended, idle without background, no outcome | `running` | `run.turn_ended_without_outcome` hooks; `continue` → next prompt in the same attempt; `fail` or no decision → `failed{hollow_done}` (with the hook reason for `fail`) |
| `preparing` (exec) | decision `proceed` (or no hooks) | `running` (no session to start: the command is spawned at once) | the command runs in the workdir with `PATH`, `HOME`, `LANG` of the daemon and the `env` of `run.preparing` |
| `running` (exec) | exit 0 / other code or a signal | `finishing` with outcome `pass` / `fail`, `summary` `exit <code>` or `signal <n>`, `data` `{exit_code, stdout, stderr}` (last 4 KiB each) | `run.finishing` hooks |
| `running` (exec) | exit 75 / timeout / spawn error | `interrupted` with failure `unknown` (`text` `exit 75 (EX_TEMPFAIL)`, scope `run`) / `runaway_turn` / `workspace_failure` | `run.attempt_failed` hooks; every attempt runs the command again |
| `starting`, `running`, `requires_action` | `failure`, `exited`, deadline, exec exit 75 | `interrupted` | stop the session (cancel, then stop the instance if it is the session's own, then sweep descendants); `run.attempt_failed` hooks |
| `finishing` | accept / veto / remark | `passed` or `failed{agent}` / `failed{check}` / `running` | bead close or conditional release; on remark the message goes as the next prompt of the same attempt, the declared outcome is cleared, the token stays valid. A remark for an exec step is not allowed (hook failure) |
| `interrupted` | `resume` / `new_session` | `starting` after `after` | new attempt: `generation + 1`, new token; `resume` first checks the transcript (invariant 17), `Missing` → attempt failure `transcript_divergence` |
| `interrupted` | `park` | `parked` | gate closed in the same transaction |
| `interrupted` | `fail`, no decision, attempts ceiling | `failed{infrastructure}` | gate closed if the decision carries one |
| `parked` | every key of the run open, a concurrency slot free (in creation order with the queue) | `starting` (`then` of the park decision); `run.resumed` | new attempt |
| `preparing`, `interrupted` | an attempt would start while a key of the run is closed (by anyone, invariant 18) | `parked` (`then`: `new_session` from `preparing`, the decided kind from `interrupted`) | — |
| any non-terminal | operator cancel | `cancelled` | stop the session, conditional release of the bead |

Every terminal transition releases the run's hold on its workdir and its ownership of its bead. A bead the run
claimed is closed (`passed`) or released conditionally (`failed`, `cancelled`) by an effect on the queue
`tracker:<project>`, followed by a new read of the ready set; a refusal (`OwnerMismatch`, `NotFound`) is
`resync.diverged{subject: "bead"}`, an unavailable tracker `tracker.sync_failed` and a retry, and the outcome of the
run never changes. The failure counter of the bead (§10.3): a failed run that held the claim adds one, `passed`
resets it, `cancelled` and a refused claim leave it; at `bead_failures` (orchestrator config, default 3) the core
closes the gate `bead:<project>/<id>`, and the operator opening that gate resets the counter. Operator holds (§5.7
gates closed by the operator, cancel) are read by every automatic decision: a decision that would start an attempt
of a run whose gate the operator closed parks it instead (invariant 18).

### 6.3 Finisher

- Every attempt gets a token: 32 random bytes as 64 hex digits. The state keeps its SHA-256 with `(run, attempt,
  generation)`; the token itself reaches the adapter through `SessionPlan` and lives only in the attempt's launch
  effect until it runs. The token is valid while its attempt is the run's current one and the run is not
  terminal.
- The session environment carries `AAE_RUN`, `AAE_ATTEMPT_TOKEN` and `AAE_SOCKET` (see 4.6 for how each harness
  gets it).
- CLI: `aae finish pass|fail --summary <text> [--data <json>|@<file>]` → `POST` of the outcome to the API with the
  token. Answers: accepted; `stale_attempt` (an older or ended attempt — nothing changes); `run_terminal`;
  `invalid` (unknown token or run, `summary` over 4 KiB, `data` over 64 KiB, a call while the run is
  `finishing`: the declared outcome is under check, or a call while the run waits for the decision on
  `run.turn_ended_without_outcome`: the turn is over, and an outcome now would decide nothing). With the daemon unreachable the command retries with backoff for 60 s (`AAE_FINISH_RETRY` seconds overrides it), then exits with code 75; every try carries the same idempotency key, a hash of the request, so a retry after a lost reply gets the first answer.
- The table of §9.4 maps onto 6.2: a call during a turn stores the outcome (`running` stays); a repeated call
  replaces it; the end of the turn with background work keeps `running` until idle without background (deadline:
  background silence budget).

### 6.4 Prompt delivery and recovery

- Every prompt has a delivery record `intent → sent → acked → settled` and a `PromptKey`. `intent` is written
  with the decision to send; `sent` after `send_prompt` returned; `acked` on `prompt.acked`; `settled` when the turn
  ended. No marker in the prompt text: both harnesses keep the key in the transcript natively
  ([experiments](experiments.md)), so the transcript check finds it.
- Daemon start (§5.6): open the state and drain the outbox; for every recorded instance check pid + start time,
  kill its process group and sweep every process carrying its marker `AAE_INSTANCE=<id>` in the environment, wait
  for the end (invariants 6, 7); runs in `starting`, `running`, `requires_action` → `interrupted` with
  `daemon_crash`, the launches, prompts and answers they still had queued dropped, and the command of an exec step
  killed by its `AAE_INSTANCE` marker (random per attempt) before any later attempt runs it; runs in `preparing`,
  `finishing`, `interrupted` keep their state — their hook effect is still
  queued and its hooks run again; re-arm deadlines and schedules; resync. Deadlines are wall-clock times:
  the writer reads the wall clock at least every 30 s, so a deadline passed while the machine slept fires late by
  at most that.
- A resumed attempt after `daemon_crash` checks the transcript for the last unsettled key: `Absent` → send the
  same prompt with the same key; `Unanswered` or `Answered` → send the reconcile prompt (config text, default
  "Your previous turn was interrupted. Check the current state of the work and continue toward the goal.") with a
  new key.

## 7. Issue gates

Issue gates are the core's only "do not start yet" mechanism: a record "key closed until time T, or until removed
by hand". A run with a closed key waits in `queued` (or `parked`, once started) with the gate as the visible reason.
Three parties close gates: the policy in answer to a failure (an account limit, an agent quarantined after a crash
series), the operator (a pause) and the core itself (a bead after a series of failed runs, §10.3). Gates are rows
in the state: they survive a restart, are listed by the API (`/v1/gates`, `aae gate`), and every closure and opening
is an event. The account traffic light and the operator hold are built from this one mechanism, with no subsystem
of their own. The adapter reports the account id (4.5, `Driver::account`).

- Keys: `orchestrator`, `project:<project>`, `rig:<project>/<rig>`, `agent:<agent>` (the qualified agent name of
  §8.2), `bead:<project>/<bead id>`, `account:<harness>/<account id>`, `model:<provider>/<model>`. The keys of a
  run are all of these that it has; a run is admitted only if none is closed.
- A gate is a set of closures `{key, until, reason, closed_by, closed_at}`, at most one per `(key, closed_by)`;
  the key is closed while any closure is active. Automation (`policy`, `core`) removes only its own closures; an
  operator closure is removed only by the operator, so automation never reopens an operator pause (criterion 19).
- A closure with `until` ends by the managed clock (`gate.opened{by: "timer"}`); without `until` only by hand
  (API, CLI).
- The account key comes from `Driver::account` on the process part of the config, read before every attempt
  launches (or from `account` of a failure); a closed account gate parks the run with no launch, so a second run
  of the same account does not start (criterion 10). The model key is known once a failure reported
  `provider_model`; until then a run is admitted on its other keys and parks on a closed model gate only through
  the policy (`gates` of `run.attempt_failed`). A `park` on a key the run does not have closes the gate and starts
  the attempt at once.
- `until` is UTC in the form events carry: `YYYY-MM-DDTHH:MM:SS[.fff]Z`; another form is an invalid decision. A
  closure whose `until` has passed counts as open even before its timer fires; one that has passed already when
  asked for closes nothing (the operator's is rejected, a decision's is ignored).
- The operator closes with `{key, until, reason}` (a closure of `(key, operator)` is replaced) and opens a key:
  that removes every closure of it, the policy's and the core's too. `gate.closed` and `gate.opened` carry no run.
- No half-open probe in the core: when a gate opens, the waiting runs are admitted in creation order within the
  concurrency limits. A failure of an attempt that started before the gate closed on its key is visible to the
  policy (`gates` in `run.attempt_failed`), which parks the run without escalating again — the "absorb" of the
  traffic light (§5.7).

## 8. Instance and service registry

- Fingerprint = SHA-256 of the harness name and the canonical JSON (sorted keys) of the `ProcessPart`; two agents
  with equal process parts share an instance (criterion 13).
- Record: `{id, kind: "instance"|"service", harness or service name, fingerprint or scope key, pid, start_time, pgid,
  state: "starting"|"ready"|"stopping", sessions, idle_since}`. Start and stop are written as intent before the
  effect (invariant 9).
- Acquire, in the transaction that starts the attempt: take a `starting` or `ready` instance of the fingerprint
  with `sessions < sessions_per_instance` (a `starting` one is shared: every launch on it waits for its one start),
  else write a new `starting` record and emit `instance.starting`; the registry gives the id, the adapter puts it
  in `InstanceHandle.id`. Release, when the attempt ends: `sessions − 1`; with no session left the instance stops
  after `instance_idle` (`<id>:idle` deadline; 0 — at once; a new session in that time takes it again). Stop:
  state `stopping`, then the effect; when it is done the record goes and `instance.stopped` is emitted. An
  `exited` event of an instance not `stopping` is a crash: `instance.failed`, the record goes, and the event fails
  every live attempt on the instance (an event with a session goes to that session's attempt only; the one
  exception is the only live attempt on an instance whose session is not known yet, which gets it — a process
  that reports before its session is recorded).
- Confirmed death (invariant 6): a launch first waits until the instances still `stopping` that served its run (or
  the run it continues) are gone by pid + start time (`/proc/<pid>/stat`), up to 30 s; past that the attempt
  fails with `unknown`.
- The process environment of every instance and service holds `AAE_INSTANCE=<id>`: once the instance is stopped
  or has exited, every process still carrying it is killed (the sweep of invariant 7). The daemon is a child
  subreaper ([experiments](experiments.md#descendants-after-kill--9-invariant-7)): every 5 s it reaps the adopted
  orphans that exited, never a child it spawned itself (those are reaped by their own `wait`).
- Health: every 30 s the daemon asks the driver for the health of each instance it started; `Dead` counts as an
  `exited` event of the instance (above).
- Resync (§5.1): every 5 minutes, with the trackers, the driver is asked for the health of every instance not
  `stopping` that has a process (pid + start time for claude and codex). One found `Dead` whose record still holds
  that process and is not `stopping` counts as its `exited` event, as above: a gone process is not ambiguous, so no
  `resync.diverged`. A record that moved on since the check is left alone.
- Services (§11.4, [config](config.md#mcp-servers-and-services-spec-114)) use the same record with kind
  `service`: the MCP name, the key `<scope>#<sha256 of the definition>` (scope `orchestrator` or `project:<p>`), the
  port and the token in the process column, the consumer count in `sessions` (an attempt's services are in
  `attempts.services`). The launch transaction acquires them (a new record emits `service.starting` and the
  `start_service` effect) and rewrites the session's config before the split. Health is the port accepting
  connections: `service.ready`; a process that ends or is not up within 30 s gives `service.failed`, then a restart
  on the same port after the effect backoff of its restarts while it has consumers, else the record goes. Idle,
  operator and recovery stops go through `stop_service`, which ends the process group and sweeps the
  marker; a reload leaves services alone. pid and pgid of the view are null: the process is tracked by its task and marker, not by pid.

## 9. Fakes in the real daemon

The daemon has no test mode. Harness binaries are resolved through mise or taken from an explicit path in config
(§6.5): `[harness.claude] binary = "<path>"`. Contract tests and model-free acceptance scenarios use exactly this:

- the real `claude`/`codex` against a fake provider, configured by ordinary settings (claude: `ANTHROPIC_BASE_URL`
  in the agent env; codex: a `model_providers` entry in the native block);
- or a scripted harness: a test binary in `crates/aae-testkit` that speaks the harness protocol subset the adapter
  uses and plays a scenario file named in the agent env, put in place by `binary = "<path>"`.

The adapters cannot tell either from the real thing; nothing in the product exists only for tests.

## 10. `default-policy` as decisions

The built-in policy expressed in the decisions of 3.2, so that the schema provably carries every reaction of
§11.5. Thresholds are its config parameters (docs/design/config.md); "consecutive" counts are computed from
`attempts` of the event, times from its `ts`, and the gate keys come from its `keys`. A `park` on a key the run does
not have (an exec run has no agent key, a run whose harness reported no account has no account key) becomes a
`resume` after the same wait, except a `park` with no `until` (a lasting `auth_required`, `billing`), which becomes `fail`; a
`fail` names no gate then. Implemented in `crates/aae-plugins`.

| Event / kind | Decision |
|---|---|
| `quota_exhausted` | `park{gate: account, until: retry_at + 60 s, then: resume}`; no `retry_at` → `until` now + 30 min |
| `rate_limited`, `overloaded` with `retried_by_harness` | `park{gate: model, until: now + 10 min, then: resume}` |
| `rate_limited`, `overloaded` without | `resume{after: min(10 s · 2^(n−1), 5 min)}` over `n` consecutive such failures; from the 3rd — the `park` above |
| `auth_required`, `billing` | `park{gate: account, until: now + min(30 s · 2^(n−1), 1 h), then: resume}` over `n` consecutive failures of that kind (postmortem 0001: a proxy or WAF error passes by itself; the attempt after the wait is the probe); never escalated to an operator, and these attempts do not count against the core's ceiling (spec §2.1 item 12); no account key: `fail` |
| `context_overflow`, `transcript_divergence` | `new_session` |
| `config_error` | `fail{gate: agent, until: null}` |
| `harness_crash`, `adapter_crash` | `resume`; 5 consecutive attempts shorter than 30 s → `park{gate: agent, until: now + 5 min, then: resume}`; 3 consecutive failed resumes → `new_session` |
| `daemon_crash` | `resume` |
| `stall` | 1st `resume`, 2nd consecutive `new_session`, 3rd `fail` |
| `runaway_turn` | 1st `new_session`, 2nd `fail` |
| `workspace_failure` | `fail{gate: rig, until: null}` |
| `tool_failure` | `resume` |
| `unknown` | `resume{after: 30 s}` up to 3 consecutive, then `fail` |
| a gate of the run already closed after its attempt started | `park` on that gate, no new closure (absorb); an absorbed failure decides no escalation, the run whose failure closed the gate does, and the attempts ceiling bounds the rest |
| `run.turn_ended_without_outcome` | 1st `continue` with a reminder to call the finisher, 2nd `fail` |
| `run.requires_action`, `run.finishing`, `run.preparing` | no decision |
