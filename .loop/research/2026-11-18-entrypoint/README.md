# CLAUDE_CODE_ENTRYPOINT=loop-manager vs sdk-cli: subscription meter A/B

Date 2026-11-18, `claude` 2.1.327 (mise install), account: Max 20x OAuth (`default_claude_max_20x`).
Spend (API-equivalent): reported `total_cost_usd` sum = $17.12; real ≈ $18.7 (see "hidden title request").

## 1. Label check (no tokens) — confirmed

Real binary against a local fake endpoint (`scripts/fake.py`, `scripts/fake2.py`), `env -i`, dummy API key.
Logs in `label-check/`.

- unset: `User-Agent: claude-cli/2.1.327 (external, sdk-cli)`, system block
  `x-anthropic-billing-header: cc_version=2.1.327.<hash>; cc_entrypoint=sdk-cli;`
- `CLAUDE_CODE_ENTRYPOINT=loop-manager`: `(external, loop-manager)` and `cc_entrypoint=loop-manager;`

## 2. Side effect found: a custom label adds a session-title request

With any non-`sdk-cli` label, headless `claude -p` sends a second, parallel request to the *main model*
(here Sonnet): system "You are naming a coding session…", user content = the whole prompt wrapped in
`<session>…</session>` (`label-check/fake2.log`). With `sdk-cli` this request is not made.
Real traffic confirms it: `proxy.jsonl` has 63 requests for 63 sdk-cli calls and 82 for 41 loop-manager calls.
`total_cost_usd` includes the title request only if it finishes before the result (29 of 41 calls), so the
reported cost under-counts. For large fresh prompts this nearly doubles input spend under the custom label.

## 3. Meter

- No 5-hour window exists on this account now: `/api/oauth/usage` returns `five_hour: null`, `limits` lists
  only weekly ones, responses carry no `anthropic-ratelimit-unified-5h-*` headers, and `rate_limit_event`
  carries only `seven_day`. The 5-hour question cannot be measured here; the weekly meter was used.
- Resolution: both sources are integer percent. Header `anthropic-ratelimit-unified-7d-utilization`
  (= `rate_limit_event.unifiedWindows.seven_day.utilization`) behaves as floor (0.07 while the endpoint said 8),
  `/api/oauth/usage` `seven_day.utilization` as round; combined, ~0.5 point resolution. The endpoint returns
  429 when polled every ~3 s.

## 4. Run (Sonnet 5.5, 20000 pseudo-words ≈ 65k fresh input tokens per call, 4 output tokens, `--tools ""`,
`DISABLE_PROMPT_CACHING=1`, empty cwd, `--setting-sources local`, through logging proxy `scripts/proxy.py`)

| segment | label | calls | reported $ | real $ | meter |
|---|---|---|---|---|---|
| calib | sdk-cli | 31 | 4.03 | 4.03 | start [7.0,7.5) → [7.5,8.0) |
| calib2 | sdk-cli | 31 | 4.03 | 4.03 | stays in [7.5,8.0) |
| blockA 1–7 | loop-manager | 7 | 1.42 | ≈1.8 | crosses 8.0 at call 7 |
| blockA 8–41 | loop-manager | 34 | 7.62 | ≈8.8 | header still 0.08 at end; endpoint 9 (≥8.5) at 07:18:36, 3.5 min after the last call |

Per call: `calib*/calls.jsonl`, `blockA/calls.jsonl`; stream-json in `*/call_N.jsonl`; meter in `usage.jsonl`,
`blockA/usage.jsonl`, `proxy.jsonl`.

Derived: ≈$9.9 moved the meter 0.5–1.0 point → $/1% weekly ≈ $10–20 for Sonnet. sdk-cli segment inside
[7.5,8.0): ≥$4.03 + A ≈$1.8 < 0.5 pt → >$11/1% (mostly sdk-cli). loop-manager after 8.0: ≈$8.8 real
reached ≥8.5 (possibly helped by lag or other activity) → ≤≈$18/1%. Ratio sdk-cli : loop-manager in
meter-per-dollar is bounded only to roughly ≤1.6 — no ABBA was possible.

Idle drift: endpoint read 9 at 07:18:36, 07:20:10, 07:21:26, 07:22:41, 07:23:57 (no change in ~5 min after the
run); other activity on the account cannot be resolved below 0.5 point.

## Verdict

Inconclusive for the stated question; no evidence of a 1.8–2.5× effect on the weekly meter (a weak bound
against it). The budget is ~20× too small: 4 blocks × 4–5 points would need ≈$200–400. The practical finding
is the opposite cost: `loop-manager` as the label triggers a full-prompt title request on the main model.

Raw data (stream-json transcripts, proxy and fake-endpoint logs, usage polls) is kept outside the repository: it holds the harness system prompt and machine paths.
