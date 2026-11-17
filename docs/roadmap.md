# Roadmap

What lies ahead of aae, so that a developer or an agent working on the repository knows where the product goes.
This is a hint, not a work list: nothing here is taken into work from this page, and nothing here needs code,
stubs or "room for later" in the core now. The core stays minimal; everything below is meant as plugins on top of
hooks and events.

| Item | What it is | Depends on | Priority |
|---|---|---|---|
| Metrics plugin | Collects what the runs spend: tokens, cost, turns, session length, per run, formula and agent | `usage` events and hooks | Before budgets |
| Budget plugin | Limits a run, a formula or a correlation by tokens or cost, from the metrics; decides what to do on overrun | The metrics plugin | After metrics |
| Compaction plugin | Decides what to do when a harness compacts the context: a handoff, a new session, nothing | Compaction reaching plugins as an event; the adapters do not report it today, only the harness hooks of a plugin see it (`context-budget` counts it so) | Later |
| Harness availability notice | Tells a human outside aae that a harness is unusable (`auth_required`, `billing`): the core only waits with a backoff up to once an hour, forever | Hooks on `run.attempt_failed` | Low |
| Recovery formulas | Formulas that recover a failed run by themselves along a set workflow: a postmortem by another agent, an archive of the session, a choice of how to go on | The recovery tools of the core and the recovery skill, used by hand first | Low |
| Restrictive permission presets | Ready native permission rules for claude and codex narrower than the permissive default | Native permission rules and harness hooks | Later |
