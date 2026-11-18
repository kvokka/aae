# Traffic switches of `claude` under a realistic tool load (Sonnet 5.5)

Date 2026-11-19, `claude` 2.1.327, OAuth subscription login. Approved measurement, API-priced from the proxy's
per-request `usage` × list prices (not `total_cost_usd`). Follow-up to `measure.md`, which used 2-turn sessions
without tools; its per-session differences were fractions of a cent. Raw logs, request bodies and scripts are kept
outside the repository.

## Method

- Same proxies as `measure.md`: a logging proxy as `ANTHROPIC_BASE_URL` (`_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL=1`)
  and a CONNECT proxy for all other HTTPS (logs only host and bytes). Auth headers are never logged.
- Production shape: `claude -p --input-format stream-json --output-format stream-json --verbose`,
  `--model claude-sonnet-5-5`, effort default (medium, adaptive thinking), caching on,
  `--tools Read,Grep,Glob,Bash,Write`, `--permission-mode bypassPermissions`, `--setting-sources project,local`,
  `--strict-mcp-config`, `--max-turns 30` per message (never reached).
- Workload: a scratch git repo (four crates of this repository, about 8k lines of Rust, one fixed commit), copied
  fresh to the same path before every session. Six stream-json messages, each sent after the previous `result`:
  summarize the crates; read one crate in full and explain its session start; list every child-process spawn with
  `file:line` into `NOTES.md`; read a second crate in full and append a comparison; count files and lines with
  Bash and append a table; verify every reference with Grep and reply `DONE`. Each session reaches ~110k tokens of context.
- Cells: 1 label unset (`sdk-cli`); 2 `CLAUDE_CODE_ENTRYPOINT=loop-manager`; 3 = 2 +
  `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`; 4 = 2 + `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`; 5 = 1 +
  `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`. Order ABCDE, EDCBA, ABCDE; N = 3 per cell (a fourth round did not
  fit the budget).
- Prices per MTok: Sonnet 5.5 in 2 / out 10 / cache read 0.10 / 1h write 4; Opus 5.5 is exactly twice that;
  Haiku 5.5 0.10 / 0.50. The Opus column re-prices main requests at Opus and keeps naming on Haiku 5.5.

## Per cell (mean per session; all writes are 1h, 5m writes are 0 everywhere)

| cell | requests main + naming | input | 1h write | cache read | output | $ Sonnet (min–max, sd) | $ cell | tool calls | turns | $ Opus proj. |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 sdk-cli | 21.3 + 0 | 53 | 108.3k | 1.30M | 10.96k | 0.673 (0.659–0.680, 0.010) | 2.02 | 34.0 | 40.0 | 1.346 |
| 2 loop-manager | 20.7 + 1 | 1279 | 115.2k | 1.33M | 12.07k | 0.715 (0.714–0.716, 0.001) | 2.14 | 34.7 | 40.7 | 1.430 |
| 3 = 2 + TITLE | 21.3 + 0 | 53 | 112.5k | 1.35M | 11.37k | 0.699 (0.693–0.711, 0.008) | 2.10 | 34.0 | 40.0 | 1.397 |
| 4 = 2 + NONESSENTIAL | 21.7 + 0 | 53 | 113.0k | 1.37M | 11.36k | 0.703 (0.661–0.731, 0.030) | 2.11 | 34.3 | 40.3 | 1.406 |
| 5 = 1 + NONESSENTIAL | 20.3 + 0 | 51 | 110.4k | 1.23M | 10.84k | 0.673 (0.657–0.689, 0.013) | 2.02 | 33.3 | 39.3 | 1.345 |

No quota check, `count_tokens` or other side call in any cell. Naming is one Haiku 5.5 request per session (~1.2k
input, no cache), $0.00013. Other hosts per session: cells 1–3 about 5 direct `api.anthropic.com` connections and 5
to the Datadog intake; cells 4–5 one `api.anthropic.com` connection and nothing else.

## Prompt and tool set (first main request bodies)

- 1 vs 2/3: the label adds the scratchpad-directory note to the context message (+295 tokens; the path holds the
  session id, so this part of the first message is never reused across sessions). System prompt and tools identical.
  The billing system block carries the label.
- 2 vs 4 (and 1 vs 5): NONESSENTIAL removes the server-side `advisor` tool (Opus 5.5, deferred), the
  `eager_input_streaming` flag on every tool, a privacy section and a pasted-content section of the system prompt,
  a shell hint and the scratchpad note from the context message, and rewords one context line. First request: 7041
  tokens vs 7361 (cell 1) and 7656 (cells 2/3). The `advisor` tool was never called in any session.
- 4 vs 5: only the label in the billing block differs. 2 vs 3: main requests are identical; only naming differs.

## Outcome

Same in every cell: all 15 sessions finished all six messages, replied `DONE`, found the same 12 spawn sites with
correct `file:line`, and wrote the same three sections and size table. Tool calls (33–37) and turns (39–41) do not
differ by cell.

## Findings

- **No difference above noise.** Session cost is very stable within a cell (sd ≤ $0.03, ≤ 4%), but cells with
  identical main prompts still differ by $0.016 (2 vs 3) and $0.030 (4 vs 5) per session. That is the noise floor
  between cells: different read paths, ±1k output and ±5k write tokens. The largest gap (2 vs 1, +$0.042, +6%) is
  the same size, and 3 and 4 fall between 1 and 2.
- **Systematic cost of each switch, from token accounting, per session at Sonnet (Opus):** naming $0.00013
  ($0.00013, stays on Haiku); scratchpad note ≈ 295 tokens × 21 cache reads + one 1h write ≈ $0.002 ($0.004);
  label `loop-manager` total ≈ +$0.002 ($0.004); TITLE removes only the naming, −$0.00013; NONESSENTIAL ≈ −$0.001
  vs cell 1 ($0.002) because of a 320-token shorter prefix. That is 0.02–0.4% of a $0.67 session: 10–100× below
  the noise between cells.
- **NONESSENTIAL is not cost-neutral in kind:** besides cutting telemetry and naming it removes the Opus `advisor`
  tool. The advisor was not used here; in a session that uses it, the advisor would add Opus-priced tokens that only
  cells 1–3 can incur.
- API-priced cost cannot show how the subscription weights a session by label or entrypoint on the server side.

Spend: $10.58 API-priced in total: cells $10.39, calibration $0.20 (two Haiku 5.5 sessions, one light Sonnet session).
