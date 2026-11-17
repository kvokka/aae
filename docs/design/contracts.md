# Contracts

The interfaces that later code may not change without a break: the event and its version, the trigger filter and input
mapping, the decisions of orchestrator hooks and their timeouts, the outbox, the driver and tracker interfaces, the run
state machine, the issue gates and the instance registry. References inside this document read "see 3.5"; other
documents are linked. Experiment results are cited from [`experiments.md`](experiments.md), not repeated.

Conventions used below:

- JSON field names are `snake_case`; times are RFC 3339 UTC with milliseconds (`2026-11-12T14:05:00.123Z`);
  durations in config and decisions are strings with a unit (`"500ms"`, `"30s"`, `"45m"`, `"2h"`).
- Identifiers are opaque strings to every consumer. The daemon makes them time-sortable with a type prefix:
  `run_…`, `att_…`, `cor_…`, `ins_…`.
- A **default** below is a config parameter with that default value; a **ceiling** is a core constant that config and policy cannot raise (invariant 16).
- Rust signatures are a shape, not code: crate layout, lifetimes and error plumbing are the code's, as long as the
  operations, types and semantics here stay.

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
| `run` | string or null | the run the event is about; for `bead.*` the run whose attempt made the change (attribution by actor, 5.1), else null |
| `correlation` | string or null | chain id: a run started by hand starts a chain, a run started by a trigger inherits the chain of the event that fired it, a run started from an agent session (6.1) that of the session's run; null for events outside any chain |
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

- `RunView`:
  `{id, formula, step, agent, step_kind: "ai"|"exec"|"drain"|"formula"|"await", state, class, attempt, generation, bead, workdir, depth, parent_run, created_at, started_at}`.
  `step` is the name of the formula step the run is at (6.1), and of an ended run the step that ran last (the steps
  it skipped after it do not count); `agent` and `step_kind` are of that step, `agent` is null
  for a step other than `ai`, `class` is null until `failed`, `bead` is the bead id or null, `depth` is the chain
  depth of 6.1 (0 for a run started by hand), `parent_run` the run whose event triggered it or whose session started
  it. The read API (`GET /v1/runs/{id}`, `aae run show`) gives the `RunView` with `project`, `rig`, `correlation`,
  `inputs`, `waiting_for`, `outcome`, `steps` (the outcomes of the other steps the run went through, by name: the last
  attempt of each; a skipped step has none), `attempts`, `usage` (`{input_tokens, output_tokens}`, summed over every attempt)
  and: `reason` (the veto text, the cancel reason, `pass`, or the failure reason), `gate` (the oldest closure of the
  run's keys while it waits on a gate or is `parked`, else null) and `round` (the round of the formula's checks, 6.1: 1,
  and one more each time a check sends the run back) and `step_attempt` (the execution of the step in the round, an
  integer from 1: a repeat under `max_attempts` adds one, a new entry into the step or a `back_to` return sets 1;
  `attempt` stays the attempt of the session).
- `Failure`: `{kind, text, scope, retry_at, retried_by_harness, source, raw, account, provider_model}`: `kind` from
  see 4.4, `text` set only for `unknown`, `scope` one of `session | instance | account | provider_model | run | rig`,
  `retry_at` time or null, `source` one of `harness | adapter | core`, `raw` the harness text cut to 4 KiB,
  `account` and `provider_model` as reported by the adapter or null.
- `Bead`: see 5.2.
- `Gate`: `{key, until, reason, closed_by: "policy"|"operator"|"core", closed_at}`, `until` null = until removed
  by hand.
- `Attempts`: list of `{n, generation, step, kind: "new_session"|"resume", started_at, ended_at, failure_kind,
  input_tokens, output_tokens}` of the run (`step`: the formula step the attempt ran), newest last, at most 20 entries;
  the tokens are the sums of the attempt's `usage` events.
- `Outcome`: `{result: "pass"|"fail", summary, data}`; `summary` ≤ 4 KiB, `data` any JSON ≤ 64 KiB.

Observed events (addons and clients read them and cannot change what happens):

| Type | `data` |
|---|---|
| `daemon.started` | `{version, pid, interrupted_runs}`; `interrupted_runs` is how many runs recovery moved to `interrupted` |
| `daemon.stopping` | `{reason: "signal"\|"drain"\|"restart"}` |
| `config.reloaded` | `{config_generation, restarted_instances: [id]}` |
| `instance.starting`, `instance.ready`, `instance.degraded`, `instance.stopped` | `{instance: {id, harness, fingerprint, pid, pgid}, reason}`; `pid`/`pgid` null before spawn, `reason` null except `stopped` (`idle`, `config`, `cancel`, `shutdown`, `recovery`, `operator`) and `degraded` (the health text); `instance.ready` with `reason: null` comes again when a degraded instance is well |
| `instance.failed` | `{instance, failure: Failure}` |
| `service.starting`, `service.ready`, `service.stopped`, `service.failed` | same as `instance.*` with `service: {id, name, scope, pid, pgid}` instead of `instance` |
| `run.queued` | `{run: RunView, waiting_for: "concurrency"\|"gate"\|"daemon_stopping"\|null, gate}`; re-emitted when the reason changes |
| `run.started` | `{run}`; once per run, when it first leaves `queued` |
| `run.awaiting` | `{run, deadline}`; the run entered an `await` step and waits for its outcome from outside until `deadline` (6.3) |
| `run.step_skipped` | `{run, step}`; the `if` of the step the run entered does not match, the run goes on without it (6.1) |
| `run.step_finished` | `{run, outcome: Outcome}`; a step that is not the last passed, `run` is still at that step (6.1) |
| `run.state_changed` | `{from, to, attempt, generation, reason}` |
| `run.parked` | `{gate: Gate\|null, then: "resume"\|"new_session"}` (`null`: the daemon is stopping, or the run's agent has as many other started runs as its `max_runs`) |
| `run.resumed` | `{gate_key}` (`"daemon_stopping"` after a park for a stopping daemon, `"concurrency"` after one for the agent's `max_runs`) |
| `run.passed` | `{run, outcome: Outcome, usage}` |
| `run.failed` | `{run, class: "agent"\|"check"\|"hollow_done"\|"infrastructure", reason, outcome, usage}`; `outcome` null unless the agent declared one |
| `run.cancelled` | `{run, reason, usage}` |
| `run.retrying` | `{run, retry, next_retry_at, text}`; the harness retries by itself (`retry` is its own number, 4.3) |
| `run.permission_denied` | `{run, tool, tool_use_id, input, reason}`; the harness or the adapter denied the agent a tool (4.3) and nothing waits on it: `tool` and `tool_use_id` are the harness's (codex: the server request's method and `itemId`), `input` the tool input (null when the harness does not tell it; codex: the request's `params`), `reason` its reason or null |
| `trigger.fired` | `{trigger, cause_seq, new_run, inputs}`; `cause_seq` null for schedules |
| `trigger.skipped` | `{trigger, cause_seq, reason: "overlap"\|"input_unresolved"\|"depth"\|"nothing_ready"\|"rejected", detail}` |
| `trigger.switched` | `{trigger, enabled}`; the operator switched a trigger on a rig off or back on (`POST /v1/triggers/disable`, `/enable`): off, neither its schedule nor its events fire it, a hand firing still does, and the switch outlives restarts and reloads |
| `gate.closed` | `{gate: Gate}` |
| `gate.opened` | `{key, by: "timer"\|"operator"\|"policy"}` |
| `bead.created`, `bead.updated`, `bead.closed`, `bead.ready` | `{bead: Bead, actor, changed: [field]}`; `changed` empty except `updated` |
| `tracker.sync_failed` | `{op, bead, error, next_retry_at}` |
| `addon.hook_failed` | `{addon, hook_type, event_seq, error: "spawn"\|"timeout"\|"exit"\|"output", exit_code, stderr_tail}` |
| `resync.diverged` | `{subject: "process"\|"instance"\|"run"\|"bead"\|"gate", id, expected, observed}` (no automatic effect follows, invariant 20); `instance` also tells an event `seq` that is not the last + 1 (4.3) |

Deciding events (an exec hook of an addon may change what happens; decisions: see 3). The last column is what the
core does when no addon decides, `default-policy` switched off included:

| Type | What a hook may do | No addon decision |
|---|---|---|
| `run.preparing` | change the workdir and the env, add to the prompt, deny the start | start as is |
| `run.attempt_failed` | choose the reaction to the typed failure: resume, new session, park on a gate until a time, fail | `failed{infrastructure}`, reason `no_decision` |
| `run.turn_ended_without_outcome` | return a remark to the agent and go on, or fail | `failed{hollow_done}`, reason `no_decision` |
| `run.finishing` | veto a `pass` with a reason, or return a remark to the agent | the outcome is taken as declared |

Each of them carries `deadline`, the time after which the core applies the "no addon decision" column (see 3.3):

| Type | `data` |
|---|---|
| `run.preparing` | `{run, workdir, env_keys: [name], prompt_append, deadline}`; `env_keys` lists names only, never values |
| `run.attempt_failed` | `{run, failure: Failure, rate_limit: {utilization, resets_at}\|null, attempts: Attempts, gates: [Gate], keys: [string], agent_failures_1h: [{run, kind, at}], deadline}` |
| `run.turn_ended_without_outcome` | `{run, stop_reason, remarks, attempts: Attempts, deadline}` |
| `run.finishing` | `{run, outcome: Outcome, remarks, deadline}` |

`usage` is `{input_tokens, output_tokens}` summed over every attempt of the run; `rate_limit` is the last limit known
for the run's account. `stop_reason` of `run.turn_ended_without_outcome` is the reported one (`completed`, `cancelled`,
`failed`). `remarks` is the number of remarks already returned in this attempt. `gates` lists every closed gate whose
key applies to the run; `keys` lists the gate keys of the run (7), so a policy can name the account, model, agent or rig
gate in its decision. Credentials and environment values never enter an event: the journal is not a secret store.

Every refusal of the core to act on its own writes an event with its reason (`run.queued` with `waiting_for`,
`trigger.skipped`, `gate.closed`, `run.failed` with `reason`, ...): the core never waits silently. A refused API
request is answered with an error instead: `not_found` (404) when it names a run, instance, trigger, project, rig or
formula there is none of, `rejected` (409) when the thing is there but refuses the request as it stands.

## 2. Filter and template language

### 2.1 Paths

A path is a dotted chain of object keys into the event view (see 1.2) or into the run context of a prompt template:
`bead.epic`, `project`, or into the run context of a step's `if` (config.md, "Formula"); the word `data` never
appears in a path. No indexing, no wildcards. A path that a payload of the
event type cannot have is a config error at load time (the payload schemas of 1.3 are known to the loader); so is a
`where` path that ends on an object, which no matcher can equal.

### 2.2 `where`

`where` is a table of `path = matcher`; all entries must match (AND). An empty or absent `where` matches every
event of the type.

| Matcher | Matches when |
|---|---|
| scalar (string, integer, boolean) | the value at the path equals it; if the value is an array, it contains it |
| array of scalars | the value equals any element (for an array value: contains any element) |

A path that is absent or null in the event does not match. There is no negation, no comparison and no regular
expression in the first release; a trigger that needs more uses an `exec` step that decides. Example — a trigger
on beads labelled `auto` is written `where = { "bead.labels" = "auto" }`.

The same table is the `if` of a step, over the run context of a prompt template (2.3, without `remarks` and `agent`)
and `back_from`, the names of the checks that sent the run back into its current round (config.md, "Formula").

### 2.3 Substitution (`inputs` and prompt templates)

One language for trigger `inputs` (`config.md`, "Trigger") and prompt templates (`config.md`, "Agent"): `{{ path }}` with optional
spaces, nothing else — no filters, conditions or loops.

- In `inputs` the root is `event` (`{{ event.bead.epic }}`); a schedule trigger has no event, so any placeholder
  there is a config error.
- A string that is exactly one placeholder takes the JSON type of the value (an integer stays an integer, an
  object stays an object). A placeholder inside a longer string must resolve to a scalar and is rendered as text.
- Tables and arrays in `inputs` are mapped recursively.
- An unresolved path in `inputs` (absent or null at fire time) skips the firing:
  `trigger.skipped{reason: "input_unresolved", detail: <path>}`.
- In a prompt template the roots are `inputs`, `bead` (when the run has one), `rig`, `run`, `project`, `steps`,
  `remarks` (the remarks of the checks that last sent the run back, empty before; config.md, "Formula") and `agent`
  (`agent.<name>`: the text of the agent's file `<name>.md`, the project's over the pack's; config.md, "Agent"); an
  unresolved path is an error before the run starts and fails the run with class `infrastructure`. `steps` holds the
  outcome of each step before the current one: `steps.<name>.result`, `.summary`, `.data.<key>` (the last attempt of
  that step; `data` of an exec step is `{exit_code, stdout, stderr}`); a path to the current or a later step, to
  a step with an `if` (it may be skipped), or to another field, is a config error.
- `{{` that is not a placeholder is written `{{"{{"}}`.

A task input is a string — the id of a bead the run takes up, or the text of a task that gets a root bead — or
`{ next_ready = true, epic = <id or absent> }` ([tracker-br.md](tracker-br.md#beads-and-runs)).

## 3. Orchestrator hooks: decisions and timeouts

### 3.1 Invocation

Built-in addons (`default-policy`, `worktree`) are in-process executors of the same contract: same input value,
same decision value, same timeouts. External hooks are declared in `addon.toml`:

```toml
[[hook]]
on = "run.finishing"           # one event type, observed or deciding
command = ["bin/check", "--strict"]   # relative to the addon directory
timeout = "30s"
```

- Which addons: a hook runs only for an addon that is attached (docs/design/config.md "Addon"). An event of a run
  whose step has an agent goes to the addons that agent attaches, by the run's config snapshot, with that agent's
  params. Any other event (no run, as `bead.*` and `service.*`, or a step of no agent, or a run before its first
  agent step prepares) goes to the addons `[settings]` of the orchestrator and of the event's project attach, with
  the params those two layers give.
- stdin: the event envelope (see 1.1) as one JSON line, then EOF.
- cwd: the addon directory. Environment: `PATH`, `HOME`, `LANG`, plus `AAE_SOCKET` (API socket),
  `AAE_ADDON_DIR` and `AAE_ADDON_PARAMS` (the values of the addon's params in force where it is attached, as a
  JSON object, `{}` without params), and `AAE_INSTANCE` (a marker random per queued hook effect, kept by its descendants).
  Nothing else is inherited from the daemon.
- A program path with a `/` is relative to the addon directory; a bare name is looked up on `PATH`.
- Own process group; at the timeout `SIGTERM` to the group, `SIGKILL` 2 s later.
- stdout is read up to 64 KiB (more is an `output` failure); the last 4 KiB of stderr are kept.
- Result: exit 0 with empty stdout — no objection and no decision; exit 0 with one JSON object on stdout — the
  decision; exit 2 — block with the reason taken from stderr; any other exit, a signal, a timeout, invalid JSON or
  a decision not allowed for the event — a hook failure, reported as `addon.hook_failed`.
- Timeout: default 30 s, ceiling 300 s per invocation.
- Hooks run after commit, outside the core loop; the result enters the core as a message. Hooks of different
  events run concurrently; the hooks of one run are serialized in `seq` order on two queues: deciding events on
  one, observed events on the other (see 3.5), so an observer never delays a decision.
- The hooks of `run.attempt_failed` start once the process the failed attempt stopped is dead, so a hook that reads
  the transcript reads it whole. The wait is at most the stop grace (10 s, after which the process is killed), and
  none when the instance stays up (a shared app-server).
- Delivery is at least once: a hook cut by a daemon crash runs again after restart if its run is still in the state
  that emitted the event (see 3.4); observed hooks always run again. Hooks must be idempotent; `seq` identifies the
  event.
- A hook keeps no state of its own, and the core has no storage for addons: a deciding event carries what a policy
  needs — the attempts with their times, the gates and the run's keys (1.3).

### 3.2 Decisions

`after` is a duration (default `"0s"`). A decision value that is not listed for the event is a hook failure.

| Event | Allowed decisions | Exit 2 means | Hook failure means |
|---|---|---|---|
| `run.preparing` | `{"decision":"proceed","workdir":"/abs/path","env":{"NAME":"value","DROP_ME":null},"prompt_append":"text"}` — every field but `decision` optional; an `env` value is text as it is, a `$` in it no reference (config.md, "Environment values"); `{"decision":"deny","reason":"…"}` | deny | deny (a veto hook) |
| `run.attempt_failed` | `{"decision":"resume","after":"30s"}`; `{"decision":"new_session","after":"0s"}`; `{"decision":"park","gate":{"key":"…","until":"<time>"\|null,"reason":"…"},"then":"resume"\|"new_session"}`; `{"decision":"fail","reason":"…"}`; `gate` is required on `park` and optional on the other three (the gate is closed in the same transaction) | `fail` with the stderr reason | no decision from this addon |
| `run.turn_ended_without_outcome` | `{"decision":"continue","message":"…"}`; `{"decision":"fail","reason":"…"}` | `fail` | no decision from this addon |
| `run.finishing` | `{"decision":"accept"}`; `{"decision":"veto","reason":"…"}`; `{"decision":"remark","message":"…"}` | veto | veto with reason `hook_failed:<addon>` (a veto hook) |

Hooks on observed events may only exit 0 or fail; their failures are reported and otherwise ignored.

### 3.3 Several addons on one event

The addons an event goes to (3.1) are ordered by declaration (orchestrator level first, then project level, each
in declaration order); the bundled addons come after all of them (docs/design/config.md, `[builtin.<name>]`).

- `run.preparing`: hooks run one after another; each receives the event with the earlier modifications applied
  (`workdir`, `env_keys`, `prompt_append`). `env` entries merge by name, `null` removes; `prompt_append` texts are
  concatenated with a blank line. The first `deny` stops the chain and fails the run with class `check`.
- `run.finishing`: every hook runs. Any `veto` → `failed{check}` with the first veto reason; otherwise any
  `remark` → one remark, messages joined with a blank line in addon order; otherwise accept. A `veto` on a declared
  `fail` changes nothing: the run fails with class `agent`.
- `run.attempt_failed`, `run.turn_ended_without_outcome`: the first decision in addon order wins; later hooks are
  not started once a decision exists.
- Every hook done and no decision: the hook runner sends an explicit "no decision" message, and the "no addon decision"
  column of the table in 1.3 applies at once — start as is; `failed{class: infrastructure}`;
  `failed{class: hollow_done}`; accept. A hook that timed out is a hook failure (3.2), not "no decision": on
  `run.preparing` and `run.finishing` it blocks.
- The `deadline` reached with no message from the runner: the hooks count as failed. On the veto events
  (`run.preparing`, `run.finishing`) that blocks — `failed{check}` with reason `hook_deadline`; on the other deciding
  events a hook failure is "no decision from this addon", so the "no addon decision" column applies.

`deadline` of a deciding event = commit time + the sum of the timeouts of its hooks + 5 s (one hook can never hold a run
longer than its own timeout; the runner reports before it unless it is lost). The hooks of a `run.attempt_failed` whose
attempt stopped its instance first wait for that process (see 3.1): their sum counts the stop grace (10 s) too. The
deciding hooks of a run run in order: when hooks of an earlier deciding event of the run are still queued, the sum
counts their timeouts, and their stop grace, too. While a run in `running` waits for the decision on
`run.turn_ended_without_outcome`, no clock of the attempt runs: late session events do not re-arm the silence
deadline. After a daemon restart the deadline of a deciding event still pending counts again from the start of the new
daemon, as the same sum over the hooks queued on the run's deciding queue + 5 s: the hooks run again from their start,
and the time the daemon was down is not theirs (the `deadline` in the event envelope keeps its first value).

### 3.4 Decisions and restarts

The hook effect of a deciding event stays queued until its decision is committed (see 3.5). A decision message names the
event `seq` and the run generation of the event; the core applies it only if the run is still in the state that emitted
that event with that generation, else it is dropped as stale and nothing changes. After a restart the dispatcher runs
the hooks of every deciding event still pending — on `run.preparing`, `run.finishing`, `run.attempt_failed` and
`run.turn_ended_without_outcome` — and the run waits for the same decision meanwhile, its deadline counted again (3.3).
The harness session of a run that ended its turn is gone with the dead daemon (no reconnection, 6.4): a decision that
needs it — `remark` on `run.finishing`, `continue` on `run.turn_ended_without_outcome` — fails the attempt with
`daemon_crash` in the transaction of the decision, and its text waits as the unsettled prompt of the next attempt
(6.4: a resume checks the transcript for it, a new session gets it after the step's prompt); the other decisions apply
as they would have. Recovery drops the pending deciding hooks only of a run whose attempt was live (`starting`,
`running` in a turn, 6.4): they judge the lost attempt, and that attempt's `attempt_failed` queues
its own hooks.

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
- API event streams are not outbox consumers: they read the journal from a `seq` (1.1, [event model](event-model.md)
  "Journal") and get the "gap" error when it is rotated away.

## 4. Driver interface

### 4.1 Shape

The core owns one driver per harness, behind one object-safe trait; the binary lists one `Adapter` per harness and knows
no harness by name ([adapters](adapters.md)), tests wire the scripted driver in (`crates/aae-testkit`). Futures are
boxed; events go to the core through a sink handed to `start_instance`. The adapter translates and executes; it never
retries a turn, parks or decides an outcome (adapter duties in 4.4–4.6 and
[adapters](adapters.md#what-the-core-expects-of-a-driver)): decisions are policy, which lives in addons and config. Its
semantics follow ACP v2: a session is `running` or `idle` (4.3), and a turn fails with a typed failure (4.4). The
interface has no steer in the middle of a turn, no model change inside a live session and no way for a human to join a
live session ([frontends](../api/frontends.md#humans-and-runs)).

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
    fn stop_background(&self, instance: &InstanceHandle, session: &SessionRef) -> BoxFuture<()>; // commands left after the turn; session stays
    fn close_session(&self, instance: &InstanceHandle, session: &SessionRef) -> BoxFuture<()>; // transcript stays

    // transcript
    fn transcript(&self, instance: &InstanceHandle, session: &SessionRef, key: PromptKey) -> BoxFuture<Result<Transcript, Failure>>;
    fn live(&self, session: &SessionRef) -> Option<broadcast::Receiver<serde_json::Value>>;
}
```

### 4.2 Types

| Type | Content |
|---|---|
| `EffectiveConfig` | the merged harness-neutral categories of `config.md` "Settings" (model, effort, env, MCP servers, harness hooks, skills, system prompt, permission mode, addon parts, host inheritance flag) plus the uninterpreted `native` block of the harness, and in a run `targets`: the configs of the subagents its session may call, by type name; produced by config assembly with the provenance of every value |
| `SessionPlan` | `New { workdir, run, attempt, token, actor, bead }` or `Resume { session: SessionRef, workdir, run, attempt, token, actor, bead }`; the adapter needs `run`/`attempt` only to name files in the state directory, and `run`, `token`, the tracker `actor` and the run's `bead` (when it has one) to set the attempt environment (4.6) |
| `Split` | `{ process: ProcessPart, session: SessionPart, fingerprint: Fingerprint }`; the two parts are opaque serializable values of the adapter; `fingerprint` = SHA-256 of the harness name and the canonical JSON of `process` (see 8) |
| `InstanceHandle` | `{ id, pid, start_time, pgid }`; `start_time` is the kernel start time of `pid` (`/proc/<pid>/stat`), pid + start time identify the process ([event model](event-model.md#state)) |
| `SessionRef` | opaque JSON the adapter fills with everything resume needs; the core stores and returns it. claude: `{session_id, config_dir, workdir}` (`config_dir` null: the session inherits the host, and its transcript is under the daemon's `CLAUDE_CONFIG_DIR`, else `~/.claude`); codex: `{thread_id, codex_home}` (the transcript lives in `CODEX_HOME`; `codex_home` is not read yet: a resume takes the home of the process part) |
| `PromptKey` | UUID v5 of `"aae:prompt:<run>/<attempt n>/<prompt n>"` in the URL namespace; the idempotency key of one prompt (see 6.4) |
| `Transcript` | `Missing` or `Present { delivered: bool }` — whether the session exists and whether the prompt with `key` is in it; answered or not is not told apart: a resumed session never has a turn in flight (the old process is always stopped before the resume), so a delivered prompt is reconciled either way |
| `Health` | `Ok`, `Degraded { text }` (the instance stays `ready` and keeps its sessions; `instance.degraded`, 8), `Dead` |

The live tap (`live`) is the direct relay of the live view ([event model](event-model.md#journal)): raw harness
messages of a session, up to 256 buffered, a slow reader loses messages; nothing of it enters the state or the journal.

### 4.3 Normalized events

Every event carries `instance`, `session` (null for instance-wide events) and `seq`, monotonic per instance from 1;
`seq` 0 is an event the core makes itself (the `exited` of an instance the health check found dead), outside the
numbering. The core takes an instance's events in `seq` order: one with `seq` ≤ the last
taken is dropped with no effect, and one that is not last + 1 (a repeat or a gap) is told with `resync.diverged`.
They are input messages of the core, not journal events; the core turns some of them into the run events of 1.3.
Nothing a driver reports is dropped: a retry of the harness, a degraded instance, token usage, an account limit and
the stop reason of a turn and a denied permission reach the policy and the observers through those events.

| Event | Fields | Core use |
|---|---|---|
| `prompt.acked` | `key` | prompt delivery `sent → acked`; ends `starting` |
| `turn.started` | — | watchdog: first event seen |
| `turn.ended` | `stop_reason: completed \| cancelled \| failed` | candidate end of the attempt's work; `failed` follows a `failure` event; `stop_reason` goes to `run.turn_ended_without_outcome` |
| `state` | `state: running \| idle`, `background: bool` | "idle without background" = `idle` and `background == false`; the adapter applies the harness rule. `idle` with `background` after the turn ended and with the outcome declared makes the core call `stop_background` (6.3) |
| `usage` | `input_tokens, output_tokens, rate_limit: {utilization, resets_at} \| null` | the core adds the tokens (increments) to the attempt; keeps the last `rate_limit` per account key (`account:<harness>/<id>`, one row, replaced by each report); still activity for the watchdog |
| `retrying` | `attempt, delay, text` | in `running`: journals `run.retrying` (`next_retry_at` = now + `delay`) and sets the silence deadline to silence + `delay` (an announced pause is not silence) |
| `permission_denied` | `tool, tool_use_id, input, reason` | in every live state: journals `run.permission_denied`; still activity for the watchdog in `running` |
| `activity` | `tool_running: bool` | watchdog only: moves the silence deadline, kept in memory, no transaction; the adapter coalesces it to at most one per second per session |
| `failure` | `Failure` (see 1.3) | ends the attempt with `run.attempt_failed`; a failed single tool call is never sent (the harness hands it back to the model), `tool_failure` means a tool is gone for the whole session (4.4) |
| `exited` | `code, signal` | the instance process is gone: every session on it gets `harness_crash` unless a failure was already reported |

### 4.4 Failure kinds

Every failure kind there is. Background work outside a turn is a state (4.3), not a failure. Emitted by the adapter
unless marked core; default `scope` in the last column (the adapter may report a narrower or wider scope when the
harness says so).

| Kind | Source | Scope |
|---|---|---|
| `quota_exhausted` | adapter | account |
| `rate_limited` | adapter | provider_model |
| `overloaded` | adapter | provider_model |
| `auth_required` | adapter | account |
| `billing` | adapter | account |
| `context_overflow` | adapter | session |
| `config_error` | adapter; core (an `exec` step's `env` names a variable the daemon does not have) | session |
| `harness_crash` | adapter | session |
| `adapter_crash` | adapter (the task reading the harness output panicked: an instance-wide `failure`, then the process group is killed and `exited` follows) | instance |
| `daemon_crash` | core | run |
| `stall` | core | session |
| `runaway_turn` | core | session |
| `tool_failure` | adapter | session (MCP server down for the whole session) |
| `transcript_divergence` | adapter, core (resume found no transcript) | session |
| `workspace_failure` | core | rig |
| `hollow_done` | core | run |
| `unknown` | adapter | session; `text` holds the harness wording |

Success traps are the adapter's: claude `subtype:"success"` with `is_error:true` is a failure, never
`turn.ended{completed}`; an exit with code 0 and no turn result is `harness_crash`.

### 4.5 Capabilities

`Capabilities { sessions_per_instance, instance_idle, versions }`:

- `sessions_per_instance`: claude 1 (a process per session, 8); codex the config value `max_sessions`
  (default 8).
- `instance_idle`: how long an instance without sessions lives on (8): claude 0, codex the config value
  `instance_idle_s` (default 60 s).
- Whether the harness retries 429 itself is the adapter's knowledge: it sets `Failure.retried_by_harness` on the
  failure, and the policy never retries on top of a harness that already did. claude retries 429 and 5xx itself, up
  to 10 times by default (`CLAUDE_CODE_MAX_RETRIES`; 7 seen in the experiment log), so its `rate_limited` and
  `overloaded` are always retried. codex retries 5xx, but a 429 only when the provider sent `Retry-After`, once
  after that pause: a 429 without it is never retried by codex and the policy must do it. Whether a quota failure
  carries `retry_at` shows on the failure itself (claude: subscription login only; codex: yes, from the rate-limit
  update).
  <!-- MEASURE id=claude-self-retries date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/34 note="delays 0.57→32.8 s on a fake API; set by CLAUDE_CODE_MAX_RETRIES" -->
- `versions`: the tested range of the harness binary, `[lowest, highest]` inclusive, the same as `Adapter.tested`
  ([adapters](adapters.md)); outside it the daemon warns at start, and `aae doctor` does.

`account(process)` returns the account id the gates use (7): a stable hash of the credential source of the
process part, never the credential itself; an `env` reference counts as written (`${NAME}`), so the id says where
the key comes from, not what it is.

### 4.6 Harness specifics the contract relies on

| Concern | claude | codex |
|---|---|---|
| Process and session | process per session; everything is in `ProcessPart`, so the fingerprint is unique per session; `start_session` and `resume_session` only bind the reference (the `--session-id`/`--resume` flag is part of the spawn) | one app-server per fingerprint over stdio; `ProcessPart` = `CODEX_HOME`, `HOME`, skills (the session's and its subagent targets', links in its own `CODEX_HOME/skills`; with host inheritance that home links the host's `auth.json` and `config.toml`), the default `sandbox_mode`, provider and key variable, process environment; `SessionPart` = cwd, model, effort, instructions, MCP, hooks with `bypass_hook_trust`, the agent roles of the subagent targets, command environment ([experiments](experiments.md#config-and-credential-delivery-experiments-4-and-5)) |
| Prompt key | user message `uuid` | `clientUserMessageId` |
| `prompt.acked` | `command_lifecycle{queued}` with our uuid | `turn/start` response with a turn id |
| Turn end | `result` of a turn, then "idle without background" by the three-condition rule ([experiments](experiments.md#idle-without-background-both-signals-in-one-run)) | `turn/completed` only; thread status `idle` does not end a turn (below) |
| Cancel | `control_request{interrupt}`; the following `aborted_tools` result is `turn.ended{cancelled}` | `turn/interrupt` |
| Resume | full flag set again on every `--resume` | full thread config again on every `thread/resume`, after app-server closed the thread (`thread/unsubscribe` until `notLoaded`, at most 10 s, else `transcript_divergence`): a thread it still holds keeps the old config |
| Attempt environment | process environment | `shell_environment_policy.set` of the thread; a hook runs with the process environment, so each hook command starts with `export` of the same variables |
| Stop of what a session left | `control_request{stop_task}` on every `local_bash` task of the last `background_tasks_changed` (a `local_agent` subagent is left alone); the `task_notification{stopped}` that follows is not a wake-up | `thread/backgroundTerminals/clean{threadId}` (experimental API): only that thread's commands go, each one's `item/completed` follows; a command that detached itself goes on `close_session`, killed by the thread's attempt token |
| Transcript check | the session file under `config_dir`: user record with `key`, answer after it | `thread/read` with turns: `userMessage` with `clientId == key`, completed turn after it |

Every adapter runs each session process (claude) or instance process (codex) in its own process group, and takes
the end of a turn from the harness's authoritative signal, never from the answer to a request.

claude (`crates/aae-claude`):

- Transport: `claude -p` with stream-json input and output, a given `--session-id`, resume by `--resume`. The
  session config is flags and files in the state directory: setting sources (the host inheritance switch),
  `--settings` with the hooks, `--mcp-config`, `--agents` with the definitions of the subagent targets,
  `--plugin-dir`, system prompt, model, effort, permission mode; the model is passed explicitly on resume too
  ([experiments](experiments.md#config-and-credential-delivery-experiments-4-and-5)), and so is `--agents`.
  `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1` leaves no built-in subagent type. The generated files
  (`settings.json`, `mcp.json` with the tokens of services, `agents.json`, the skills plugin) are a process's own, in
  `<state>/claude/<session>/<instance>/`, and are removed when it exits; a resume writes them again. The config
  directory of an isolated session (`<state>/claude/<session>/config/`, its transcript) stays: no driver call says
  that a session will never be resumed, so it is removed by hand.
- Signals read: `system/api_retry` (`retrying`), `rate_limit_event` with the reset time, the assistant message's
  `error`, the turn `result` with its error flag, failure fields and `permission_denials`, `system/permission_denied`, `session_state_changed`,
  `background_tasks_changed`, `task_notification`, and the `tool_progress` heartbeat of a long tool every 30 s
  ([experiments](experiments.md#heartbeat-of-a-long-tool)).
  No heartbeat comes from inside a subagent; the watchdog's budgets allow for that (invariant 15).
- The reset time of a limit comes only with a subscription login; with an API key a limit is a plain `rate_limit`
  without it. The shape of the event when a real quota runs out has not been seen live (the claude half of
  criterion 10 stays open).
- Nothing is printed before the first prompt: `init` comes after the prompt and on every turn, so waiting for
  `init` before the prompt deadlocks. The number of turn results is not the number of prompts: a background task
  wakes the session into a turn of its own.
- Subagents of claude run in the background by default, so a turn's `result` does not end its work: the end is
  "idle without background" (4.3). While a `local_agent` task lives, the state stays `running` after the `result`
  and the subagent's steps come out of the turn only as its commands end
  ([experiments](experiments.md#a-background-subagent-after-the-result)); so the adapter reports `State{Idle,
  background: true}` right after such a `result` (the background budget, 6.1) and no activity outside a turn. A retry,
  rate limit or denial the subagent meets outside a turn is told with `State{Idle, background: true}` after it, so it
  does not shorten that budget to the silence one.
- When the daemon dies, the turn goes on unobserved; its outcome is learned from the transcript and the state of the
  repository (4.2 `Transcript`, 6.4). A `--resume` reaches `init` in 0.30 s, so a resumed attempt costs little; a
  planned restart drains first ([invariants](invariants.md#recovery)).
  <!-- MEASURE id=claude-resume-time date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42 note="one-turn session, haiku, n=6; through the ACP adapter on 2.1.321 0.4–0.5 s (reports/14); resume of a long transcript not measured" -->
- A cold start takes 0.25–0.35 s to `init`; the rest is the model's time to the first token, so no warm pool.
  <!-- MEASURE id=claude-coldstart-init date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42 note="haiku, n=5–7, medians from spawn; re-measure on the role models" -->
- `CLAUDE_CODE_RETRY_WATCHDOG=1` is never set in a session's environment: claude then waits out a 429 or 529 in
  silence (one `api_retry{max_retries: 300}` with a pause of an hour), which the core's watchdog reads as a stall.
- An MCP server that `init` lists as `failed` or `needs-auth` is a `tool_failure` of the session: nobody logs a
  server in after the start.
- claude runs with `--permission-prompts none`: whatever would prompt is denied by claude itself and the model sees
  the denial; the permission mode decides the rest. The mode is `bypassPermissions` unless `native.claude` sets
  `permission_mode` or `permissions.defaultMode` ([config](config.md#permissions)); as root claude exits before
  `init` under that mode unless the session's `env` sets `IS_SANDBOX=1`. Any `control_request` of the CLI
  gets an error `control_response` at once, so nothing waits. claude prompts for some commands even under
  `bypassPermissions` (seen on 2.1.325: a Bash heredoc into a directory outside the workdir); such a command is denied
  at once.
- A denial comes as `system/permission_denied` (`tool_name`, `tool_use_id`, `decision_reason_type`, no tool input)
  and again in the `permission_denials` (`tool_name`, `tool_use_id`, `tool_input`) of the turn's `result`; the adapter
  sends one `permission_denied` per `tool_use_id` in a turn, from whichever comes first (`reason` is
  `decision_reason_type`, `input` only from the `result`). A denial without `tool_use_id` is sent as it is. The
  `result` list was seen live (2.1.322); the shape of `system/permission_denied` is read from the code of 2.1.326.

codex (`crates/aae-codex`):

- Transport: aae's own `codex app-server` over stdio; the user's own codex daemon is never used. Protocol types are
  not generated from the schema: the adapter reads the fields it uses by hand, and the contract suite
  ([harness updates](../harness-updates.md)) catches a change on every codex update.
- Signals read: `error` with `willRetry` and `codexErrorInfo`, `turn/completed` with its status,
  `thread/status/changed`, `account/rateLimits/updated` with the reset time, `rateLimitReachedType` and
  `spendControlReached`.
- Thread status `idle` comes before `turn/completed`, and a command that outlives its turn (`exec_command` yields while
  it runs) runs while the status is `idle`: the turn ends `idle` with `background`, and its late `item/completed` makes
  the thread idle without background (contract scenario 14). While the provider is silent before its response headers
  there is no notification at all and codex has no timeout of its own: the core's watchdog owns it.
- The thread starts with `approvalPolicy = "never"` unless `native.codex.approval_policy` sets another
  ([config](config.md#permissions)); under another policy the adapter declines command and file-change approvals
  (`{"decision":"decline"}`, the turn goes on) and answers every other server request with an error; each refused
  request of a thread is a `permission_denied`.
- Every MCP server aae gives a thread is `required`: one that does not start fails `thread/start` or `thread/resume`
  (`required MCP servers failed to initialize`), which is a `tool_failure` before the first prompt. A server of the
  rig or the host that does not start comes as `mcpServer/startupStatus/updated{status: failed}`: a `tool_failure`
  of the thread, told at once in a turn and otherwise held until the thread is known to the core (the end of
  `start_session`/`resume_session`, the next `send_prompt` or `turn/started`, whichever comes first).
- Two `turn/start` on one thread are not queued but merged: the second becomes a steer of the first. The core sends
  a session's next prompt only after its turn ended (6.2), so this never happens.
- Hooks run with the environment of the app-server process, not of the thread (4.6 table, attempt environment); so
  the attempt token stands in the `export` of the hook command and is visible in its argv, which is accepted.
- Subagent targets are agent roles of the thread: `config.agents.<type> = {description, config_file}`, over the
  `agents` table of the native block, which a thread's `agents` would replace. codex takes a role only from a
  file, so the adapter writes each (`developer_instructions`, `model`, `model_reasoning_effort`,
  `[[skills.config]]` turning off the process's skills the target has not; any other key drops the role) as
  `<state>/codex/roles/<hash of its content>.toml` before `thread/start` and `thread/resume`. Roles are not in
  the fingerprint: sessions with different targets share an app-server. The multi-agent version is codex's
  choice by the model. A child thread's notifications carry its own thread id and belong to no session
  ([experiments](experiments.md#codex-subagent-roles-experiment-10)).
- app-server syncs its plugins over the network by itself (a `git fetch` of the vendor's plugins repository, seen
  on 0.168), as a child that carries the instance marker. It outlives a killed daemon until a restarted one sweeps it,
  and through it a session without a model still touches the network.
- app-server is marked `[experimental]` and codex ships often; see [harness updates](../harness-updates.md).

The core does not rely on the harnesses' own watchdogs, whether they fire before its own or after. claude cuts a silent
response stream after 180 s by bytes (300 s behind `ANTHROPIC_BASE_URL`) and 300 s by events, and a silent background
subagent after 10 min (vendor documentation for 2.1.322, not measured). codex gives up a stream silent for
`stream_idle_timeout_ms` (300 s) only after `stream_max_retries` (5) reconnects, about 30 min in the worst case (an
estimate).

A failed turn is classified by the structured fields of the harness, the first field that names a kind deciding;
text is a narrow fallback, and a value no rule knows is never an error: the next field decides, and what none tells
apart is `unknown` with the harness words (or `field=value`) in `text` and the message in `raw`. A refusal of the
model by policy and a network break without a status are `unknown` on both harnesses.

- claude, in order: `startup_failure_reason` of a claude that could not start (sessions run with
  `CLAUDE_CODE_STARTUP_FAILURE_RESULTS=1`, so it comes as a `result` and not only on stderr; a reason that may pass by
  itself, such as remote settings that did not arrive, is `unknown`); the result's `subtype` and `terminal_reason`
  (turn, budget and structured-output limits `runaway_turn`, `prompt_too_long` and `blocking_limit`
  `context_overflow`); the assistant message's `error` of the turn; `api_error_status` (401 and 403
  `auth_required`, 429 `rate_limited`, 404 `config_error`, 408, 5xx and 529 `overloaded`); then two text rules
  (`Not logged in` and, for a rate-limited turn, `You've hit your`). A `rate_limit_event` of the turn refines only a
  rate-limited turn: `credits_required` makes it `billing`, `rejected` `quota_exhausted` with its reset time. The
  first `api_retry` with 401, 403 or a logged-out `error` (`authentication_failed`, `oauth_org_not_allowed`,
  `cloud_credential_error`) is `auth_required` at once.
- codex: the variant of `codexErrorInfo` and its `httpStatusCode` (statuses as for claude; `flexUnavailable` and
  `policyCapacityUnavailable` `overloaded`, `sandboxError` `config_error`). `usageLimitExceeded` stands for a used-up
  plan, a missing plan, no credit and a spend cap alike: it is `billing` when the last rate-limit snapshot says
  `rateLimitReachedType` `workspace_*` or `spendControlReached`, or its message is one of codex's plan and credit
  messages, and otherwise `quota_exhausted` with the reset of the exhausted window. A retry with 401 or 403 is
  `auth_required` at once; a JSON-RPC answer `-32001` (the app-server's request queue is full) is `overloaded`.

The tables are owned by the adapters and pinned by the contract tests ([harness updates](../harness-updates.md#how-the-suite-is-built)).

## 5. Tracker interface

### 5.1 Operations

The trait has the seven operations the core calls; an operation is added only when a caller needs it.
Every call names the actor: `aae:<run id>` when the daemon acts for a run, `aae` for
the daemon's own reads. Sessions and exec steps get `BD_ACTOR=aae:<run id>:<attempt n>` and the database path in
their environment ([experiments](experiments.md#br-as-an-event-source-experiment-3)). An actor of any other form is a
human or an outside tool and starts a new correlation chain (`correlation`, 1.1).

```rust
trait Tracker: Send + Sync {
    fn claim(&self, owner: &str, id: &str) -> BoxFuture<Result<Bead, TrackerError>>;
    fn release(&self, owner: &str, id: &str) -> BoxFuture<Result<(), TrackerError>>;   // conditional
    fn close(&self, owner: &str, id: &str, reason: &str) -> BoxFuture<Result<(), TrackerError>>; // conditional
    fn ready(&self) -> BoxFuture<Result<Vec<Bead>, TrackerError>>;          // every ready bead, tracker order
    fn under(&self, root: &str) -> BoxFuture<Result<Vec<String>, TrackerError>>;     // ready ids below root
    fn changes(&self, after: i64) -> BoxFuture<Result<Vec<Change>, TrackerError>>;           // after a seq
    fn create(&self, actor: &str, key: &str, title: &str, description: &str)
        -> BoxFuture<Result<Bead, TrackerError>>;                               // once per key
}
```

`release` and `close` carry no generation (invariant 4): the condition `assignee == aae:<run id>` is the same check,
because a run id is not reused within one state directory (it is a counter there: a fresh state directory counts from
`run_1` again) and a run owns at most one bead once. A claim answer carries the generation it was made for and is
dropped unless the run still owns the bead in that generation and still waits for it in `preparing`.

`create` makes an open epic without labels: the root bead of a run whose task is a text that names no bead the core
knows; a text that is the id of a bead returns that bead instead. It is idempotent by `key`: a repeat (an effect run
again after a restart) returns the bead of the first call, never a second bead.

`under` gives the ids of the ready beads below `root` at any depth, in the tracker's order: the beads a `drain` step
gives out (`config.md`, "Formula"). A bead's `epic` names only its parent, and an epic with open children is not ready, so the
ancestors of a grandchild cannot be told from `ready()`; the tracker answers for the whole subtree. An epic becomes a
member once its own children are closed and it is ready itself. A `root` the tracker does not have is `NotFound`.

`changes` is pull-only; the adapter also owns a watcher that sends a "changed" message to the core, and the core then
calls `changes` from its stored cursor (and on start and after recovery without waiting for a signal). One effect on the
queue `tracker:<project>` reads `changes(cursor)`, then `ready()`, then `under` for the root of each run in a `drain`
step; the core turns the writes past the cursor into events, moves the cursor (table `tracker_cursors`, never back) and
diffs the ready set. The first read of a project's tracker takes its whole history as the starting cursor and raises no
`bead.created|updated|closed` for it; every bead ready at that read is new to the set and raises `bead.ready`. A write
whose actor is `aae:<run id>[:<attempt n>]` carries that run and its correlation; `bead.ready` carries those of the last
such write in the same read (the write that freed the bead is usually that one); the rig of a bead event is the one
whose prefix is the bead's id up to its last `-`, absent for a bead of the root. The daemon gives a project a tracker when `<project>/.beads` exists, and every session
and exec step of the project gets `BEADS_DIR` (layer: project) and `BD_ACTOR=aae:<run id>:<attempt n>`; the core sets
the actor of an exec step and hands that of a session to the adapter in the session plan.

### 5.2 Types and errors

- `Bead`: `{id, title, description, status: "open"|"in_progress"|"blocked"|"deferred"|"closed", priority, labels,
  assignee, epic, created_at, updated_at, closed_at}`. `epic` is computed by the adapter (br has no such field,
  [tracker-br](tracker-br.md#limits-of-br-the-adapter-is-shaped-by)). The rig of a bead is the prefix of its id (no label).
- `Change`: `{seq, kind: "created"|"updated"|"closed", actor, changed: [field], bead: Bead}`; `seq` orders the writes
  and is the cursor of `changes(after)`, `bead` is the bead when the change is read; the core maps it to `bead.created`,
  `bead.updated`, `bead.closed`, and derives `bead.ready` by diffing `ready()` with the last ready set it keeps in the
  state (a bead is "ready" the first time it enters the set, `config.md` "Trigger").
- `TrackerError`: `NotFound`; `Claimed { holder }` (claim of a bead someone else holds — told by the state of the
  bead after the call, not by the exit code text, [tracker-br](tracker-br.md#limits-of-br-the-adapter-is-shaped-by)); `OwnerMismatch { holder }` (conditional release or close
  found another owner, nothing changed); `Unavailable { text }` (transient: retried by the effect queue with
  backoff, `tracker.sync_failed` on every failure); `Invalid { text }` (permanent).
- `release` and `close` are conditional on `assignee == owner`; br 0.7.6 has neither a conditional close nor a release,
  so the adapter reads, compares and writes. A `close` of a parent with open children leaves it open: br refuses it, and
  the adapter releases the bead instead, so it does not stay `in_progress` behind a terminal run. The window between
  read and write is open only to humans (the daemon is the only claimer, invariant 3). An owner found changed before the
  write is `OwnerMismatch`, reported as `resync.diverged`; a human write that lands inside the window shows up only as a
  `bead.updated` of the next `changes` batch, until the resync of the tracker compares owners.
- The state of the orchestrator is the truth of ownership; the assignee in br mirrors it (6.2).

## 6. Run

### 6.1 States

| State | Meaning | Deadline (default) | On deadline |
|---|---|---|---|
| `queued` | not started: waits for a concurrency slot or an open gate; the reason is visible (`run.queued`) | `queue_timeout` 7 d | `failed{infrastructure, queue_timeout}` |
| `preparing` | the bead claim confirmed, `run.preparing` hooks running, workdir and environment being fixed, snapshot of the effective config written | `start_timeout` for the claim, then the hook deadline (see 3.3) | the hooks count as failed → deny → `failed{check, hook_deadline}` |
| `starting` | an attempt: instance acquired, session started or resumed, first prompt sent, waiting for `prompt.acked` | `start_timeout` 120 s | attempt failure `stall` |
| `running` | the agent works (or the `exec` command runs, or the `drain` step gives out beads, or the `await` step waits for its outcome) | step `timeout` of the formula (default 45 m; `drain` and `formula` 24 h) per attempt → `runaway_turn` (`await`: 168 h → `failed{check, await_timeout}`, or `pass` with `fail_on_timeout = false`); silence budgets (an announced `retrying` pause adds to the limit): first event after ack 15 min, model thinking 15 min, tool running 40 min, background work after the turn 120 min → `stall` | attempt failure |
| `finishing` | outcome declared and the session idle without background; `run.finishing` hooks running, then the step check script of `step_check_script_path` if the hooks accepted a `pass` (no decision awaited) | hook deadline; for the script the step `timeout` plus a margin, a lost script counting as not passed | the hooks count as failed → veto → `failed{check, hook_deadline}` (a declared `fail` stays `failed{agent}`); script exit 0 → the next step or `passed`; 75 → `failed{check}` with no repeat and no `back_to` round; any other result is an execution that did not pass: under `max_attempts` the run repeats the step (`run.step_finished` with outcome `fail`, then `preparing` with `step_attempt` + 1), else `failed{check}` (`failed{agent}` if the step said `fail`); a script that cannot start → `failed{infrastructure}`, one whose `env` names a variable the daemon does not have (it is not started) → `failed{infrastructure}` with reason `config_error: check script: …` naming the key and the variable, and no `failure_kind` on the attempt, which did not fail |
| `interrupted` | the current attempt ended with a failure (or by a daemon restart); `run.attempt_failed` is deciding | hook deadline, then the decision's `after` | no decision → `failed{infrastructure}` |
| `parked` | a started run waits for a gate or a slot of its agent; no harness process | `park_timeout` 7 d from parking, or the gate's `until` if later (a gate with `until` opens by its timer first) | `failed{infrastructure, park_timeout}` |
| `passed`, `failed{class}`, `cancelled` | terminal | — | — |

The thresholds named here (`queue_timeout`, `start_timeout`, the silence budgets, `park_timeout`)
are the `[watchdog]` table of the orchestrator config, with these defaults (config.md, orchestrator).

`interrupted` is every attempt failure waiting for its decision, a daemon restart among them,
so that the state machine has one place where `run.attempt_failed` is decided.

- The class of `failed` tells the cause: `agent` (the finisher said `fail`), `check` (an addon's veto or deny, or a step check script that refused),
  `hollow_done` (the turn ended without an outcome), `infrastructure` (attempts ran out or the policy ended the run).
- `queued` has not started: it waits for a concurrency slot or an open gate. `parked` has started and
  waits for a gate or a slot of its agent with no harness process.
- A run belongs to a project and a place: a rig, or the project root under its name (config.md "Project"), named
  when it is created (API, CLI, MCP) or by the trigger that fires there. A start that names no rig takes it from the
  id prefix of the bead an input names by id, and runs in the root when the id matches no rig or no bead is named. An
  unknown rig, or a bead that still carries a `rig:` label, is refused before the run exists. Its workdir is the place's
  directory, or a worktree of it with `worktree = true` in the formula (config.md); in a directory without git that
  run fails (`deny` of `worktree`). The session of an ai step works in the run's workdir, but that of an agent of
  scope `root` works in the root's directory when the run is on a rig. aae does not serialize runs by directory: two
  runs may work in one directory at once. Keeping parallel writers apart in one rig is the user's choice:
  `worktree = true` gives each run its own directory, and `pool` and `max_runs` bound how many run at once.
- Concurrency is bounded per orchestrator (`max_runs`, default 8) and per agent (`max_runs` in `agent.toml`). A run
  that started keeps its orchestrator slot from step to step; a step of an agent at its limit parks until a run of
  that agent ends.
- A run goes through the steps of its formula in order (config.md, "Formula"); `preparing` to `finishing` are
  states of the current step, named by `step` of the `RunView`. A `pass` of a step that is not the last, once
  accepted, ends the attempt with `run.step_finished` and moves the run from `finishing` to `preparing` of the next
  step in the same transaction, so `run.preparing` is decided for every step that runs and `run.started` comes once.
  A step whose `if` does not match on entry is skipped in that transaction: `run.step_skipped`, no `run.preparing`, no
  attempt, and the run enters the next step or ends. The `pass` of the last step is `passed`, with the outcome of the
  last step that ran (a run whose every step is skipped is `failed{infrastructure}`, `every step was skipped`; one
  sent back into a round that skips every step is `failed{infrastructure}`, `every step of round <n> was skipped`); a
  failure of a step fails the run, whose outcome is that of that step, unless the step is a check (`back_to`): then the run goes on to the next check of a row, or back to an earlier step for the
  next round, the same way from `finishing` to `preparing` (config.md, "Formula").
- An attempt starts a new session or resumes the previous one of its step, and has its own generation. Each prompt
  step has a session of its own: the first attempt of a step the run went back to resumes it with the step's rendered
  prompt (6.4 is the prompt of a resumed retry). A `new_session` attempt gives up the step's earlier sessions: a later
  `resume` resumes only a session started since, else (the new session failed before it was recorded) it starts
  another new one. Attempt numbers run on across the steps of the run, so the token of
  an attempt of an earlier step is stale.

Ceilings of the core (invariant 16): 20 failed attempts per run (the attempts of every step together), not counting the
attempts whose `auth_required`, `billing` or `quota_exhausted` failure the decision parked on the run's own `account:`
gate, closed by it (a wait on the account does not spend the ceiling; the attempt is marked
`waited` in the state; a park on a gate that does not close, `until` passed, or any other park counts), 10 remarks per
attempt (`continue` after a hollow turn and `remark` after `run.finishing` both count). A repeat of a step under `max_attempts` spends neither ceiling. The eleventh fails the run with
reason `remarks_ceiling` and the class the event would give: `hollow_done` for `continue`, `check` for `remark`. Between
turns (from idle without background to the next prompt) the step `timeout` pauses and resumes with the time it had
left. Trigger
depth: a trigger does not start a run deeper than 10 (`trigger.skipped{depth}`); the limit is fixed, with no
setting. A run started on `bead.ready` keeps the depth of the run that caused it, with no level added: a bead made ready
is the next work of the tracker, not a run the parent started, so a long epic stays legal. A run started from an agent
session (`aae run start` with `AAE_RUN` and `AAE_ATTEMPT_TOKEN` in its environment, `parent` of `POST /v1/runs`) is a
child of the session's run: the token must be that of the parent's live current attempt, as for the finisher (6.3),
else the start is `rejected` ("stale attempt"); the child takes the parent's correlation and `parent_run`, one level
deeper, under the same ceiling of 10 (`rejected` beyond it). Cancelling a run cancels its live children started from its
sessions, with reason `parent_cancelled` (runs its events triggered are not its children to cancel); a pass or a failure
of the parent leaves them alone.

### 6.2 Transitions

| From | On | To | Effects after commit |
|---|---|---|---|
| — | create (API, CLI, MCP, trigger) with an idempotency key | `queued` | — (with a task input that names a bead the core knows, the run is recorded as the bead's owner in this transaction, invariant 3: a bead another live run holds refuses; `{ next_ready = true, epic }` takes the first bead of the last ready set, in the tracker's order, that is in the epic, of the run's rig or of no rig, without an owner and without a closed gate, passing over (reply `passed_over`) a bead whose `harness:` or `agent:` label the run cannot take; none → the reply `{"nothing_ready": true}`, for a trigger `trigger.skipped{nothing_ready}`; the input then holds the bead id; any other text gets its bead on `preparing`, tracker-br.md) |
| — | create with `continue_from: <run>` (a terminal run with a session; a session is never reused implicitly) | `queued` | in the workdir of that run; the first attempt resumes that run's last session with the formula prompt; a later `resume` uses the run's own session, or that one while it has none and started no new one; a harness other than the agent's → `failed{infrastructure}`. Only the first step continues that session: a formula whose first step is not `ai`, or has an `if`, cannot be continued, and the later steps start sessions of their own |
| `queued` | slot free, no gate of the run closed | `preparing` | with a task text that names no bead the core knows: the tracker creates the root bead, or returns the bead the text names (`create`, key kept in the effect), the run owns it and its input takes the id; with a bead: the tracker claim as owner `aae:<run>` (queue `tracker:<project>`), and the prompt renders once the claim returned the bead; `run.preparing` hooks |
| `preparing` | decision `proceed` (or no hooks) | `starting` | attempt 1: instance acquire, `start_session`, `send_prompt` |
| `preparing` | the `if` of the step does not match, on entry | `preparing` of the next step, or the end of the run | `run.step_skipped`; no hooks, no attempt |
| `preparing` | `deny`, template error, claim error `Claimed` / `NotFound` / `Invalid`, `create` error other than `Unavailable`, no claim or `create` answer within `start_timeout` | `failed{check}`, `failed{infrastructure}`, `failed{infrastructure, bead_claimed / bead_not_found / bead_invalid}`, `failed{infrastructure, bead_not_created: <error>}`, `failed{infrastructure, tracker_unavailable}` | — |
| `starting` | `prompt.acked` of the first prompt | `running` | — |
| `running` | turn ended, idle without background, outcome declared | `finishing` | `run.finishing` hooks |
| `running` | turn ended, idle with background, outcome declared | `running` | the adapter stops the commands the session left (`stop_background`); `finishing` follows on idle without background |
| `running` | turn ended, idle without background, no outcome | `running` | `run.turn_ended_without_outcome` hooks; `continue` → next prompt in the same attempt; `fail` or no decision → `failed{hollow_done}` (with the hook reason for `fail`) |
| `preparing` (exec) | decision `proceed` (or no hooks) | `running` (no session to start: the command is spawned at once) | the command runs in the workdir with `PATH`, `HOME`, `LANG` of the daemon (its whole environment with `inherit_env`), the `env` of `[exec]` and of the step with its references resolved (config.md, "Formula"), the `env` of `run.preparing`, `AAE_SOCKET` (the API socket), `AAE_INPUT_<NAME>` per input and, per earlier step, `AAE_STEP_<NAME>_RESULT` (`pass`\|`fail`), `AAE_STEP_<NAME>_SUMMARY` and `AAE_STEP_<NAME>_DATA` (the `data` as JSON, when there is one); `<NAME>` is formed as for inputs (config.md, "Formula") |
| `running` (exec) | exit 0 / other code or a signal | `finishing` with outcome `pass` / `fail`, `summary` `exit <code>` or `signal <n>`, `data` `{exit_code, stdout, stderr}` (last 4 KiB each) | `run.finishing` hooks |
| `preparing` (drain) | decision `proceed` (or no hooks) | `running` (an attempt with no process, `harness = drain`) | a read of the tracker; the config snapshot is `{drain: <formula>, pool, on_item_failure}` |
| `running` (drain) | a read of the tracker (`Ready`) | `running` | while fewer than `pool` children are alive (queued or running) and no bead of this entry failed under `soft_stop`/`hard_stop`, each ready bead under the run's bead, at any depth, not given in this entry of the step (a restart starts a new attempt, not a new entry): the create of the first row with the bead as input, no rig, the run's correlation, `depth + 1`, `parent_run` the run (a refusal leaves the bead without a run); a run in a drain step holds no concurrency slot |
| `preparing` (formula) | decision `proceed` (or no hooks) | `running` (an attempt with no process, `harness = formula`) | the child run of the step's formula is created, or found if a restart cut the attempt that made it |
| `running` (formula) | the child cannot start: refused (depth, a bead held by another run) or `next_ready` finds no ready bead (`nothing_ready`) | `failed{infrastructure}`, no attempt left to `max_attempts` | — |
| `preparing` (await) | decision `proceed` (or no hooks) | `running` (an attempt with no process, `harness = await`) | `run.awaiting {run, deadline}`; the config snapshot is `{fail_on_timeout}`; a run waiting in the step holds no concurrency slot |
| `running` (await) | an outcome for the step (`POST /v1/runs/{id}/resolve`, `aae run resolve`) | `finishing` with that outcome | `run.finishing` hooks; an outcome for another step or a run not waiting is refused (`rejected`) and changes nothing |
| `running` (await) | the deadline, no outcome | `failed{check, await_timeout}`, no return to `back_to`; with `fail_on_timeout = false` `finishing` with `pass`, summary `timeout` | — |
| `running` (formula) | the child ends | `finishing` with the child's outcome (or its reason, without one) | `run.finishing` hooks |
| `running` (formula) | the run fails in the step (`runaway_turn` with no retry) | `failed` | the live child is cancelled, reason `parent_failed` |
| `running` (drain) | a read after a bead of this entry failed under `hard_stop` | `running` | the cancel of each alive child, reason `drain_hard_stop` (its children go with it) |
| `running` (drain) | a read that gave nothing, with no child alive and no later read queued | `finishing` with outcome `pass`, `summary` `<n> closed, <n> failed, <n> refused`, `data` `{closed, failed, refused: {<bead>: <reason>}}` of this entry's beads; after a stop `fail`, `summary` `stopped on <beads>: <n> closed, <n> failed, <n> not started`, `data` adds `stopped_on` and `not_started` | `run.finishing` hooks |
| `running` (exec) | exit 75 / timeout / spawn error / an `env` reference to a variable the daemon does not have (the command is not spawned) | `interrupted` with failure `unknown` (`text` `exit 75 (EX_TEMPFAIL)`, scope `run`) / `runaway_turn` / `workspace_failure` / `config_error` (`text` names the key and the variable) | `run.attempt_failed` hooks; every attempt runs the command again |
| `starting`, `running` | `failure`, `exited`, deadline, exec exit 75 | `interrupted` | stop the session (cancel, then stop the instance if it is the session's own, then sweep descendants); `run.attempt_failed` hooks |
| `finishing` | accept of a `pass` of a step that is not the last | `preparing` of the next step | `run.step_finished`; the next step's prompt renders (the workdir and the bead claim stay) and its `run.preparing` hooks run |
| `finishing` | accept / veto / remark | `passed` or `failed{agent}` / `failed{check}` / `running` | bead close or conditional release; on remark the message goes as the next prompt of the same attempt, the declared outcome is cleared, the token stays valid. A remark for a step other than `ai` is not allowed (hook failure) |
| `interrupted` | `resume` / `new_session` | `starting` after `after` | new attempt: `generation + 1`, new token; `resume` first checks the transcript (invariant 17), `Missing` → attempt failure `transcript_divergence` |
| `interrupted` | `park` | `parked` | gate closed in the same transaction |
| `interrupted` | `fail`, no decision, attempts ceiling | `failed{infrastructure}` | gate closed if the decision carries one |
| `parked` | every key of the run open, a concurrency slot free (in creation order with the queue) | `starting` (`then` of the park decision); `run.resumed` | new attempt |
| `preparing`, `interrupted` | an attempt would start while a key of the run is closed (by anyone, invariant 18), the daemon is stopping, or the run's agent has as many other started runs as its `max_runs` (a step with no agent, drain or exec, has no such limit) | `parked` (`then`: `new_session` from `preparing`, the decided kind from `interrupted`) | — |
| any non-terminal | operator cancel, or the cancel of the parent (6.1, reason `parent_cancelled`) | `cancelled` | stop the session, conditional release of the bead |

Every terminal transition frees the run's concurrency slot and releases its ownership of its bead. A bead the run
claimed is closed (`passed`) or released conditionally (`failed`, `cancelled`) by an effect on the queue
`tracker:<project>`, followed by a new read of the ready set; a refusal (`OwnerMismatch`, `NotFound`) is
`resync.diverged{subject: "bead"}`, an unavailable tracker `tracker.sync_failed` and a retry, and the outcome of the
run never changes. The failure counter of the bead: a failed run that held the claim adds one, `passed`
resets it, `cancelled` and a refused claim leave it; at `bead_failures` (orchestrator config, default 3) the core
closes the gate `bead:<project>/<id>`, and the operator opening that gate resets the counter. Operator holds (gates
closed by the operator, 7, and cancel) are read by every automatic decision: a decision that would start an attempt
of a run whose gate the operator closed parks it instead (invariant 18).

### 6.3 Finisher

- Every attempt gets a token: 32 random bytes as 64 hex digits. The state keeps its SHA-256 with `(run, attempt,
  generation)`; the token itself reaches the adapter through `SessionPlan` and lives only in the attempt's launch
  effect until it runs. The token is valid while its attempt is the run's current one and the run is not
  terminal.
- The session environment carries `AAE_RUN`, `AAE_ATTEMPT_TOKEN`, `AAE_SOCKET` and, when the run has a bead,
  `AAE_BEAD`, as an exec step's does (see 4.6 for how each harness gets it).
- CLI: `aae finish pass|fail --summary <text> [--data <json>|@<file>]` → `POST` of the outcome to the API with the
  token. Answers: accepted; `stale_attempt` (an older or ended attempt — nothing changes); `run_terminal`; `invalid`
  (unknown token or run, `summary` over 4 KiB, `data` over 64 KiB, a call while the run is `finishing`: the declared
  outcome is under check, or a call while the run waits for the decision on `run.turn_ended_without_outcome`: the turn
  is over, and an outcome now would decide nothing). With the daemon unreachable the command retries with backoff for 60
  s (`AAE_FINISH_RETRY` seconds overrides it), then exits with code 75; every try carries the same idempotency key, a
  hash of the request, so a retry after a lost reply gets the first answer.
- The finisher is the agent's word on the outcome: `pass` or `fail`, a short summary, optional data. A failure of
  the infrastructure is not an outcome: it raises `run.attempt_failed`. The outcome is kept in the run's result
  and reaches triggers as `outcome` of `run.passed` and `run.failed` (`outcome.data.<key>` in `where` and `inputs`).

How the finisher behaves, on the transitions of 6.2:

| Situation | What happens |
|---|---|
| finisher called while the turn still runs | the declared outcome is stored in the state; the run stays `running` |
| finisher called again in the same attempt | the last call wins |
| turn ended, no background work, outcome declared | `finishing`: `run.finishing` is raised, check addons may veto |
| turn ended, outcome declared, commands left running | the daemon has the adapter stop them (`stop_background`, 4.1); the session comes to idle without background, then `finishing`; the session stays for a veto remark |
| turn ended, background the daemon does not stop (a subagent), or no outcome | the run stays `running` until idle without background; the wait has a deadline (background silence budget, 6.1) |
| finisher called after the turn ended, while background runs | the outcome is stored and the commands left are stopped, as above |
| turn ended, no background work, no outcome | `run.turn_ended_without_outcome`; with no addon decision `failed{hollow_done}` |
| a `run.finishing` hook returned a remark | the remark goes to the agent as the next prompt in the same session and attempt; the declared outcome is cleared; the token stays valid |
| a `run.finishing` hook vetoed | `failed{check}` with the hook's reason |
| finisher with the token of a stale attempt | refused with an explicit error (`stale_attempt`); the state does not change |
| daemon unreachable at the call | the command retries for a bounded time and fails (exit 75); after the daemon restarts, the run goes the `interrupted` way |

The token belongs to the attempt, not to one call. Returning a remark is the check loop; the number of returns is
bounded by the core's ceiling (6.1, invariant 16).

The outcome of an `await` step comes from outside, not from a session: `POST /v1/runs/{id}/resolve` (`aae run
resolve`) with `{step, outcome}` and no token. It is taken while the run waits on that step (`running`, the step
named), with the finisher's limits (`summary` up to 4 KiB, `data` up to 64 KiB), and goes on as a declared outcome
(`finishing`, the `run.finishing` hooks); otherwise it is `rejected` and the state does not change. A repeat is
answered by its `Idempotency-Key`.

Out of the box `pass` is the agent's word. Checking that the work is really done (a commit exists, tests are green)
is the job of an outer loop of the user and is not bundled: the veto mechanism is there (`run.finishing`), an addon
for it is not.

### 6.4 Prompt delivery and recovery

- Every prompt has a delivery record `intent → sent → acked → settled` and a `PromptKey`. `intent` is written
  with the decision to send; `sent` after `send_prompt` returned; `acked` on `prompt.acked`; `settled` when the turn
  ended. No marker in the prompt text: both harnesses keep the key in the transcript natively
  ([experiments](experiments.md)), so the transcript check finds it.
- Daemon start ([invariants](invariants.md#recovery)): open the state and drain the outbox; for every recorded instance check pid + start time, kill its
  process group and sweep every process carrying its marker `AAE_INSTANCE=<id>` in the environment, wait for the end
  (invariants 6, 7); every hook still queued is first killed by its `AAE_INSTANCE` marker, with what it started, so a
  hook the dead daemon left running does not outlive it. A hook process is killed with SIGKILL without waiting for its
  death, so the hook that runs again may start before the old process is gone: hooks must be idempotent (3.1).
  Runs with a live attempt — `starting`, `running` in a turn — → `interrupted` with `daemon_crash` (not a run waiting
  in an `await` step: its attempt and deadline stay, and a deadline passed while the daemon was down fires at start) (a `drain` attempt
  too: its next attempt gives no bead its entry of the step gave out), the launches, prompts and deciding hooks they
  still had queued dropped, and the command of an exec step killed by its `AAE_INSTANCE` marker (random per attempt)
  before any later attempt runs it. A run that waits for a decision on a deciding event keeps its
  state and waits for the same decision (3.4): `preparing`, `interrupted` waiting for the policy, `finishing`, and
  `running` after a turn that ended without an outcome; `finishing` with a step check script running is one too, but
  its script is killed by its marker and starts again, and the execution counts only once the script's word is
  recorded. Its hook effect is still queued and its hooks run again; the
  declared outcome and the result of an exec command stand, the turn and the command do not run again. A run goes on
  from the step it is at: the move to the next step is in the transaction that accepted the outcome (6.1), so a finished
  step never runs again, and an exec command killed by the restart runs again in a new attempt of the same step.
  Recovery lets go of the handle of such an attempt, whose session died with its instance, and drops what it still had
  queued for the session. A daemon that dies while what a session left in the background is being stopped (outcome
  declared, turn ended, idle without background not yet seen) leaves a `running` run whose only wait was that stop:
  the kill of its instance is the stop, so the run settles its turn as idle without background would — `finishing`
  with the declared outcome — and the stop still queued is dropped. The same holds for a background subagent of
  claude, killed with its instance. The deadlines that wait for an effect's answer count again from the start:
  `decide` (3.3) and the claim of a bead in `preparing` (`start_timeout`); those of real time — gate `until`,
  schedules (a missed one fires at most once), the `after` of a retry, `queue_timeout`, `park_timeout` — stay. Then
  resync. Deadlines are wall-clock times: the writer reads the wall clock at least every 30 s, so a deadline passed
  while the machine slept fires late by at most that.
- Daemon stop (a signal, the end of a drain, `restart`): the core stops first, then the daemon, a child subreaper,
  kills every descendant it still has — sessions, hooks, exec commands — before it exits.
- A resumed attempt after `daemon_crash` checks the transcript for the last unsettled key: not delivered → send the
  same prompt with the same key; delivered → send the reconcile prompt (a constant of the core:
  "Your previous turn was interrupted. Check the current state of the work and continue toward the goal.") with a
  new key. A `new_session` attempt after `daemon_crash` sends the step's prompt; a remark or a `continue` left
  unsettled follows it, after a blank line, in the same first prompt.

## 7. Issue gates

Issue gates are the core's only "do not start yet" mechanism: a record "key closed until time T, or until removed
by hand". A run with a closed key waits in `queued` (or `parked`, once started) with the gate as the visible reason.
Three parties close gates: the policy in answer to a failure (an account limit, an agent quarantined after a crash
series), the operator (a pause) and the core itself (a bead after a series of failed runs, see 6.2). Gates are rows
in the state: they survive a restart, are listed by the API (`/v1/gates`, `aae gate`), and every closure and opening
is an event. The account traffic light and the operator hold are built from this one mechanism, with no subsystem
of their own. The adapter reports the account id (4.5, `Driver::account`).

- Keys: `orchestrator`, `project:<project>`, `rig:<project>/<rig>`, `agent:<agent>` (the qualified agent name of
  `config.md` "Names", or the name of a variant), `bead:<project>/<bead id>`, `account:<harness>/<account id>`,
  `model:<harness>/<model>`. The keys of a run are all of these that it has; a run is admitted only if none is
  closed. A run has exactly one model key, its effective config's (config.md "Harness and model": the harness and
  the model name the adapter expanded), taken from the run's snapshot, or before the snapshot as computed when the
  run was created; it is known before the first attempt. A model a failure names (`provider_model`, 4.4) stays data
  of the `run.attempt_failed` event and adds no key.
- A gate is a set of closures `{key, until, reason, closed_by, closed_at}`, at most one per `(key, closed_by)`;
  the key is closed while any closure is active. Automation (`policy`, `core`) removes only its own closures; an
  operator closure is removed only by the operator, so automation never reopens an operator pause (criterion 19).
- A closure with `until` ends by the managed clock (`gate.opened{by: "timer"}`); without `until` only by hand
  (API, CLI).
- The account key comes from `Driver::account` on the process part of the config, read before every attempt
  launches (or from `account` of a failure); a closed account gate parks the run with no launch, so a second run
  of the same account does not start (criterion 10). A `park` on a key the run does not have closes the gate and
  starts the attempt at once.
- `until` is UTC in the form events carry: `YYYY-MM-DDTHH:MM:SS[.fff]Z`; another form is an invalid decision. A
  closure whose `until` has passed counts as open even before its timer fires; one that has passed already when
  asked for closes nothing (the operator's is rejected, a decision's is ignored).
- The operator closes with `{key, until, reason}` (a closure of `(key, operator)` is replaced) and opens a key:
  that removes every closure of it, the policy's and the core's too. `gate.closed` and `gate.opened` carry no run.
- No half-open probe in the core: when a gate opens, the waiting runs are admitted in creation order within the
  concurrency limits. A failure of an attempt that started before the gate closed on its key is visible to the
  policy (`gates` in `run.attempt_failed`), which parks the run without escalating again — the "absorb" of the
  traffic light.

## 8. Instance and service registry

- Fingerprint = SHA-256 of the harness name and the canonical JSON (sorted keys) of the `ProcessPart`; two agents
  with equal process parts share an instance (criterion 13). The adapter decides what is the process part (4.6):
  claude puts everything in it, so every session has its own process (no shared server, no memory saved); codex
  shares one app-server per fingerprint. So an addon whose agent side changes the process part gives another
  fingerprint and another instance by itself, and an addon that works only on the orchestrator side adds no instance.
- An instance that holds many sessions has a session cap (`sessions_per_instance`, 4.5): its crash takes all its
  sessions down at once. Past the cap a second instance with the same fingerprint starts (acquire, below).
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
- Confirmed death (invariant 6): a launch first waits until nothing is left of the instances that served its run (or
  the run it continues) and are `stopping` or no longer recorded: the old process gets to end by itself (pid + start
  time, `/proc/<pid>/stat`), then every process still carrying its marker is killed until none is found and each one
  killed is a zombie or gone (a dying process loses its environment, so its marker, first), up to 30 s in all; past
  that the attempt fails with `unknown`. A live shared instance is never waited for.
- The process environment of every instance and service holds `AAE_INSTANCE=<id>`: once the instance is stopped
  or has exited, every process still carrying it is killed (the sweep of invariant 7). The daemon is a child
  subreaper ([experiments](experiments.md#descendants-after-kill--9-invariant-7)): every 5 s it reaps the adopted
  orphans that exited, never a child it spawned itself (those are reaped by their own `wait`). The daemon's crates
  spawn every child through `aae_contract::orphans`, which registers it; clippy denies a spawn past it
  (`clippy.toml`).
- Health: every 30 s the daemon asks the driver for the health of each instance it started; `Dead` counts as an
  `exited` event of the instance (above). A `ready` instance that turns `Degraded` gets `instance.degraded`
  `{instance, reason: text}` and `degraded: <text>` in the instance view (`GET /v1/instances`, `aae instance list`;
  null otherwise); it keeps the state `ready` and its sessions. When it is well, `instance.ready` with `reason: null`
  is emitted again.
- Resync ([event model](event-model.md#resync)): every 5 minutes, with the trackers, the driver is asked for the health of every instance not `stopping`
  that has a process (pid + start time for claude and codex). One found `Dead` whose record still holds that process and
  is not `stopping` counts as its `exited` event, as above: a gone process is not ambiguous, so no `resync.diverged`. A
  record that moved on since the check is left alone. Every process still carrying the marker of an instance an attempt
  used (live or ended within the hour) and no record holds is swept again; at a start too ([event
  model](event-model.md)).
- Services ([config](config.md#mcp-servers-and-services)) use the same record with kind `service`: the service
  name, the key `<scope>#<sha256 of the definition>` (the definition is `command`, `env`, `health`; scope `orchestrator`
  or `project:<p>`), the definition, the port and the token in the process column, the consumer count in `sessions` (an
  attempt's services are in `attempts.services`). The launch transaction acquires them (a new record emits
  `service.starting` and the `start_service` effect) and rewrites the session's config before the split (the services
  of the session's subagent targets are bound with it, those with an MCP server only), with
  `${AAE_SERVICE_PORT:<id>}` for the port: the launch waits for the service's own health check and puts in the port it
  passed on. A start finds its port free or fails without a process. Its health check (the port accepting connections,
  an HTTP `GET` or a command) passing gives `service.ready` and `ready: true` in the process column; a process that ends
  or is not up within 30 s gives `service.failed`, then a restart after the effect backoff of its restarts while it has
  consumers (on a new port until `ready`, then on the same one), else the record goes. Idle, operator and recovery stops
  go through `stop_service`, which ends the process group and sweeps the marker; a reload leaves services alone. pid and
  pgid of the view are null: the process is tracked by its task and marker, not by pid.

## 9. Fakes in the real daemon

The daemon has no test mode. Harness binaries are resolved through mise or taken from an explicit path in config:
`[harness.claude] binary = "<path>"`. Contract tests and model-free acceptance scenarios use exactly this:

- the real `claude`/`codex` against a fake provider, configured by ordinary settings (claude: `ANTHROPIC_BASE_URL`
  in the agent env; codex: a `model_providers` entry in the native block);
- or, below the adapters, the in-process scripted driver of `crates/aae-testkit`, which the core's tests wire in
  instead of an adapter.

The adapters cannot tell the fake provider from the real thing; nothing in the product exists only for tests.

## 10. `default-policy` as decisions

The built-in policy expressed in the decisions of 3.2, so that the schema provably carries every reaction of the bundled `default-policy`.
Thresholds are its config parameters (docs/design/config.md); "consecutive" counts are computed from `attempts` of the
event, times from its `ts`, and the gate keys come from its `keys`. A `park` on a key the run does not have (an exec run
has no agent key, a run whose harness reported no account has no account key) becomes a `resume` after the same wait,
except a `park` with no `until` (a lasting `auth_required`, `billing`), which becomes `fail`; a `fail` names no gate
then. Implemented in `crates/aae-addons`.

The thresholds below are uncalibrated defaults, not measurements: the 30 s rapid crash, five in a row before a 5 min
pause and three failed resumes before a new session come from the code of an earlier orchestrator, the
`min(10 s · 2^(n−1), 5 min)` backoff from the Symphony specification. Each may be changed on evidence.

| Event / kind | Decision |
|---|---|
| `rate_limited` while the run's account is used up (`rate_limit.utilization >= 1.0`, `resets_at` in the future; the run has an `account:` key) | `park{gate: account, until: resets_at + quota_margin_s, then: resume}`; takes precedence over the other `rate_limited` rows |
| `quota_exhausted` | `park{gate: account, until: T + quota_margin_s (60 s), then: resume}`, `T` = `retry_at` of the failure, else `resets_at` of the used-up account; neither → `until` now + `quota_wait_s` (30 min) |
| `rate_limited`, `overloaded` with `retried_by_harness` | `park{gate: model, until: now + 10 min, then: resume}` |
| `rate_limited`, `overloaded` without | `resume{after: min(10 s · 2^(n−1), 5 min)}` over `n` consecutive such failures; from the 3rd — the `park` above |
| `auth_required`, `billing` | `park{gate: account, until: now + min(30 s · 2^(n−1), 1 h), then: resume}` over `n` consecutive failures of that kind (a proxy or WAF error often passes by itself; the attempt after the wait is the probe); never escalated to an operator, and these attempts do not count against the core's ceiling (6.1); no account key: `fail` |
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
| `run.finishing`, `run.preparing` | no decision |
