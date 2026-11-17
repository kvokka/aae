# Harness adapters

A harness is an agent CLI aae drives (claude, codex). The core treats every harness as the `Driver` trait
([contracts 4](contracts.md#4-driver-interface)); each harness gets an adapter crate, `crates/aae-<name>`, that
translates its protocol into the trait. Everything known of one harness — its protocol, the shape of its failures, its
options, its model short names, its `doctor` checks — lives in its adapter. Adapters are modules of the one `aae`
binary, not processes of their own: there is no adapter protocol between processes; the way to connect an outside
harness later is a generic ACP v2 driver.

Where adapters stand among the other parts, and the boundaries the build checks, is in the
[architecture](../architecture.md). The adapter never decides policy: retry, park and refuse live in plugins and
config ([config](config.md#where-the-bundled-plugins-end-and-the-core-begins)).

## Adding a harness

1. A crate `crates/aae-<name>` that depends on `aae-contract` and implements `Driver`.
2. In it, one exported description of itself:

   ```rust
   pub const ADAPTER: aae_contract::driver::Adapter = Adapter {
       name: "<name>",                   // [harness.<name>] and the binary mise resolves
       models: &[("<short>", "<full>")], // short model names it expands; any other name goes as it is
       tested: [(1, 2, 0), (1, 4, 3)],   // tested binary versions, inclusive; also Capabilities.versions
       agent_types: &["<type>"],         // agent types built into the harness: no subagent takes such a name
       new,                              // fn(&Setup) -> Result<Arc<dyn Driver>, String>
       doctor,                           // fn(DoctorAgents) -> Vec<Check>
   };
   ```

   `new` gets the resolved binary, the state directory, the API socket and the adapter's own keys of
   `[harness.<name>]` (everything but `binary` and `model`). It parses them itself and names the wrong key in its
   error, which becomes a config error of `aae config check`, `aae doctor` and daemon start. A harness that takes
   no settings refuses any key. `doctor` gets every agent on the harness, labelled and with its effective config,
   and answers what only the adapter knows: whether a session the daemon starts can log in (the daemon's own
   environment reaches no session, so a key there does not count), host settings every session inherits (stdio
   MCP servers), a sandbox. `login_check` and `host_mcp_check` in `aae_contract::driver` give the common wording.

   `models` is what the config knows of the harness (config.md "Harness and model"): a short model name becomes
   the full one when the config is assembled, so the gate key `model:<harness>/<model>` and `explain` have the name
   the harness gets. A harness whose CLI resolves its own short names (claude: `fable`, `opus`, `sonnet`, `haiku` to
   the latest of each) lists none. The core reads them from `Config::adapters`, which the root fills with its
   `ADAPTERS` when it loads the config (`config::load_for`); the core knows no harness by name. Which vendor a harness
   reaches its model through is the harness's own setting (host config or `native.<harness>`); the core neither
   reads nor names it.

   A test of the adapter checks the table against the harness's own model list, with no model call (codex:
   `model/list` of `codex app-server`, `crates/aae-codex/tests/harness/models.rs`). A target the harness does not
   list, or marks as replaced, fails it: that is a finding for the owner, who picks the new name; the table is never
   switched to another model silently.
3. One row in `ADAPTERS` (`crates/aae/src/harness.rs`) and the crate in `crates/aae/Cargo.toml`. Nothing else in
   the root names a harness; a `[harness.<name>]` with no row is a config error that lists the known ones.
4. The binary pinned in `mise.toml` by release line ([harness updates](../harness-updates.md)), a line budget in `scripts/budget.sh`, and the contract suite below.

## What the core expects of a driver

The trait and its types are in [contracts 4](contracts.md#4-driver-interface); what the core does with each event
and each `Health` value is the table of [contracts 4.3](contracts.md#43-normalized-events). Beyond the signatures
the core relies on these, and a driver that misses one fails runs by the watchdog, not by an error:

- **`PromptAcked{key}`** for every prompt, with the prompt's own key. It is the only way an attempt leaves
  `starting`. A harness with no acknowledgement of its own needs one made from the first sign it took the prompt.
- **A turn ends with two events**: `TurnEnded{stop_reason}`, then `State{Idle, background}`. The attempt settles
  only on `State{Idle, background: false}`; with `background: true` (work the harness goes on with after the turn)
  the core waits for a later `State{Idle, background: false}`. Only the harness's authoritative turn-end signal
  ends a turn, never the answer to a request.
- **`stop_background`** comes once the outcome is declared and the turn has ended with background (contracts 6.3). The
  adapter stops the commands the session left, through the harness so it accounts for them, and nothing the agent
  may still wait for (a background subagent). The session stays to take a next prompt (a veto remark), starts no
  turn of its own, and comes to `State{Idle, background: false}` by its usual rule once the commands are gone. It is
  best effort: an unknown instance or an error is no failure, and the run then waits for the background budget.
- **A failure** is `Failure{kind, scope, retry_at_ms, retried_by_harness, raw, account, provider_model}` of the
  kinds in contracts 4.4. `retried_by_harness` is true when the harness already retried a 429 or a 5xx itself:
  the policy never retries on top of it. The adapter classifies by the harness's structured fields (contracts
  4.6); text is a narrow fallback, and what no field tells apart is `unknown` with the harness words or the value
  in `text`. A success that carries an error is a failure; an exit in an open turn is `harness_crash` whatever
  the exit code.
- **`Usage`** carries increments, not running totals; `Retrying{delay}` is in real units (the core adds it to the
  silence limit); the events of one instance are sent in `seq` order by one sender at a time (a repeat is dropped, a
  gap is told as `resync.diverged`).
- **`Exited{code, signal}`**, instance-wide, once the process is gone, after the failure of a turn it left open.
- **`Activity`** at most once a second per session; any event moves the silence deadline.
- **Session events carry the `SessionRef` the adapter returned**, byte for byte: the core finds the attempt by
  the JSON text. Changing the shape of a stored `SessionRef` breaks the attempts in flight across an upgrade.
- **The attempt environment** — `AAE_RUN`, `AAE_ATTEMPT_TOKEN`, the tracker actor of `SessionPlan`, `AAE_BEAD` (when the
  run has a bead), `AAE_SOCKET` — reaches the session's tools and its harness hooks (contracts 4.6 says how each harness
  delivers it).
- **Service URLs and headers stay verbatim in the parts**: until a service is up its port and token are the
  placeholders `${AAE_SERVICE_PORT:<id>}` and `${AAE_SERVICE_TOKEN:<id>}`, which the core replaces as text in the
  JSON the adapter made of them, so an adapter must not encode or reshape a value it got.
- **The process** runs in a process group of its own and carries `AAE_INSTANCE` (the orphan sweep finds it by
  that); nothing of the daemon's environment is inherited unless the adapter passes it.
- **`env` references resolve only at the spawn**: the process part, the fingerprint and the account hash are made of
  the `${NAME}` form (config.md, "Environment values"), and `base_env` resolves it into the process environment, a
  variable the daemon does not have failing the start as `config_error` that names the key and the variable.
- The adapter never retries a turn, parks or decides an outcome: that is policy.

`aae_contract::driver::child` holds what every process-based adapter does the same way: `spawn` (piped stdio, own
group, pid and start time), `base_env` (the clean base and the agent's `env`, references resolved), the `Events`
sender, `health`, the stop ladder `stop`, `watch` (a panic of the reader task becomes `adapter_crash`) and
`config_error`; `stderr_tail` and `HOST_NETWORK_ENV` sit next to it.

## Contract suite

Every adapter has `crates/aae-<name>/tests/harness/contract.rs`, the module `contract` of its one integration test
binary: the fifteen scenarios of [harness updates](../harness-updates.md) on the real binary against a fake provider
of `crates/aae-testkit` (`anthropic` or `openai`; a harness on either API needs no new fake). They check the events
the adapter sends, not the fake. A harness update is accepted when the suite passes on it (harness updates, steps).

## Known limits of the driver interface

Made for the two harnesses there are; none is changed before a harness needs it.

- `harness()` returns a constant equal to the name in the config, and an adapter has one name: one adapter for
  several harnesses (a generic ACP adapter behind `[harness.gemini]` and `[harness.opencode]`) cannot be listed
  twice yet.
- `account(&ProcessPart)` assumes the process decides the account, true when the credential is a key in the
  environment or a login directory. For a harness that picks a provider per model it is the session's, not the
  process's. The gate key is `account:<harness>/<hash>`, so one provider key used by two harnesses is two gates.
- `PromptKey` is carried as the harness's own message id, and `transcript(key)` asks where that message stands.
  A harness that takes no client id or keeps no readable history can only answer whether the session exists, and
  the guard against a double delivery after the daemon died does not hold for it.
- Hooks of `EffectiveConfig` are in the Claude dialect (`event`, `matcher`, `command`, `timeout`). codex takes the same
  form, with the attempt environment exported in front of the command (a codex hook runs with the process environment,
  not the thread's); a harness without command hooks can only refuse an agent that has any, as a `config_error` at the
  start of a run.
- `EffectiveConfig.targets` (the subagents a session may call) is delivered as the harness's own subagent
  definitions: claude takes them by `--agents`; the codex adapter does not deliver them yet.
- `capabilities(&ProcessPart)` takes a process part although the tested versions do not depend on it; the root
  reads them from `Adapter.tested` instead.
- `InstanceHandle{pid, start_time, pgid}`: an instance is a local child process of the daemon.
