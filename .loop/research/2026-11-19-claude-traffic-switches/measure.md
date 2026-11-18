# Traffic switches of `claude` on the wire: label, title, non-essential traffic, `-p` vs TUI

Date 2026-11-19, `claude` 2.1.327 (mise install), OAuth subscription login. Approved measurement, API-priced.
Raw logs, request bodies and scripts are kept outside the repository.

## Method

- Every request to the base URL goes through a local logging proxy (`ANTHROPIC_BASE_URL` = proxy,
  `_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL=1` so side calls are routed as in production). The proxy logs path,
  model, `max_tokens`, system-block kind, `cache_control` count and the full `usage` object, never auth headers.
- All other outbound HTTPS goes through a CONNECT proxy (`HTTPS_PROXY`, `NO_PROXY` for the base URL) that logs only
  host, bytes and duration. Anything that ignores the proxy variables would be invisible.
- Workload per session: first prompt is a fresh synthetic service log (~5k tokens, ~7.8k with the context message)
  plus a one-line question; second turn "reply with one word". `--tools ""`, `--effort low`, `--setting-sources local`,
  `--strict-mcp-config`, prompt caching on. `-p` cells use the production shape
  (`-p --input-format stream-json --output-format stream-json --verbose`), second message sent after the first
  `result`. TUI cells drive a real `claude` in tmux (bracketed paste, Enter, wait for idle, `/exit`).
- N = 3 sessions per cell, round-robin, `claude-haiku-5-5`. Tables use the warm sessions 2–3: the system prefix is
  cached across sessions, so the first session of a new prompt variant pays a cold write.
- Prices per MTok: Haiku 5.5 in 0.10 / out 0.50 / read 0.01; Sonnet 5.5 2 / 10 / 0.10; Opus 5.5 4 / 20 / 0.20;
  Haiku 4.5 1 / 5 / 0.10; cache write 1.25× (5m) or 2× (1h) of input. The Opus column re-prices main and
  suggestion requests at Opus and keeps naming on Haiku 5.5 (where it goes with a first-party base URL).

## Per cell (warm sessions, mean per two-turn session)

| cell | requests | main 1h write | main read | naming in | $ Haiku | $ Opus proj. | other hosts |
|---|---|---|---|---|---|---|---|
| 1 `-p` sdk-cli | 2 main | 7602 | 12685 | – | 0.00167 | 0.0642 | api 5, datadog 1 |
| 2 `-p` loop-manager | 2 main + naming | 8230 | 12708 | 7842 | 0.00259 | 0.0703 | api 5, datadog 1 |
| 3 = 2 + DISABLE_TERMINAL_TITLE | 2 main | 8186 | 12664 | – | 0.00181 | 0.0697 | api 5, datadog 1 |
| 4 = 2 + DISABLE_NONESSENTIAL | 2 main | 7728 | 12180 | – | 0.00169 | 0.0651 | api 1 |
| 5 = 1 + DISABLE_NONESSENTIAL | 2 main | 7722 | 12179 | – | 0.00168 | 0.0649 | api 1 |
| 6 TUI (`cli`) | quota + 2 main + naming + suggestion | 8326 | 13080 | 7949 | 0.00286 | 0.0751 | api 4, datadog 2, downloads 1 |
| 7 TUI + DISABLE_NONESSENTIAL | 2 main | 7808 | 12432 | – | 0.00171 | 0.0658 | none |
| Sonnet 5.5, cell 2 | 2 main (Sonnet) + naming (Haiku) | 9021 | 10478 | 7920 | 0.0382 | – | api 5, datadog 1 |

Request kinds: naming has no tools and no `cache_control`, so it never reads or writes cache. TUI quota check is
`claude-haiku-4-5`, `max_tokens` 1, ~8 input tokens. TUI suggestion is a fork of the main context after the turn
(~0.5k input, ~10.7k cache read, 2 output). No `count_tokens` or other non-Messages request hit the base URL.
"api" = direct TLS connections to `api.anthropic.com` that bypass the base URL (with defaults, three ~2 KB/5 KB
at start, one 0.3–0.5 MB upload at exit, one ~7 KB/35 KB long-lived); one ~2 KB/5 KB connection remains in `-p`
under NONESSENTIAL; none in the TUI. One TUI session also fetched ~0.45 MB from `raw.githubusercontent.com`.

## Findings

- **Cache TTL**: every cache write in every cell is 1h (`ephemeral_1h_input_tokens`; 5m is always 0), Haiku and
  Sonnet, `-p` and TUI. No switch changes the TTL.
- **NONESSENTIAL makes `loop-manager` look like `sdk-cli`**: same request set, same tokens (cells 4 and 5 differ by
  < 10 tokens). It removes the naming request and also the ~300-token scratchpad note that the custom label adds.
  The label itself stays visible: User-Agent and the billing system block still carry `loop-manager`.
- **NONESSENTIAL also changes the main prompt** (most likely because feature flags are not fetched): the cached
  system prefix shrinks ~300 tokens and turn 2 gains a remaining-token-budget system message. Same for `sdk-cli`.
  Token cost is neutral (+$0.00001/session on Haiku vs cell 1), but the model sees a different prompt variant.
- **DISABLE_TERMINAL_TITLE** removes only the naming request; the scratchpad note stays (cell 3 ≈ cell 1 + 600
  write tokens).
- **Cost per session vs cell 1** (Haiku / Opus projection): label `loop-manager` +$0.00092 / +$0.0061; of that,
  naming ≈ $0.0008 (Haiku, ~7.8k uncached input, grows with the first prompt) and the scratchpad note ≈ 620 extra
  1h-write tokens ($0.00013 / $0.005 at Opus, so at Opus prices the note costs more than the naming). With
  TERMINAL_TITLE: +$0.00014 / +$0.0055. With NONESSENTIAL: +$0.00002 / +$0.0009.
- **TUI vs `-p`**: the default TUI adds quota check, naming and a suggestion call (+$0.0012 Haiku, +$0.011 Opus
  projection per session vs cell 1). TUI with NONESSENTIAL is the same two main requests as `-p` and has no outbound
  connection other than the base URL.
- **`rate_limit_event`** is in the stream-json output of every `-p` session, NONESSENTIAL included (TUI has no
  stream-json).
- The pseudo-word prompt of the earlier study triggered a Haiku 5.5 safeguard refusal (category `bio`) and the CLI
  then retried once, doubling the main requests. This study used a synthetic log instead.

## Limits

API-priced cost shows only traffic differences. It cannot show how the subscription weights a session by label,
entrypoint or client on the server side. N = 2 warm sessions per cell; output tokens vary by ±30 between sessions.

Spend: $0.169 API-priced for all 74 logged requests (including smoke runs), of which $0.118 is the Sonnet row.
