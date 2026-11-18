# Prompt-cache keepalive in Claude Code 2.1.327

Question: can Claude Code keep a prompt cache warm while one step (a long Bash / cargo build) runs longer than the
cache TTL, and how is that configured? Short answer: the mechanism exists, but it is a TUI-only, server-flagged
feature for the 1h subscriber cache of the main thread. It never runs in headless `-p`, never for subagents and never
through a proxy `ANTHROPIC_BASE_URL`. For loop-manager the lever is the TTL itself (`CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL=1h`).

Raw data (outside the repo): `research-raw/2026-11-19-cache-keepalive/` (proxy log, run transcripts, scripts).

## 1. Mechanism

Not a Messages request. It is a separate touch endpoint that names an earlier request by id; no prompt is resent.

```js
pnr="tengu_sequential_cloud", hnr="anthropic-cache-keepalive", ynr="/v1/messages/cache_touch",
Q8e=1e4 /*touch timeout*/, _nr=3, knr=3, Snr=3, Xse=rL /*300000*/, H9t=Xse-Q8e, wnr=3600000
vnr = { enabled: true, touchIntervalSeconds: 10..120 default 30, activityWindowMinutes: 1..1440 default 60,
        inFlightWindowMinutes?: 1..1440, resumeTouches: default false }      // schema of the server flag value
// the touch itself:
await client.post("/v1/messages/cache_touch", { body: { request_id }, headers: { "anthropic-beta":
  [BUe /*oauth beta*/, "cache-keepalive-2026-09-03"].join(",") }, maxRetries: 0 })
```

Lifecycle:

- A main-loop request is sent with beta `cache-keepalive-2026-09-03` (body unchanged, `cache_control` keeps its 1h ttl).
  If the response carries `anthropic-cache-keepalive: active`, the client arms a sender for that response's
  `request_id` (`setInterval(…, touchIntervalSeconds*1000)`, default 30 s). While the header is active the client
  counts the cache life as 5 min (`l7e(ke?Xse:OR)`), i.e. the server-side record lives 5 min and each touch renews it.
- A touch fires only if some activity happened within `activityWindowMinutes` (default 60): `main_request`,
  `user_input`, `work_start/end`, `awaiting_user_start/end`. While a turn or subagent is running (`turn_running`,
  `subagent_running`) the window is `max(activityWindowMinutes, inFlightWindowMinutes)`. So it does cover a long tool
  call or a long idle TUI wait, up to the window, then stops.
- Stops: next main request (re-armed on the new id), compaction or `/clear` (`chain_released`), model switch, account
  switch, flag off, gate off (`not_allowed`), record not renewed for 290 s (`entry_lapsed`), 3 × 404 not found,
  3 failed touches → off for 1 h, HTTP 400/403 → off for this login; 429/5xx back off (retry-after, else 1.5 × interval,
  ≤ 60 s). No count limit other than the activity window.
- `resumeTouches` (default false) would touch the last request of a resumed session.
- Cost: the touch carries only `request_id`; the client reads no `usage` from it, so its price is not observable from
  the client. Whether the server bills it as a cache read of the prefix is unknown.

Unrelated identifiers with "keepalive" in the name: `keepaliveIntervalMs` (bridge WebSocket heartbeat),
`idleKeepaliveS` (flag `tengu_slate_lantern_hush`, idle session timer), `keepaliveReasons` / `keepaliveGated` /
`hasAgentChildKeepalive` / `releaseBackgroundShellKeepalive` (task lifecycle: keep an agent task alive while its
children or background shells run), `throttleKeepalive` (gRPC). None of them sends API traffic.

## 2. Gates

```js
function Q9t(ttl, newChain){ return ttl?.ttl==="1h" && ttl.reason==="subscriber" && V9t()
  && !(newChain && (billingType==="usage_based" || isUsingOverage)) }
function V9t(){ return !refused && !_e() /*non-interactive*/ && !CCR_SESSION_PROFILE
  && !bt() /*CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC*/ && Ln() /*firstParty*/
  && nc() /*ANTHROPIC_BASE_URL unset or host api.anthropic.com*/ && !ANTHROPIC_UNIX_SOCKET
  && FS() /*!CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS, not HIPAA*/ && Pt() /*claude.ai OAuth*/
  && Tn("allow_cache_keepalive") /*org policy*/ }
function Qse(){ let e=S("tengu_sequential_cloud", false); /* must parse as the object above */ }
J9t(req, { isMainLoopRequest, continuesPrefix, cacheTtl })  // the sender is armed only for isMainLoopRequest
```

- Surface: interactive TUI only. `-p`, `--input-format stream-json` and the SDK are non-interactive, so they are out.
- Thread: main loop only. Subagents never get a sender; forks that continue the main prefix may carry the header.
- TTL: only a 1h cache with reason `subscriber` (the server-allowlist path). A 5m cache never gets keepalive, and
  neither does a 1h cache chosen by `CLAUDE_CODE_PROMPT_CACHE_TTL`, a setting, agent frontmatter or
  `ENABLE_PROMPT_CACHING_1H` (reasons `env` / `setting` / `agent_frontmatter` / `enable_1h_env`).
- Server flag: `tengu_sequential_cloud`, cached value for this account: `false`. So it is off even in the TUI here.
- Env / settings: no user knob turns it on or tunes it; interval and windows come only from the flag. Off switches:
  `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS`, any non-Anthropic
  `ANTHROPIC_BASE_URL` (`_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL` does not help, `nc()` checks only the host),
  `ANTHROPIC_UNIX_SOCKET`, API-key or cloud-provider auth, usage-based billing or overage. `DISABLE_TELEMETRY` and
  `DO_NOT_TRACK` do not affect it (`bt()` is true only for the nonessential-traffic switch).
- Entrypoint (`CLAUDE_CODE_ENTRYPOINT`) is not checked.

## 3. Measurement (headless `-p`, Haiku 5.5, logging proxy)

One `-p` turn, tools `Bash,Agent`, `BASH_DEFAULT_TIMEOUT_MS=BASH_MAX_TIMEOUT_MS=600000`: the main thread runs a
420 s wait in Bash, then a general-purpose subagent runs a 420 s wait in Bash. A bare `sleep 420` is refused by
Claude Code's own long-sleep guard (first run), so the wait was `python3 -c "import time; time.sleep(420)"`.

| t (s) | request | cache write | cache read |
|---|---|---|---|
| 0 | main #1 | 1554 (1h) | 6356 |
| 422 | main #2, after the 420 s tool call | 136 (1h) | 7910 |
| 424 | subagent #1 | 5084 (5m) | 0 |
| 845 | subagent #2, after its 420 s tool call | 5867 (5m) | 0 (full re-write) |
| 847 | main #3 | 409 (1h) | 8046 |

Zero `cache_touch` requests in 14 minutes, no `cache-keepalive` beta on any request, no `anthropic-cache-keepalive`
response header, no `[cache-keepalive]` line in the debug log. There is no "keepalive on" arm because no
configuration enables it in `-p`. Three gates block it here independently: non-interactive (`_e()`), proxy base URL
(`nc()`), flag `tengu_sequential_cloud=false`. The main thread survives the gap because its TTL is 1h; the subagent
loses its 5m cache. Spend: about $0.006 at list prices for both runs.

## 4. Cost model (Opus 5.5, 60k-token prefix)

- Re-write after expiry: 5m $0.30 (60k × $5/M), 1h $0.48 (× $8/M). Full-prefix read: $0.012 (× $0.20/M).
- If a touch were billed as a full-prefix read: break-even 25 touches against a 5m re-write (12.5 min at the default
  30 s interval) and 40 touches against a 1h re-write (20 min). The touch's real price is not visible to the client.
- 1h TTL for a subagent instead: +$3/M on written tokens, about +$0.18 for a subagent that grows to 60k. One step
  longer than 5 min costs $0.30 at 5m, so 1h pays off once ≥ 0.6 such steps happen per subagent.

## 5. Recommendation for loop-manager

Keepalive cannot be turned on for loop-manager: it is TUI-only, main-thread-only, needs the server flag, and is
switched off by a proxy base URL. The main `-p` thread already gets 1h (query source `sdk` is in
`tengu_prompt_cache_1h_config`). For subagents that run cargo builds, set
`CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL=1h` (or `subagentPromptCacheTtl` in settings). To target only the agents that
build, use `experimental: { cacheTtl: "1h" }` in their definition; the env var and the setting take precedence over
it, and on overage `1h` from the agent definition is ignored. Short helper agents with no long step are cheaper on 5m.
