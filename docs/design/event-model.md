# Event model of the core

How work enters the core, how it is committed and how its consequences are carried out. The payloads of events,
the outbox lifetime and the per-subject contracts are in [contracts.md](contracts.md); this page is the shape that
all of them share.

## Messages, one transaction, effects after commit

- Everything that happens enters the core as a message to the single writer (`crates/aae-core/src/writer.rs`): a
  frontend command, an event of a harness adapter, a tracker event, a deadline firing, the answer of an exec hook.
  The run messages are `aae_core::run::Msg`.
- The writer handles one message in one SQLite transaction. That transaction changes the state and records
  everything the message caused: the events it emits (the outbox, each with its `seq`), the effects to run and the
  deadlines to set or clear (`Tx::emit`, `Tx::effect`, `Tx::set_deadline`, `Tx::clear_deadline`). Nothing outside
  the state happens before commit (invariant 9: intent before effect).
- Consumers of committed events: the JSONL journal writer (the one durable outbox consumer), trigger matching and
  plugin exec hooks (both tied to the transaction that commits the event), and API event streams (which read the
  journal from a `seq`). See [contracts.md §3.5](contracts.md#35-outbox-lifetime).
- The journal writer appends each drain as one batch with one `sync_data` at its end (and one before a file is
  rotated away), and acknowledges the batch in the outbox only after that sync: an unsynced line is never acked, so
  a power loss can cost journal lines but the next drain writes them again from the outbox.

## State

- One SQLite database, `$XDG_STATE_HOME/aae/state.db`, in WAL mode; SQLite is linked statically (rusqlite
  `bundled`). The daemon is its only writer (`crates/aae-core/src/store.rs` opens it, the writer changes it). The CLI,
  plugins and agents change the state only through the API.
- The state holds runtime only: runs, attempts, session references, harness instances (the process part and the
  handle: pid, start time, pgid), bead ownership with its generation and failure count, ready sets and tracker cursors, deadlines (timers
  and schedules), gates (operator holds among them), idempotency keys, the outbox and the effects queue.
- Definitions (projects, rigs, agents, formulas, triggers, plugins) live in config files. A run keeps only the
  snapshot of its effective config (`runs.config`, the serialized `EffectiveConfig`), so it answers "what did this
  run with" and is reproducible.
- No status files, PID files or lock files. One daemon per state directory: the daemon holds an exclusive `flock`
  on the state directory itself (`crates/aae/src/daemon.rs`, `lock`); the OS drops it with the process, so a crash
  leaves nothing stale. Whether a process is alive is read from the process table: pid plus its kernel start time.
- The state directory and `journal/` are 0700, the database and journal files 0600, whatever the umask.

## Effects

- Effects run after commit through a queue keyed by subject (a run, a harness instance, `daemon`; see
  `crates/aae-core/src/effects.rs`): one effect at a time per key, in order.
- A pending effect with the same `(key, dedupe)` absorbs a new one, so repeating a message does not repeat its
  effect.
- A failed effect is retried with backoff 500 ms, 1 s, 2 s, … capped at 5 minutes; a panicking effect is logged and
  retried the same way. Effects live in the state, so a daemon restart picks them up.
- An effect reports back by sending a new message to the writer; it never writes the state itself.

## Deadlines, not polling

- Timers are deadlines with an exact wake-up time stored in the state; the writer sleeps until the earliest one.
  Deadlines are wall-clock times: the writer reads the wall clock at least every 30 s so a clock jump is noticed.
  A deadline whose handler fails fires again a minute later.
- The latency from an event to the core's reaction contains no polling interval. Acceptance checks it as the hop
  (criterion 6, `crates/aae-acceptance` scenario `c06_hop.rs`: 30 hops, p50 about 35 ms to the harness launch).
- Tests run the same writer on a managed clock (`aae_core::clock`), which makes every deadline deterministic.

## Resync

Resync is not a tick: it advances nothing in normal operation and only catches lost signals.

- At daemon start, after recovery (`run/recover.rs`): every recorded instance is checked by pid + start time
  and stopped, the trackers are read again, and the process table is checked for markers (below).
- Every 5 minutes (deadline `daemon:resync`, a constant): the trackers are read again whatever their watchers say
  (`sync_trackers`), and the driver is asked for the health of every instance record not `stopping` that has a
  process (`check_instances`; pid + start time for claude and codex). The check is one effect per daemon: while
  one still waits in the queue (backoff, a slow health call), a newer one is dropped with its newer list of
  records; the next resync catches what that list would have, and a stale list is harmless (see below).
- A process found dead whose record still holds that process and is not `stopping` is taken as its lost exit: the
  instance fails as `harness_crash` and so do the attempts on it. A gone process is not ambiguous, so this is a
  normal transition, not `resync.diverged`. A record that changed since the check is left alone.
- With the health check, every process still carrying the marker (`AAE_INSTANCE`) of an instance this state ran
  (an attempt names it, live or ended within the hour) and no longer records is a sweep a descendant outran: it is
  swept again (invariants 7, 20), a normal transition with no event. A recorded instance and a marker this state
  never gave (another daemon's) are left alone. `/proc` is walked only when there is such an instance. Markers of
  services and exec steps are not checked: the state keeps no history of their ids.
- An ambiguous difference (for example a bead whose owner changed outside the daemon) is reported as
  `resync.diverged` and no automatic effect follows (invariant 20).
- Services (`kind = 'service'`) are not checked on resync: their process is supervised by its own task, which sees
  its exit directly.
