# Re-check: the session-title request under a custom `CLAUDE_CODE_ENTRYPOINT`

Date 2026-11-19, `claude` 2.1.327 (mise install). Real API runs: `claude-haiku-5-5` only, prompt caching on.
Spend ≈ $0.06 (37 real requests). Raw logs, request dumps and scripts are kept outside the repository.

## Gate (static, from the embedded JS)

```js
function oMs(){return bt()||s.CLAUDE_CODE_DISABLE_TERMINAL_TITLE}          // bt(): CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC set
function DZi(){return oMs()||(s.CLAUDE_CODE_ENTRYPOINT==="sdk-cli"||s.CLAUDE_CODE_REMOTE_SESSION_ID!==void 0)
  &&!process.env.CLAUDE_AGENT_SDK_VERSION&&!y()&&S("tengu_quizzical_thimble",!0)}
...fn=y.some((J)=>J.type==="user"||J.type==="assistant");BHi(sd,fn||DZi());   // headless: titles.attempted = resumed || DZi()
```

The title is skipped when: the label is exactly `sdk-cli` (and `CLAUDE_AGENT_SDK_VERSION` is unset, flag
`tengu_quizzical_thimble` is true by default), or `CLAUDE_CODE_DISABLE_TERMINAL_TITLE` /
`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` is set, or the session already has messages (`--resume`, `--continue`),
or the first prompt is shorter than 10 characters. It is sent once per session; a failed or unusable reply resets the
latch, so the next turn tries again. The result is written to the transcript as an `ai-title` record (the session
picker and list read it) and, for remote sessions, synced to the remote session. Nothing in `-p` output carries it.

Model: the helper asks for the small fast model (`claude-haiku-5-5`), but only if the base URL is first-party.
With a custom `ANTHROPIC_BASE_URL` (a local proxy, a gateway) it falls back to the main model. The 2026-11-18 run
saw the main model (Sonnet) only because it went through a local proxy.

## Fake endpoint (no tokens)

| case | requests per call |
|---|---|
| unset, `sdk-cli`, `cli` (becomes `sdk-cli`) | 1 |
| `loop-manager`, `sdk-ts`, `claude-vscode` | 2 (main + title) |
| unset + `CLAUDE_AGENT_SDK_VERSION` | 2 |
| `loop-manager` + `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1` or `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | 1 |
| `loop-manager` + `DISABLE_TELEMETRY=1` | 2 |
| `loop-manager`, prompt "say OK" | 1 |
| `loop-manager`, two-turn stream-json | 3 (title once) |
| `loop-manager`, `--resume` / `--continue` | 1 |
| main Sonnet via a local base URL / same + `_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL=1` | title on Sonnet / on `claude-haiku-5-5` |

## Real API (Haiku)

- `loop-manager`: one title request per session, never with `sdk-cli` (6 + 6 single calls, 1 + 1 two-turn sessions).
- The title request carries no `cache_control` and has a different system prompt and no tools, so it shares no
  prefix with the main request: `cache_read = cache_creation = 0` every time. It contains the full first prompt
  (no cap): 1.19k, 5.9k and 48k input tokens for prompts of about 0.03k, 5k and 47k tokens.
- `total_cost_usd` includes it only if it finishes before the result: it was missing in 2 of 6 small `-p` calls; in
  stream-json a later `result` (cumulative) picks it up.
- Cost per session: about $0.0006 for a 5k-token first prompt and $0.0048 for 50k at Haiku input $0.10/MTok. If the
  title goes to Opus 5.5 (custom base URL with Opus as the main model), the same costs are about $0.024 and $0.19.
- Unrelated to the title: on this OAuth account the `loop-manager` main request also carries about 290 more tokens
  (a "Scratchpad directory" note in the context message), which are absent with `sdk-cli`.
