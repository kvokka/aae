# What CLAUDE_CODE_DISABLE_TERMINAL_TITLE and CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC switch off (claude 2.1.327, static)

Method: `strings -n 8` over the Bun-compiled binary of claude 2.1.327 (build 2026-11-17), then a regex search of the
minified JS with context. No model calls. The docs check was a plain `curl` of `code.claude.com/docs/en/env-vars.md`.
Minified names (`bt`, `px`, `DZi`, ...) are local to this build. Some names are reused across chunks, so each site
below was checked in its own context.

## Core predicates

```js
function cNn(){if(process.env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC)return"essential-traffic";
  if(process.env.DISABLE_TELEMETRY)return"no-telemetry";if(Ae(process.env.DO_NOT_TRACK))return"no-telemetry";return"default"}
function bt(){return cNn()==="essential-traffic"}   // NONESSENTIAL only
function px(){return cNn()!=="default"}             // NONESSENTIAL or DISABLE_TELEMETRY or DO_NOT_TRACK
function Eb(){return px()||OM()||Bo()!==null||ny()} // "event logging off"
function X5(){return!Eb()}
function Yp(){return W8()===null&&(X5()||W9())}     // GrowthBook enabled (W8 = DISABLE_FEATURE_FLAGS/DISABLE_GROWTHBOOK)
```

NONESSENTIAL works on presence: any non-empty value, `0` included, turns it on. It does not set the other variables. It
puts the process in the strongest mode, so every `px()` gate (telemetry off) is also on. `bt()` has about 170 call
sites. The rest of the reads are messages, env forwarding lists and the remote-runner config.

## CLAUDE_CODE_DISABLE_TERMINAL_TITLE (read only as presence `s.CLAUDE_CODE_DISABLE_TERMINAL_TITLE`)

| What is cut | Where | Effect |
|---|---|---|
| Session-title request (Haiku, `querySource:"generate_session_title"`), in `-p` and the TUI | `oMs(){return bt()\|\|s.CLAUDE_CODE_DISABLE_TERMINAL_TITLE}`, used by `DZi()` (-p: `BHi(sd,fn\|\|DZi())` marks the title as attempted) and by the TUI query path `!xo&&!oMs()&&...&&!this._haikuTitleAttempted` | one fewer side request per session |
| OSC terminal-title writes | title source `n(){...return s.CLAUDE_CODE_DISABLE_TERMINAL_TITLE?{keep:"none"}:ENi()}`; session title controller `new p5({...,disabled:s.CLAUDE_CODE_DISABLE_TERMINAL_TITLE})`; agents view and `/resume` picker `RSt(...?null:"claude · resume")`; the title restore at exit `if(!s.CLAUDE_CODE_DISABLE_TERMINAL_TITLE)ae(1,NSa)` | TUI only |
| `process.title="claude"` | CLI preAction | the process name stays as it is |
| Terminal status reports (`programStatus` capability, tab writes) | `_e({tabWritesDisabled:s.CLAUDE_CODE_DISABLE_TERMINAL_TITLE})` | TUI only |
| Remote Control auto-title (`skipAutoTitle:iO()`) | `iO(){return(bt()\|\|TITLE)&&S("tengu_concurrent_moonbeam",!1)}` | bridge only; flag default false |

Nothing else. The variable does not touch telemetry, flags, updates or the stream.

## CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC

Messages API calls:

| What is cut | Where (snippet) |
|---|---|
| Session-title request | `oMs()` above |
| `max_tokens:1` quota probe (`source:"quota_check"`) | `probeQuotaStatus(){if(bt())return null;...if(_e()&&!u?.probeInNonInteractiveSession)return null` (off in `-p` anyway) |
| Usage-limit recheck probe while a request waits on a limit | `LYn`: `else if(bt())h="skipped_essential_traffic_only"` → "the request sleeps on" until reset |
| Prompt-cache keepalive (TUI) | `V9t(){...!_e()&&...!bt()&&...Tn("allow_cache_keepalive")}` |
| Model list / capabilities fetch (`models.list`) | `od(){if(!Eoo())return;if(bt())return;...}` |
| Model catalog | `Xcn(){...if(!o&&bt())return"essential_traffic"` |

Not gated by either variable: prompt suggestions, away summary, agent progress summaries, tool-use summaries,
memory extraction, auto-dream, compaction, the startup model check. They follow their own env vars and settings,
plus feature flags (see the flag-default risk below).

Other traffic:

| What is cut | Where |
|---|---|
| Feature flags (GrowthBook fetch) | `Yp()` via `Eb()`/`px()`; the getter returns `{value:n,source:"disabled"}` (the code default) |
| 1P analytics and Datadog event logging | `X5()=!Eb()` (`eventLoggingEnabled:X5()`) |
| Anthropic metrics exporter (team/enterprise) | `st(){if(px())return!1;...}` |
| Error reporting | `...\|\|process.env.DISABLE_ERROR_REPORTING\|\|bt())return` |
| Auto-update | `Db(){...let n=S6e();if(n)return{type:"env",envVar:n}`; GCS `latest` fetch `S0o(){if(bt())return null`; brew cask check; manifest `dt(){return Te()==="firstParty"&&!bt()}` |
| Release notes / changelog | `iAs(){if(_e())return;if(Y())return;if(bt())return;...}` |
| `/api/claude_cli/bootstrap` (client data, extra model options) | `"[Bootstrap] Skipped: Nonessential traffic disabled"` |
| Connectivity `/api/hello` | `eMs(){if(bt()\|\|px())return"skipped_privacy"` |
| API preconnect | `F(){...if(bt())return;...}` |
| First-party REST client (`api`/`claude-ai`/`mcp-proxy` hosts) | `if(!e.bypassEssentialTrafficOnly&&!o&&bt())return"essential-traffic-only"`; only `/api/claude_code/organizations/metrics_enabled` bypasses it |
| MCP registry, claude.ai connectors (cross-org override blocked), fast-mode org prefetch `CZe` | `JX(){if(bt())return;...}`, `CZe(){...if(bt())return;` |
| Plugins: command-source re-runs, refresh before install, claude.ai archive download, publishing | `Ykn(){if(bt())return!1`, `aln(){if(bt())return{outcome:"ineligible"}` |
| `/feedback`, `/bug`, surveys, SendFeedback | `if(bt())return\`${e} has been disabled via the CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC...` |
| gh/glab PR badge polls, trusted-device enrollment, org memory, cloud and remote sessions, Remote Control, design/projects, file uploads, promos, referral passes | many `if(bt())return` sites |

Docs (env-vars page): NONESSENTIAL turns off auto-updates, telemetry, error reporting, `/feedback`, release notes, PR
badges, availability checks and plugin command-source re-runs. It also turns off feature-flag fetching. It does not
cover the official marketplace auto-install (`CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL`). The code
matches. In effect it covers what `DISABLE_TELEMETRY`, `DISABLE_ERROR_REPORTING`, `DISABLE_AUTOUPDATER`,
`DISABLE_BUG_COMMAND` and `DISABLE_FEEDBACK_COMMAND` do, but it does not set them.

## Flag-default risk

Yes: NONESSENTIAL turns off the flag fetch. `DISABLE_TELEMETRY` and `DO_NOT_TRACK` alone do the same. All flags
(about 970 distinct `tengu_*` reads) fall back to the defaults in the code. They do not use the disk cache unless
`CLAUDE_CODE_GB_DISK_CACHE_WHEN_TELEMETRY_OFF` is set (`Qke()`). Verified defaults that matter here:

- `tengu_quizzical_thimble` default `true`: with `CLAUDE_CODE_ENTRYPOINT=sdk-cli` naming stays off. It is off under
  NONESSENTIAL in any case.
- Prompt cache TTL `nBe()`: a subscriber (OAuth), not in overage, gets `1h` when the query source matches
  `S("tengu_prompt_cache_1h_config",{allowlist:DSt})`, with
  `DSt=["repl_main_thread*","sdk","auto_mode","memdir_relevance"]`. So the `-p` main thread (`sdk`) keeps 1h with
  flags off. Subagents (`agent:*`) get 5m unless the server allowlist includes them. Which way the server value
  goes is not visible offline. `tengu_tidy_frost` (forces 5m) defaults to `false`. API-key auth always gets 5m unless
  `ENABLE_PROMPT_CACHING_1H` is set. These override the flag: `CLAUDE_CODE_PROMPT_CACHE_TTL`,
  `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL`, `FORCE_PROMPT_CACHING_5M` and the settings
  `promptCacheTtl`/`subagentPromptCacheTtl`.
- Compaction and fallback: dozens of obfuscated kill-switches and rollouts (`tengu_breezy_blanket`,
  `tengu_compact_cache_prefix`, ...) take their code default. This was not mapped one by one.
- The stream the adapter parses (`system/init`, `status`, `api_retry`, `session_state_changed`, `stream_event`,
  `result`, `rate_limit_event`): no flag or `bt()` gate was found. `session_state_changed` is gated only by
  `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS`. `rate_limit_event` comes from response headers (`FE()`/`bon()`).

## Recommendation for headless loop-manager sessions

Set `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`. It is precise: it cuts only the naming request and title writes, and the
flags stay live. For cost determinism also pin `CLAUDE_CODE_PROMPT_CACHE_TTL` (and the subagent one). Use
NONESSENTIAL where egress must be minimal (tests already do). Accept that flags then take their code defaults, a
sleeping usage-limit wait is not rechecked, and model capabilities come from bundled data.
