# Harness adapters

A harness is an agent CLI aae drives (claude, codex). The core treats every harness as the `Driver` trait
([contracts 4](contracts.md#4-driver-interface)); each harness gets an adapter crate, `crates/aae-<name>`, that
translates its protocol into the trait. Adapters are modules of the one `aae` binary, not processes of their own:
there is no adapter protocol between processes (spec §4).

## Adding a harness

1. A crate `crates/aae-<name>` that depends on `aae-core` and implements `Driver`.
2. In it, one exported description of itself:

   ```rust
   pub const ADAPTER: aae_core::driver::Adapter = Adapter {
       name: "<name>",                   // [harness.<name>] and the binary mise resolves
       tested: [(1, 2, 0), (1, 4, 3)],   // tested binary versions, inclusive; also Capabilities.versions
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
   MCP servers), a sandbox. `login_check` and `host_mcp_check` in `aae_core::driver` give the common wording.
3. One row in `ADAPTERS` (`crates/aae/src/harness.rs`) and the crate in `crates/aae/Cargo.toml`. Nothing else in
   the root names a harness; a `[harness.<name>]` with no row is a config error that lists the known ones.
4. The binary pinned in `mise.toml` (spec §13), a line budget in `scripts/budget.sh`, and the contract suite below.

## What the core expects of a driver

The trait and its types are in [contracts 4](contracts.md#4-driver-interface). Beyond the signatures the core
relies on these, and a driver that misses one fails runs by the watchdog, not by an error:

- **`PromptAcked{key}`** for every prompt, with the prompt's own key. It is the only way an attempt leaves
  `starting`. A harness with no acknowledgement of its own needs one made from the first sign it took the prompt.
- **A turn ends with two events**: `TurnEnded{stop_reason}`, then `State{Idle, background}`. The attempt settles
  only on `State{Idle, background: false}`; with `background: true` (work the harness goes on with after the turn)
  the core waits for a later `State{Idle, background: false}`. Only the harness's authoritative turn-end signal
  ends a turn, never the answer to a request.
- **A failure** is `Failure{kind, scope, retry_at_ms, retried_by_harness, raw, account, provider_model}` of the
  kinds in contracts 4.4. `retried_by_harness` is true when the harness already retried a 429 or a 5xx itself:
  the policy never retries on top of it. Wording → kind tables belong to the adapter. A success that carries an
  error is a failure; an exit in an open turn is `harness_crash` whatever the exit code.
- **`Exited{code, signal}`**, instance-wide, once the process is gone, after the failure of a turn it left open.
- **`Activity`** at most once a second per session; any event moves the silence deadline.
- **Session events carry the `SessionRef` the adapter returned**, byte for byte: the core finds the attempt by
  the JSON text. Changing the shape of a stored `SessionRef` breaks the attempts in flight across an upgrade.
- **The attempt environment** — `AAE_RUN`, `AAE_ATTEMPT_TOKEN`, the tracker actor of `SessionPlan`, `AAE_SOCKET`
  — reaches the session's tools and its harness hooks (contracts 4.6 says how each harness delivers it).
- **Service URLs and headers stay verbatim in the parts**: until a service is up its port and token are the
  placeholders `${AAE_SERVICE_PORT:<id>}` and `${AAE_SERVICE_TOKEN:<id>}`, which the core replaces as text in the
  JSON the adapter made of them, so an adapter must not encode or reshape a value it got.
- **The process** runs in a process group of its own and carries `AAE_INSTANCE` (the orphan sweep finds it by
  that); nothing of the daemon's environment is inherited unless the adapter passes it.
- The adapter never retries a turn, parks or decides an outcome (spec §4).

`aae_core::driver::child` holds what every process-based adapter does the same way: `spawn` (piped stdio, own
group, pid and start time), `base_env`, the `Events` sender, `health`, the stop ladder `stop`, `watch` (a panic of
the reader task becomes `adapter_crash`) and `config_error`; `stderr_tail` and `HOST_NETWORK_ENV` sit next to it.

## Contract suite

Every adapter has `crates/aae-<name>/tests/contract.rs`: the fifteen scenarios of
[harness updates](../harness-updates.md) on the real binary against a fake provider of `crates/aae-testkit`
(`anthropic` or `openai`; a harness on either API needs no new fake). They check the events the adapter sends, not
the fake. A harness update is accepted when the suite passes on it (harness updates, steps).

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
- Hooks of `EffectiveConfig` are in the Claude dialect (`event`, `matcher`, `command`, `timeout`). codex takes
  the same form; a harness without command hooks can only refuse an agent that has any, as a `config_error` at
  the start of a run.
- `capabilities(&ProcessPart)` takes a process part although the tested versions do not depend on it; the root
  reads them from `Adapter.tested` instead.
- `InstanceHandle{pid, start_time, pgid}`: an instance is a local child process of the daemon.
