# Prompt-cache TTL and breakpoints: main thread vs native subagents, `sdk-cli` vs `loop-manager`

Date 2026-11-19, `claude` 2.1.327 (mise install), OAuth Max subscription login. Approved measurement, API-priced.
Raw logs, request bodies and scripts are kept outside the repository.

## Static: how the TTL is chosen

`nBe(querySource, {agentCacheTtlOverride, mainThreadModel})` decides the TTL on every model request. In order:

1. `FORCE_PROMPT_CACHING_5M` → 5m.
2. The env var depends on whether the query source is in the built-in list
   `DSt=["repl_main_thread*","sdk","auto_mode","memdir_relevance"]`. It is always this built-in list, never the
   server flag. In the list: `CLAUDE_CODE_PROMPT_CACHE_TTL`. Not in the list: `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL`.
3. Settings `promptCacheTtl` / `subagentPromptCacheTtl`, split the same way.
4. The agent definition's `cacheTtl` (frontmatter, `agentCacheTtlOverride`). A `1h` there is ignored while the
   account is in overage.
5. `ENABLE_PROMPT_CACHING_1H` → 1h. This is the only way an API key gets 1h.
6. Without OAuth, or in overage → 5m.
7. The query source must match the allowlist in the server flag `tengu_prompt_cache_1h_config`, which defaults to
   `DSt`. The allowlist is latched once per session. No match → 5m.
8. `tengu_tidy_frost` (default `false`), for certain main-thread models → 5m.
9. Otherwise 1h (`reason:"subscriber"`).

`CLAUDE_CODE_MESSAGES_PROMPT_CACHE_TTL=5m` lowers only the message breakpoints to 5m.

Query sources:

- The headless `-p` main thread gets `"sdk"`. The string is hard-coded in the SDK/print query path.
- A subagent (Agent tool) gets `agent:custom:<name>`, `agent:builtin:<name>` or `agent:default`.
- The session title request gets `generate_session_title`.
- Compaction gets `compact`.

None of these reads `CLAUDE_CODE_ENTRYPOINT`. The TTL code does not read it either.

The one indirect link: the flag-targeting attributes include `entrypoint` (`Am()` = `CLAUDE_CODE_ENTRYPOINT`). So the
server could target `tengu_prompt_cache_1h_config` by label. The measurement below shows it does not today.

Cached server value for this account (flag map only):

- `tengu_prompt_cache_1h_config` allowlist = `repl_main_thread*`, `sdk`, `auto_mode`, `rolling_compact`,
  `memdir_relevance`, `agent_classifier`, `prompt_suggestion`, `away_summary`, `extract_memories`.
- No `agent:*`, no `compact`, no `generate_session_title`.
- `tengu_tidy_frost` is absent, so it takes its default `false`.
- `tengu_subagent_cache_evict` = `false`. `tengu_system_prompt_global_cache` = `true`.

## Measured

Setup:

- Headless `-p` stream-json through a logging proxy, with `_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL=1`.
- One prompt: the main agent delegates to a custom `reader` subagent (passed with `--agents`, tools `Read`/`Glob`,
  `model: inherit`). The subagent globs and reads three files.
- `--tools Agent,Read,Glob`, `--effort low`, `--setting-sources local`.
- Haiku 5.5, N=2 per cell. Plus one Sonnet 5.5 session per label.
- 85 requests, $0.13 total.

Results were identical in every session of a cell, Haiku and Sonnet alike:

| request | sdk-cli | loop-manager | + `SUBAGENT_TTL=1h` | + `SUBAGENT_TTL=1h`, `PROMPT_CACHE_TTL=5m` (loop-manager) | + NONESSENTIAL (both labels) |
|---|---|---|---|---|---|
| main | 1h | 1h | 1h | **5m** | 1h |
| subagent | 5m | 5m | **1h** | **1h** | 5m |
| side: title | – | no `cache_control`, uncached | – | no `cache_control`, uncached | – (off) |

Where the breakpoints sit:

- **Main** (4 system blocks): `system[2]` is marked `scope:global`; `system[3]` and the last user message are also
  marked. 1h writes are reported in `ephemeral_1h_input_tokens`.
- **Subagent** (3 system blocks): `system[1]`, `system[2]` and the last one or two messages. No `ttl` field (= 5m)
  and no global scope. 5m writes are reported in `ephemeral_5m_input_tokens`. Headers:
  `x-claude-code-request-class: subagent`, agent type `custom`.
- The subagent still reads about 1.8k tokens of system prefix that earlier sessions wrote (5m).
- With the override, the subagent marks carry `ttl:"1h"` and the writes move to the 1h bucket.
- **Title request** (only with a non-`sdk-cli` label): goes to Haiku even when the session runs on Sonnet. About 1.2k
  uncached input tokens.

No compaction or other side call fired in these short sessions. Statically, `compact` would get 5m.

## Verdict

The label does not choose the cache TTL. The main thread is 1h and subagents are 5m under both `sdk-cli` and
`loop-manager`, with or without NONESSENTIAL. The split comes from the query source matched against the allowlist.
The label's only cache-related effect is the extra uncached title request. The env overrides work as their code
says.

## Cost: a subagent with idle gaps over 5 minutes

The 5m entry expires only when the start-to-start gap between two requests of the same subagent goes over 5
minutes. Long total runtime alone does not expire it. Typical cause: a long build or test inside one tool call.

After such a gap, the whole context P is written again at 1.25× instead of read. With 1h the next request reads it,
but every token written costs 2× instead of 1.25×.

Prices per MTok (5m write / 1h write / read):

- Opus 5.5 (input $4): $5 / $8 / $0.20.
- Sonnet 5.5 (input $2): $2.5 / $4 / $0.10.
- Haiku 5.5 (input $0.10): $0.125 / $0.20 / $0.01.

Example for Opus, P = 60k:

- Each long gap on 5m costs 60k × ($5 − $0.20) = **$0.29**.
- The extra cost of 1h over the subagent's whole life is 60k × $3 = **$0.18**, about 0.6 of one gap.
- So one gap near full context already pays for 1h. With no long gaps, 5m is strictly cheaper.
- For loop-manager subagents that run long `cargo` commands, set `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL=1h`, or
  `cacheTtl: 1h` in the agent definition. The main thread already gets 1h.
