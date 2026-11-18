# What CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC changes in claude 2.1.327: server flags, telemetry, limit recheck, REST, error reports (static)

Method: `strings` over the Bun-compiled binary (build 2026-11-17, git f467840), regex and context reads of the minified
JS, plus a read of the on-disk flag cache in `~/.claude.json` (flag maps only; credentials and account fields were not
read). Docs check: plain `curl` of `env-vars.md`, `monitoring-usage.md` and `data-usage.md` under
`code.claude.com/docs/en/`. No model calls. Minified names are local to this build. Snippets are shortened.

Builds on `static.md` (same directory): `bt()` = NONESSENTIAL set, `px()` = NONESSENTIAL or DISABLE_TELEMETRY or
DO_NOT_TRACK, `Eb()` = event logging off (includes `px()`), GrowthBook on only when `Yp()`.

## 1. Server flags versus code defaults

### How a flag is read when NONESSENTIAL is set

```js
getFeatureValueWithSource(e,n){ ...env/config overrides...
  if(!this.deps.isEnabled()&&!this.deps.isDiskCacheReadableWhileDisabled())return{value:n,source:"disabled"};
  ... remoteEval → "payload"; saved flags on disk → "disk"; else "fallback" }
function S(e,n){return pn(e,n).value}
function Le(t,{safe:e}){...pn(t,e)...}           // "safe" getter: the safe value unless served
function Jp(e){switch(e){case"payload":case"override":case"disabled":return!0;case"fallback":return _i();case"disk":return!1}}
```

Under NONESSENTIAL every read returns the code default with `source:"disabled"`. The disk cache is not read
unless `CLAUDE_CODE_GB_DISK_CACHE_WHEN_TELEMETRY_OFF` is set. Some gates check `Jp(source)`, and they treat
`"disabled"` as a real answer. Overrides that still work: env vars named at each read site (`CLAUDE_CODE_*`), settings
keys, and the GrowthBook env/config override maps.

### The cache on disk

`~/.claude.json` has `cachedGrowthBookFeatures` (1452 keys; `cachedFeatureFlags` is an identical copy, fetched
2026-11-19 03:18), `cachedExperimentFeatures` / `cachedExperimentData` (9 experiments) and `clientDataCacheSlots`
(12 slots of `/api/claude_cli/bootstrap` client data, keyed by entrypoint and model, 2 to 7 small capability keys
each). Comparison against the defaults read from the code:

| class | count |
|---|---|
| keys the server sends that this build never reads | 600 |
| read, cached value = code default | 416 |
| read, cached value != code default | 245 |
| default is a local expression or constant (checked by hand where it matters; most resolve equal, e.g. `t0e=0.2`, `Rqn=15000000`, `d_=180000`) | 81 |
| read only through a registered reader or a `"flag"` string, no default visible | 110 |

Of the 245 differing flags, 68 are surface-specific: Remote Control and attached machines (`tengu_violin_*`, 32),
CCR hosted sessions (`tengu_ccr_*`, 16), artifacts (`tengu_cobalt_plinth_*`, 9), kairos push and cron (4), Chrome
(2), VS Code (2), coordinator rows (`tengu_hearth_*`, 2) and the bridge (1). None of them applies to a local `-p`
session. The rest are below, grouped by what a headless coding agent sees.

### A. Text the model sees

| flag | server | default | read site, effect |
|---|---|---|---|
| `tengu_lapis_anchor` | `"off"` | `"padded-countdown"` | `Pqn(){let e=S(Dwt,"padded-countdown")...}`, `bge(e,n){...BA("total_tokens_reminder",ut(e),e)?Pqn():"off"}`. The `<total_tokens>N tokens left</total_tokens>` attachment, budget `tengu_lapis_anchor_budget` = 15M (same both ways). **This is the remaining-token-budget message measured on turn 2.** It applies only when the model has the `total_tokens_reminder` capability. Pin it with `CLAUDE_CODE_TOTAL_TOKENS_REMINDER=off` or the `totalTokensReminder` setting |
| `tengu_dazzling_platypus` | true | false | Environment section: `aQo()?iQo:null`, a one-line `curl --form-string` note (~15 tokens). The long "Downloaded files ... untrusted" paragraph `sQo` is unconditional. The longer untrusted-values paragraph `lQo` is gated by `tengu_reactive_heron`, which is absent from the cache, so it is off both ways |
| `tengu_amber_prism` (experiment, arm 1) | true | false | `xV(e){if(o_()&&S("tengu_amber_prism",!1))return e+e4e}`: suffix on the tool-rejected message |
| `tengu_scalable_whistle` | true | false | Note "The last attempt reached the limit while still thinking." on a max-tokens cut retry |
| `tengu_starry_pebble_enabled` | true | false | After web-search results with no citations: critical reminder "REMINDER: You MUST cite the search results ..." |
| `tengu_sleepy_shore` | `{"enabled":true}` | null | Web-search "fast" mode description, system hint and addendum |
| `tengu_sparkling_hanrahan` | true | false | Pasted-content critical reminder (interactive paste only) |
| `tengu_mcp_subagent_prompt` (experiment) | true | false | MCP output-truncation prompt variant (`MCP_TRUNCATION_PROMPT_OVERRIDE` overrides it) |
| `tengu_lantern_wick_release` | release text | false | Model-visible note "Earlier usage-limit notes no longer apply ..." when a usage-limit grace window ends |
| `tengu_lucid_anchor` | true | false | Wording of the InputValidationError for a deferred tool that was not loaded |
| `tengu_typed_pebble` | 60000 | 0 | Floor of the skill-listing character budget, only when `Ph()` |
| `tengu_cozy_teapot` / `tengu_thrifty_sonic` | `"strict"` / true | `"strict"` (code fallback) / false | Bash-first steering cohort. `qrr()` also turns on the Bash edit-diff path. Env overrides: `CLAUDE_CODE_COZY_TEAPOT`, `CLAUDE_CODE_THRIFTY_SONIC` |
| `tengu_hushed_lark` | 5 | `Nlo` (not resolved) | Turns before the "user hasn't heard from you" reminder. Also gated by the `silent_turn_reminder` capability, env `CLAUDE_CODE_SILENT_TURN_REMINDER_TURNS` |

**Observed prompt differences, explained.** The turn-2 budget message is `tengu_lapis_anchor` (certain). In the
`loop-manager` cells the scratchpad note disappears because `zx()` (scratchpad on) requires `tengu_scratch`
(server false) or artifact-tool eligibility. That eligibility depends on `tengu_cobalt_plinth` (default true for
prosumer OAuth) and on the org policy `allow_cobalt_plinth`, and `sdk-cli` is excluded (`isArtifactSdkDefaultOff`).
That NONESSENTIAL closes this gate through the policy lookup is inferred, not traced to the last call. The ~250-token
shrink of the cached prefix in the plain `sdk-cli` cells is **not fully explained** offline. Only ~15 tokens come
from a flag (`dazzling_platypus`). Most prompt sections are switched by per-model capabilities
(`BA("finishing_work_guidance"|"memory_guidance"|"turn_updates"|...)`). Those come from client data and the model
catalog, and NONESSENTIAL skips both: the bootstrap is skipped and `Xcn()` returns `"essential_traffic"`. The catalog
mode is also a flag (`tengu_delegated_quail`: server `{served:"primary"}`, default `{mode:"off"}`). To settle it for
free, diff the system text of the cell-1 and cell-5 request bodies that the measurement already saved.

### B. Compaction, context, caching

| flag | server | default | effect |
|---|---|---|---|
| `tengu_prompt_cache_1h_config` | allowlist + `rolling_compact`, `agent_classifier`, `prompt_suggestion`, `away_summary`, `extract_memories` | `DSt` = `repl_main_thread*`, `sdk`, `auto_mode`, `memdir_relevance` | With flags off these side queries write 5m cache. The main `-p` thread (`sdk`) keeps 1h. Subagents (`agent:*`) are in neither list, so they get 5m both ways |
| `tengu_amber_moleskin` | `{"default":{"repl":0.15,"sdk":0.15}}` | null → scalar `tengu_amber_rokovoko` 0.2 | Precompute-compaction arm fraction per surface |
| `tengu_russet_abacus` | true | false | Shared budget for truncating unsaved tool-result heads |
| `tengu_bramble_lintel` | 7 | null → 1 | Memory extraction runs every N turns (every turn under NONESSENTIAL, if extraction is on) |
| `tengu_juniper_sundial` | 10 | built-in `TURNS_BETWEEN_MAINTENANCE` | Session-memory maintenance cadence |
| `tengu_wild_tome` | true | false | Main-thread side-query reuse of the parent request (cache sharing) |
| `tengu_rustling_pixel` | `"upgrade"` | `"all"` | Which thinking blocks are kept when the model changes mid-conversation |
| `tengu_vectorized_mitten` | true | false | Subagent queries keep relayed thinking |
| `tengu_mossy_dawn`, `tengu_prancy_charm` | true | false | Queued-command handling and silent-turn-end normalisation in the query loop |
| `tengu_gentle_sphinx` | false | true | Tool-list handling on context clear (`tools_kept_by_clear`) |

### C. Models, fallback, API shape

`tengu_dash_flame` true (server default fallbacks on), `tengu_calm_seal` `["Mythos"]` (model-family allowlist),
`tengu_usage_overage_included_models` (Fable/Mythos list versus the built-in list), `tengu_fgts` true (fine-grained
tool streaming for first-party claude.ai; `CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING` overrides),
`tengu_plum_vx3` true (web search runs on the small helper model), `tengu_clever_orbit` true (experiment: WebFetch
agent; `CLAUDE_CODE_WEB_FETCH_AGENT`), `tengu_curious_tower` + `_stateless_models` `"fable-5-1"`, `tengu_quiet_heron`,
`tengu_stellar_drift` (deferred-tools beta), `tengu_humble_hammock` (echo of wire tool inputs), `tengu_linear_swan`
(declared tool definitions held), `tengu_frolicking_frost` (custom tool schema handling), `tengu_basalt_spur`,
`tengu_lexical_glade` (warning on org change with locked thinking), `tengu_velvety_kay`, `tengu_marbled_teal` (effort
cost index), `tengu_cinder_swift` `"interrupt"`. Equal to the default (no change): `tengu_byte_stream_idle_timeout_ms`
180000, `tengu_hazel_trellis` 3 (max subagent spawn depth), `tengu-off-switch`, `tengu_sage_compass2` (advisor
tool, default true on first party).

### D. Limits, usage, plan UI (TUI or interactive mostly)

`tengu_amber_tally`, `tengu_nifty_lemur`, `tengu_toasty_breeze`, `tengu_lantern_sconce(_copy)`,
`tengu_wise_bear_copy`, `tengu_pewter_summit`, `tengu_saffron_ladder`, `tengu_rustling_blossom`, `tengu_starry_waterfall`
and `tengu_snug_storm` cover limit-wall offers, credit copy and upsells. `tengu_bright_wall` (usage-limit recheck kill
switch) is absent from the cache, so it stays at its default true.

### E. Subagents and teammates

`tengu_flickering_lagoon` (in-process teammate resume), `tengu_jaunty_ember` (agent messaging),
`tengu_eager_pillow` (orphaned tool-use rows), `tengu_mellow_drawer` (typed code-review findings tool; default `ht()`
= remote only, server true), `tengu_luminous_locket` (subagent config warning).

### F. Telemetry, sampling, runtime

`tengu_1p_event_batch_config` (`/api/event_logging/v2/batch`, 30 s, 400/batch), `tengu_event_sampling_config`,
`tengu_agile_glade`, `tengu_log_datadog_events` true, `tengu_iridescent_gem` 0.01 (prefix-break hash sampling, also
`px()`-gated), `tengu_orford_ness` true (error reporting, §5), `tengu_drift_lantern` / `tengu_event_loop_stall`
(stall detector, non-blocking stdout), `tengu_kestrel_moor`, `tengu_cobalt_thicket`, `tengu_session_name_uniqueness`,
`tengu_elegant_pancake` and `tengu_version_config` (`minVersion` 1.0.24).

### G. Interactive or other surfaces only

Prompt suggestion, surveys, onboarding, plugins and marketplace (`tengu_amber_lattice`, `tengu_harbor*`,
`tengu_staged_storm`, `tengu_sdk_mcp_manifests`), MCP connectors (`tengu_eager_raccoon`, `tengu_patient_raccoon`,
`tengu_mcp_claudeai_eligibility_gate`), Remote Control and IDE auto-enable, ultraplan and ultrareview, `/import`,
native cursor, xterm atlas (`tengu_basalt_meadow`).

**Policy flag that flips under NONESSENTIAL:** `tengu_doorbell_agave`, server false, default true:
`pd(){let{value:e,source:n}=pn(cS,!0);return e===!1&&Jp(n)}`. With flags off, `enforce_web_search_mcp_isolation`
is honoured if an org policy sets it. A personal plan is unlikely to have that policy.

### Experiments and holdouts

The cached experiment arms are `tengu_amber_prism` (1, true), `tengu_clever_orbit_experiment` (1, true),
`tengu_flint_harbor` (1, false), `tengu_mcp_subagent_prompt-public` (1, true), `tengu_plank_river_frost` (0,
`"user_intent"`), `tengu_radiant_fox_experiment` (1, true), `tengu_rippling_waffle_experiment` (0, false),
`cai_giggly_dream` → `tengu_swift_lynx` (a $250 cloud-credit promo) and `tengu_ultrareview_awareness`.
`tengu_slate_harbor_experiment` (new `/init`) is false. No holdout-named flag was found. Under NONESSENTIAL the session
is in no arm: it runs the code defaults, which for the active experiments are the pre-experiment behaviour, and no
exposure is logged.

## 2. Telemetry: does NONESSENTIAL stop a user's OpenTelemetry export? **No.**

```js
function xt(){return Ae(process.env.CLAUDE_CODE_ENABLE_TELEMETRY)}
async function ys(e,r){... let n=xt(),a=n?Pe(s.OTEL_METRICS_EXPORTER):[]; if(n)o.push(...await rr(a));
  if(!r?.disableBigQuery&&st())o.push(ir(e)); ... if(n&&(!Y()||SMt())){...log exporters (OTEL_LOGS_EXPORTER)...}
  if(n&&Cni()&&!Y()){...trace exporters...}
function st(){if(px())return!1;let e=ar(),r=Pt()&&(e==="enterprise"||e==="team");return sNt()||r}
```

The user's pipeline (metrics, logs/events, traces via `OTEL_*_EXPORTER` and `OTEL_EXPORTER_OTLP_*`) is gated only by
`CLAUDE_CODE_ENABLE_TELEMETRY`. Traces also need `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA`. The init path (`A0e` → `b` →
`t3i` → `ys`) has no `bt()` or `px()` check, and the event emitter `rl()` has none either. Exporters run by Anthropic
and their gates:

| exporter | gate |
|---|---|
| Anthropic metrics exporter (`ir(e)`, team/enterprise or `sNt()`) | `st()` → off under `px()` |
| 1P event logging `/api/event_logging/v2/batch` | `X5()=!Eb()` → off under `px()` |
| Datadog event logging (`tengu_log_datadog_events`) | `X5()` and the flag |
| GrowthBook fetch and exposure logging | `Yp()` → off under `px()` |
| Datadog error tracking | §5 |
| `/api/claude_code/organizations/metrics_enabled` | not gated (`bypassEssentialTrafficOnly:!0`); it is the org opt-out check for the metrics exporter |

Docs agree: NONESSENTIAL "turns off telemetry" (Anthropic's). The monitoring page ties OTel only to
`CLAUDE_CODE_ENABLE_TELEMETRY`. The env-vars page offers `CLAUDE_CODE_ENABLE_FEEDBACK_SURVEY_FOR_OTEL` for orgs that
"block nonessential traffic but capture ... through their own OpenTelemetry collector", so the user's OTel keeps
running under NONESSENTIAL. Some OTel events depend on features that NONESSENTIAL turns off. For example, the
internal-error event is not emitted when `DISABLE_ERROR_REPORTING` is set.

## 3. The usage-limit recheck

It is code: `LYn` in the retry loop. It is neither prompt nor hooks, and it runs only inside the **retry watchdog**.

```js
let bo=!jn&&(_a||SDe()&&YEt(Fo));            // SDe()=CLAUDE_CODE_RETRY_WATCHDOG; YEt = 429/529
if(Fo.status===429){ea=pws(Fo,y);            // ms until anthropic-ratelimit-unified-reset
  if(ea!==null&&headers["anthropic-ratelimit-unified-status"]==="rejected"){ ...MAX_USAGE_LIMIT_WAIT_MS... Bi=Fo}}
zu=Bi&&o.isUsageLimitLifted?s.CLAUDE_CODE_RETRY_WATCHDOG_USAGE_LIMIT_RECHECK_MS:void 0
async function LYn({rejection:e,...}){
  if(!S("tengu_bright_wall",!0))h="skipped_kill_switch_off";
  else if(bt())h="skipped_essential_traffic_only";
  else if(!Pt()||Te()!=="firstParty")h="skipped_account";
  else if(gL())h="skipped_low_priority";
  else{let k=Wge(e,n);if(k!=="refused")h=`skipped_refusal_${k}`}   // Wge default types ["five_hour","seven_day"]
  ... Sjr → Dt(): messages.create({model:RPe() /*haiku 4.5*/, max_tokens:1, messages:[{role:"user",content:"quota"}]}) }
```

- **Without `CLAUDE_CODE_RETRY_WATCHDOG`** (the loop-manager default): a usage-limit 429 is not waited out. It
  surfaces as an error (the retry-after is too long, or retries run out) and `-p` ends the turn with the limit
  error. The adapter's own `quota_exhausted` park is then the only wait, so nothing changes there.
- **With the watchdog**: the request sleeps until `anthropic-ratelimit-unified-reset` (capped by
  `CLAUDE_CODE_RETRY_WATCHDOG_MAX_USAGE_LIMIT_WAIT_MS` and `_MAX_WAIT_MS`). Only if
  `CLAUDE_CODE_RETRY_WATCHDOG_USAGE_LIMIT_RECHECK_MS` (60000 to 10800000) is set does it probe every so often with a
  `max_tokens:1` Haiku request. It re-sends as soon as the probe is admitted. The probe runs only when all of these
  hold: subscriber, first party, `five_hour` or `seven_day` limit with status `rejected`, no overage or credits in
  use, not low-priority mode, reset still ahead. Not for `seven_day_opus` / `_sonnet` / `overage`.
- **NONESSENTIAL**: no probes (`skipped_essential_traffic_only`). The watchdog still sleeps until the reset. The docs
  say the same ("not while CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC is set").
- Separate from this: `probeQuotaStatus` (the TUI startup `quota_check`) is off in `-p` unless
  `probeInNonInteractiveSession`, and off under `bt()`.

## 4. First-party REST under NONESSENTIAL

The gate in the shared REST client (`Gt`/`Io`, hosts `api`, `claude-ai`, `mcp-proxy`, ...):

```js
function tf(e,n){... if(!e.bypassEssentialTrafficOnly&&!o&&bt())return"essential-traffic-only"; ...}
```

**Blocked** (anything that goes through `Gt`/`Io`):
- `/api/oauth/usage`: `/usage` and the limit-wall utilization reads fail with "Auth error: essential-traffic-only".
- `/api/oauth/account/settings` (Grove, artifact pins), `/api/oauth/organizations/*` (code repos, sync, connectors),
  `/api/oauth/org`, `/api/oauth/files/*`, `/api/oauth/file_upload`.
- `/api/claude_code/settings` / `skills` / `notification/preferences`.
- `/api/claude_cli/bootstrap` (client data, extra model options).
- MCP registry and claude.ai connectors, plugin downloads, feedback, release notes, the model catalog.

**Allowed** (direct HTTP calls, no `bt()` gate):
- OAuth token exchange and **refresh**: `Rke()` → `POST https://platform.claude.com/v1/oauth/token` (refresh grant).
  Login and refresh keep working.
- Profile `GET api.anthropic.com/api/oauth/profile` (`jA()`), roles `/api/oauth/claude_cli/roles`, and
  `create_api_key` during login.
- `/api/claude_code/organizations/metrics_enabled` (explicit bypass).
- The Messages API. The base-URL connectivity preflight (`/api/hello`, `/v1/oauth/hello`) is skipped (`eMs`).

**Subscription detection still works.** `Pt()` (claude.ai subscriber) and `subscriptionType` /
`rateLimitTier` come from the stored OAuth credentials and `oauthAccount`, refreshed by the token and profile calls
above. So the 1h prompt-cache path (`nBe`: subscriber, not in overage, query source on the allowlist) still applies.
The measurement saw 1h writes in every cell. Overage state comes from response headers.

**`rate_limit_event` is unaffected.** It is built from the `anthropic-ratelimit-unified-*` response headers of the
Messages call, and the measurement saw it under NONESSENTIAL.

What breaks or degrades: `/usage`, limit-wall offers (credits, reset), claude.ai connectors and MCP registry, plugin
installs that download, account sync, artifacts, Remote Control, feedback, release notes, auto-update and flags.

## 5. Error reporting

The destination is **Datadog error tracking, not Sentry**. Sentry appears only as a sandbox egress constant
(`SENTRY_INGEST_HOST`) and in the MCP registry (`mcp.sentry.dev`).

```js
var p="https://browser-intake-us5-datadoghq.com/api/v2/logs"   // POST ?ddsource=browser&dd-api-key=<public client key>
function zeo(){if(process.env.DISABLE_ERROR_REPORTING)return!1; if(Y())return!1; if(Eb())return!1; if(YQ())return!1;
  if(Te()!=="firstParty"||!Ki())return!1; if(!semver.gte(VERSION,"2.1.193"))return!1; if(!Fmi())return!1; return l()}
function l(){return S("tengu_orford_ness",!1)}            // server true, default false
function i(t){... if(USE_BEDROCK|VERTEX|FOUNDRY|ANTHROPIC_AWS|...||process.env.DISABLE_ERROR_REPORTING||bt())return; ...}
```

- **When it sends:** handled errors routed through `logError`, plus unhandled rejections and uncaught exceptions.
  Known noise is filtered out (aborts, auth errors, some fs errors). There is a per-process cap and batches flush
  every 30 s (`CLAUDE_CODE_DD_ERROR_TRACKING_FLUSH_INTERVAL_MS`).
- **Gates:** `DISABLE_ERROR_REPORTING` (any value), NONESSENTIAL (`bt()` in `i()`, and `Eb()`), DISABLE_TELEMETRY or
  DO_NOT_TRACK (`Eb()`), the third-party providers, remote sessions, first party with claude.ai OAuth only
  (`Ki()`), the org policy `allow_error_reporting` (`Fmi`: not with an API-key helper or `ANTHROPIC_AUTH_TOKEN`), and
  the flag `tengu_orford_ness`. Under NONESSENTIAL the flag is false as well, so it is off three ways. This matches
  the docs ("On for Pro, Max, and Ultra sign-ins on v2.1.198+").
- **Payload:** `ddtags` (version, platform, OS release, `user_bucket`, entrypoint, node/bun version, model, error code,
  session kind, renderer mode), `message` = class + masked message (≤ 4000), `error.stack` (top 20 frames, ≤ 16000
  chars), fingerprint, handled/unhandled, `host_name_redacted` (12 chars of a hash), and `feature_flags`.
  Messages go through secret and path masking (`M3a(I3a(...))`), and an error thrown outside the bundle is replaced
  by a placeholder. No prompts and no transcript.
- **Other diagnostic uploads**, all `Eb()`/`px()`-gated: 1P event batches (including the ~0.3 to 0.5 MB batch at
  exit seen in the measurement) and Datadog events. `/feedback` and `/bug` upload a transcript only on request, and
  are `bt()`-gated. The session-quality survey can attach a transcript only after an explicit "Yes", and is off under
  NONESSENTIAL. No crash-dump upload was found.

## Takeaways for loop-manager

- NONESSENTIAL does not break login, refresh, the 1h cache for the main thread, `rate_limit_event` or the user's
  OTel. It does break `/usage` and every claude.ai REST feature.
- It changes model-visible behaviour through code defaults: the total-tokens countdown turns on, experiment arms are
  dropped, and side queries use the 5m TTL. Pin `CLAUDE_CODE_TOTAL_TOKENS_REMINDER` if the countdown is unwanted.
- The usage-limit recheck is a watchdog feature. Without `CLAUDE_CODE_RETRY_WATCHDOG`, `-p` fails fast on a limit,
  and the adapter's `quota_exhausted` park stays the only wait.
