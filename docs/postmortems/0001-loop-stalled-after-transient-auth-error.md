# Postmortem 0001: development loop stalled after a transient auth error

Date: 2026-11-13. Status: open — describes the problem and the requirement; nothing is implemented yet.

This is the first incident observed while building `aae` itself. The development loop that writes `aae`
(an overseer session driving `coder` → `reviewer` subagents) is exactly the kind of unattended agent run `aae`
is meant to supervise. It stopped making progress for hours, and nothing in the loop could notice or recover.
`aae` must handle this class of failure on its own.

## Timeline (local time, UTC+7)

- ~16:35 — cycle 32: the coder finished; the overseer started the reviewer as a background subagent.
- shortly after — the reviewer died on its first API calls:
  `API Error: 403 Attention Required! | Cloudflare (error type authentication_failed)`, with the advice
  "Please run /login". The harness sent exactly one notification: task `failed`.
- The overseer followed its retry rule for API errors: paused 590 s, then asked the loop controller for the next
  step. The controller answered "WAIT: reviewer is still running".
- The overseer ended its turn to wait for a completion notification. That notification had already been delivered
  (the `failed` one), so no other notification was coming. From then on nothing was running: no agents, no processes,
  no timers.
- ~19:15 — the owner looked in, about 2 h 40 min later. The API was working again: the 403 had cleared without anyone
  acting (network or edge/WAF glitch; no re-login was needed). The loop resumed only because a human prompted it.

## Why it stalled

1. **Push-only waiting.** The waiting party slept until a push event arrived, and that event had already fired.
   Nothing re-checked on a timer. A waiter that depends on a single notification hangs for good once the notification
   is lost or already consumed.
2. **Two sources of truth about liveness.** The harness knew the agent had failed. The loop controller decided
   liveness from a heuristic instead: a marker file plus "transcript written within the last 20 minutes". A process
   killed by an API error does not remove its marker (no stop hook fires), and the retry pause (590 s) was shorter than
   the freshness window (1200 s). The controller therefore reported a dead agent as alive.
3. **The retry rule had no owner after the pause.** "Wait, then ask again" assumes someone wakes up to ask. After
   one "wait" answer the overseer had no scheduled next step, so the retry ladder ended after its first rung.
4. **Misleading failure class.** The error looked like `auth_required` ("run /login"), which suggests that a human
   must act. In fact it was transient and went away by itself. Treating it as terminal or human-only would also have
   stalled the loop. Treating it as "retry blindly forever" would have hidden a real logout.
5. **Nobody checked whether the cause had gone away.** Nothing probed whether the external condition (auth,
   network, provider) had recovered. Recovery "from outside" or "by itself" was invisible to the loop.

## What aae needs

These items refine existing parts of the spec (`docs/init/AAE-SPEC.md` §5 invariant 15, §7 failure kinds,
§11.5 `default-policy`). They do not add new concepts.

- **Liveness by observation, not by notification.** The core's watchdog must decide "alive / dead / stalled" from
  the process and the session (pid/exit status, protocol events, heartbeats), with one authoritative record. A
  missed or already-consumed event must not leave a run in "running" forever. Stale markers must not count as life.
- **Every wait is bounded and re-armed.** Any state that waits on something outside `aae` (a subagent, a harness
  retry, a quota reset, an auth problem) has a deadline. When the deadline passes, the waiter re-probes and moves on
  by itself, without a new external event.
- **Probe whether the cause cleared.** For failures caused by the environment (`auth_required`, `overloaded`,
  `rate_limited`, network, `unknown`), the policy runs a cheap probe on a backoff schedule. Examples of probes: the
  harness's own auth/status check, or one minimal request on the cheapest model. When a probe succeeds, the account
  or agent gate reopens and the run is resumed or restarted. The journal records that recovery happened without
  operator action.
- **Classify by evidence, escalate late.** `auth_required` from a proxy/WAF page (HTTP 403 with an HTML body,
  Cloudflare marker) is first treated as transient and retried with probes. It becomes "needs a human" only after
  probes keep failing for a configured time. Only then does the escalation event to the operator fire. The raw
  error is kept (`failure.raw`).
- **Self-check of the supervisor.** The supervising loop needs a dead-man's switch: if no run has made progress and
  no wait is armed for longer than a threshold, that is an incident in its own right (`stall` of the orchestrator)
  and must be raised, not silently tolerated.
- **Acceptance.** A scripted-driver scenario: the agent dies with a transient 403 and the completion event is
  dropped or duplicated. The run must not hang. It recovers on its own after the fake provider starts answering
  again, and the journal shows failure → probes → recovery. A second scenario: the auth stays broken, and the run
  escalates after the configured time instead of retrying forever.

Owner's note: this is one of the reasons `aae` exists. Glitches like this one must not need a human to unstick the
work.
