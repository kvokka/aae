# 15. Codex CLI под оркестратором без tmux: ACP-адаптер, `codex app-server`, `codex exec --json`

Дата: 2026-11-10. Стенд: `codex-cli 0.166.0` (aarch64 Linux), `@agentclientprotocol/codex-acp 2.6.0`, клоны
`src/codex` (HEAD aa39ded, 2026-11-10), `src/codex-acp` (архив zed-industries), `src/codex-acp-new` (= `src/codex-acp-current`,
HEAD ab7a968). Лаборатория: `lab/codex/` (скрипты `scripts/e1…e10*.py`, сырые логи JSON-RPC `out/*.jsonl`, выводы `out/*_run.txt`).
Изолированный `CODEX_HOME=lab/codex/home`; `~/.codex` не менялся. Модель `gpt-6-luna`, effort `low`, промпты «Reply with exactly: OK».

Важная оговорка о стенде: владелец ходит в модель не через ChatGPT-логин, а через кастомный `model_provider` (прокси, `env_key`);
`codex login status` → `Not logged in` **[ПРОВЕРЕНО]**. Поэтому лимиты подписки вживую не проверялись — сбои воспроизведены на
локальном фейковом провайдере (`scripts/fake_provider.py`), который отдаёт те же тела/заголовки, что разбирает код codex.

## 1. Выводы за 60 секунд

- **Rust-адаптер `zed-industries/codex-acp` мёртв** (архивирован, npm `deprecated`). Актуальный — `@agentclientprotocol/codex-acp`
  (TypeScript, Zed + JetBrains, релизы почти еженедельно). Он **не линкует codex in-process, а спавнит дочерний `codex app-server`**
  и транслирует ACP ↔ app-server. Нативного ACP в самом codex нет.
- **Настоящий «родной» интерфейс codex — `codex app-server`** (JSON-RPC, ~280 методов и нотификаций). На нём внутри сидят и TUI, и
  `codex exec`, и ACP-адаптер. ACP для codex — это сужающая обёртка над ним.
- **H4 для codex подтверждается измерением**: один app-server держит N тредов; +1 тред ≈ +5–10 МБ RSS против ≈155–180 МБ на
  отдельный процесс TUI/exec. 4 треда в одном процессе — 182 МБ (измерено); 4 TUI — ≈730 МБ (экстраполяция 4×182). ACP-адаптер добавляет
  сверху ≈100 МБ (node).
- **H10 для codex подтверждается экспериментом**: к app-server оркестратора (`--listen unix://PATH`) штатный TUI цепляется
  `codex --remote unix://PATH resume <thread>`; человек и оркестратор видят один и тот же тред и события друг друга, steer/interrupt
  работают с любого соединения. Через ACP-адаптер так нельзя (stdio, один клиент).
- **Сбои в app-server структурированы** (`error{willRetry, codexErrorInfo}`, `turn/completed status=failed`, `thread/status=systemError`,
  `account/rateLimits/updated` с `resetsAt`). **В стандартном ACP они теряются**: 429 и 503 приходят как `stopReason: end_turn`
  с текстом ошибки в виде сообщения агента. Типизация есть только через вендорное расширение `_meta.jetbrains.air.sessionFailure`.
- **Краш переживается**: `kill -9` app-server → клиент видит EOF мгновенно, дочерние команды умирают вместе с сервером, тред
  возобновляется `thread/resume` (ход помечен `interrupted`), но результат незавершённого tool-call потерян (модель отвечает UNKNOWN).
  ACP-адаптер сам перезапускает app-server и сам возобновляет сессию при следующем запросе.
- **Per-thread конфиг в одном процессе реален** (app-server): cwd, model, effort, developerInstructions, `config`-оверрайды
  (хуки, MCP, env через `shell_environment_policy.set`, `bypass_hook_trust`). Но `CODEX_HOME` (auth, скиллы, плагины, пользовательский
  конфиг), окружение процесса и (в 0.166) модельный провайдер — одни на процесс.
- **Хуки у codex есть и работают под app-server/ACP**: 14 событий, формат как у Claude Code (`hooks.json`), 4 типа обработчиков,
  система доверия по хэшу. Gas City уже кладёт их в `.codex/hooks.json` агента.
- **MCP не шарится**: два треда с одинаковым stdio-MCP ⇒ два процесса MCP-сервера. Шаринг возможен только через HTTP-MCP,
  поднятый оркестратором один раз.
- **Писатель у треда один** (`thread … already has an active writer`): нельзя параллельно открыть тот же тред вторым процессом
  (`codex resume` рядом с оркестратором) — только через тот же сервер.
- **Зрелость**: `app-server` помечен `[experimental]`, websocket-транспорт «experimental and unsupported», стабильный релиз codex
  раз в 2–8 дней, адаптер пинит конкретную версию codex. Демон app-server у владельца прямо сейчас сломан (сокет исчез, процесс жив).
- **Политика**: документация OpenAI прямо говорит, что app-server-аутентификация ChatGPT-подпиской «has never been permitted for
  commercial or hosted services»; локальный личный оркестратор над официальным CLI под запрет не попадает, но это серая зона.

## 2. Факты

### 2.1. Что сейчас «codex по ACP»

| Факт | Источник |
|---|---|
| `zed-industries/codex-acp`: «Development has moved to agentclientprotocol/codex-acp. The new adapter is built on the new Codex App Server» | `src/codex-acp/README.md:4` **[ПРОВЕРЕНО]** |
| Старый адаптер линковал `codex-core` in-process (тег `rust-v0.137.0`), ACP crate `=0.14.0` | `src/codex-acp/Cargo.toml:21,27` **[ПРОВЕРЕНО]** |
| Репозиторий архивирован (`archived=true`, pushed 2026-07-22); npm: `deprecated: replaced by @agentclientprotocol/codex-acp` | `gh api repos/zed-industries/codex-acp`, `npm view @zed-industries/codex-acp` **[ПРОВЕРЕНО]** |
| Новый адаптер: npm `2.6.0` (2026-11-08), `preview 2.6.1-preview.2`; зависимости `@openai/codex ^0.166.0`, `@agentclientprotocol/sdk ~2.4.0`; npm-maintainers `benbrandt`, `aguzubiaga`, `cirwin`; среди авторов коммитов адреса `jetbrains.com` | `npm view @agentclientprotocol/codex-acp`; `git log` в `src/codex-acp-new` **[ПРОВЕРЕНО]** |
| Адаптер спавнит `codex app-server` (stdio) — один на процесс адаптера, все ACP-сессии = треды в нём | `src/codex-acp-new/src/index.ts:70`, `src/CodexJsonRpcConnection.ts:37`; дерево процессов в E4/E7 **[ПРОВЕРЕНО]** |
| Адаптер тащит свой бинарник codex (node_modules 411 МБ); системный подставляется `CODEX_PATH` | `du -sh lab/codex/acp/node_modules`; README адаптера **[ПРОВЕРЕНО]** |
| `initialize` → `protocolVersion: 1`; ACP v2 поддержан как experimental (`sdk/experimental/v2/unstable`); сам ACP v2 — «Available in Draft» | E4; `src/codex-acp-new/src/AcpAgentRouter.ts:1-2`; `src/agent-client-protocol/docs/updates.mdx:50-54` **[ПРОВЕРЕНО]** |
| Нативного ACP в `openai/codex` нет (grep по `agent-client-protocol` в клоне пуст); реестр ACP указывает на адаптер | `src/agent-client-protocol/docs/get-started/agents.mdx:17`, `registry.mdx:170-171` **[ПРОВЕРЕНО]** |
| `codex mcp-server` в 0.166 отсутствует (подкоманды нет в `codex --help`) — этот путь закрыт | `codex --help` **[ПРОВЕРЕНО]** |
| `codex exec` и TS-SDK: exec работает на `InProcessAppServerClient`; SDK «spawns the CLI and exchanges JSONL» | `src/codex/codex-rs/exec/src/lib.rs:22`; `src/codex/sdk/typescript/README.md:5` **[ПРОВЕРЕНО]** |

`agentCapabilities` адаптера (E4): `loadSession`, `sessionCapabilities{resume, list, close, delete, fork, title, additionalDirectories,
subagents, systemPrompt}`, `mcpCapabilities{http:true, sse:false}`, `_meta.steering.supported`. При кастомном провайдере из auth-методов
остаётся только `api-key`, но `session/new` проходит без `authenticate` **[ПРОВЕРЕНО]**.

### 2.2. `codex app-server`: протокол и транспорт

- JSON-RPC без поля `jsonrpc`, stdio = JSONL. `--listen stdio:// | unix:// | unix://PATH | ws://IP:PORT | off` (`codex app-server --help`).
  **Unix-сокет говорит WebSocket** (HTTP upgrade), а не сырой JSONL (`app-server-transport/src/transport/unix_socket.rs:1`);
  `codex app-server proxy` — тупой байтовый мост stdio↔UDS (`stdio-to-uds/src/lib.rs:12`). Каталог сокета обязан быть 0700 — иначе
  `socket parent must be owned by the user…` **[ПРОВЕРЕНО, E2]**.
- Методы (282 строки сопоставлений в `app-server-protocol/src/protocol/common.rs`): `thread/start|resume|fork|archive|delete|read|list|loaded/list|
  compact/start|inject_items|unsubscribe|goal/*|queue/*`, `turn/start|steer|interrupt`, `review/start`, `model/list`, `hooks/list`,
  `skills/list`, `plugin/*`, `mcpServerStatus/list`, `config/read|value/write`, `account/read|login/start|rateLimits/read`,
  `server/diagnostics`. Нотификации: `thread/started|status/changed|compacted|tokenUsage/updated`, `turn/started|completed`,
  `item/started|completed|agentMessage/delta|commandExecution/outputDelta`, `hook/started|completed`, `error`,
  `account/rateLimits/updated`, `mcpServer/startupStatus/updated`. Серверные запросы: `item/commandExecution/requestApproval`,
  `item/fileChange/requestApproval`, `item/tool/requestUserInput`, `mcpServer/elicitation/request`.
- Версионирование: схема генерируется из бинарника (`codex app-server generate-ts|generate-json-schema`), «each output is specific to
  the Codex version you ran»; экспериментальные методы — за `capabilities.experimentalApi`
  (https://learn.chatgpt.com/docs/app-server) **[ПРОВЕРЕНО]**. В `codex --help` подкоманда помечена `[experimental]`
  (`cli/src/main.rs:171`). «WebSocket transport is experimental and unsupported» (там же).

**Много тредов и клиентов — эксперименты.**

E1 (`scripts/e1_appserver_mem.py`, `out/e1.jsonl`): один процесс, 4 треда с разными `cwd`, 4 хода параллельно за 5.7 с, все `completed`.

E2 (`scripts/e2_multiclient_kill.py`): сервер на `unix://`, два независимых WebSocket-клиента A и B.
B после `initialize` видит тред A в `thread/loaded/list`, делает `thread/resume` на **идущем** ходе (`status: active`), шлёт `turn/steer`
с `expectedTurnId` → `{"turnId": …}`; оба клиента получают одинаковые `item/*` и `turn/completed`, финальный ответ `STEERED`.
Ход, начатый B, прерывается `turn/interrupt` от A → у обоих `turn/completed status=interrupted` **[ПРОВЕРЕНО]**.

E8 (`scripts/e8_tui.py`, `out/e8_run.txt`): штатный TUI `codex --remote unix://SOCK resume <threadId>` открыл тред оркестратора,
показал его историю (`ORCH-MARKER-1`), человек ввёл промпт — соединение оркестратора получило полный поток
(`turn/started … item/agentMessage/delta ×8 … turn/completed`, текст `HUMAN-MARKER-2`). Затем тот же тред продолжен
`codex exec --json resume <id>` (`EXEC-MARKER-3`). В rollout одного файла — три пишущих клиента подряд **[ПРОВЕРЕНО]**.

E11: то же на управляемом демоне (`codex app-server daemon start` в lab-home): два клиента, `thread/loaded/list` общий,
`server/diagnostics` → `residentMemoryBytes: 189153280`, `core.threads.live: 1` **[ПРОВЕРЕНО]**.

E10 (`scripts/e10_env_lock.py`): второй **процесс** app-server на `thread/resume` того же треда получает
`-32600 thread … already has an active writer` (`rollout/src/writer_lock.rs:83`) **[ПРОВЕРЕНО]**.

**Демон.** TUI 0.166 по умолчанию цепляется к общему фоновому серверу, кроме исключений: `--no-daemon`, `--oss`, `--profile`,
любые `-c` кроме `features.*`, «custom provider or gateway sign-in» (`tui/src/daemon_startup.rs:19,78,115`). Есть `codex agents`
(«Browse all agent sessions on the shared local app-server daemon»), `codex queue --thread --message [--resume]`,
`codex remote-control`. Демон: pid-файлы в `$CODEX_HOME/app-server-daemon/`, сокет-симлинк в `/tmp/codex-daemon-<uid>/<hash>`,
отдельный процесс-апдейтер, который сам качает новую версию и **перезапускает сервер** («active or queued work may be interrupted»,
`app-server-daemon/README.md:68-69`); «per-client environment isolation is not provided» (`:22-24`).
У владельца: демон `0.167.0` (pid 183053, с 6 ноября) и апдейтер (с 18 октября) живы, но `codex app-server daemon version`
→ `failed to connect to ~/.codex/app-server-control/app-server-control.sock: No such file` — симлинк висит в пустоту, в логе апдейтера
ежечасно `daemon_not_ready` **[ПРОВЕРЕНО]**. В lab `daemon stop` остановил сервер, но оставил апдейтер (≈89 МБ RSS) — убит вручную по PID.

### 2.3. Память (H4) — измерено

RSS/PSS из `/proc/<pid>/status|smaps_rollup`, контексты крошечные (≈10k токенов на ход).

| Конфигурация | 0 сессий | 1 | 2 | 4 | Источник |
|---|---|---|---|---|---|
| `codex app-server` (stdio), RSS, МБ: idle → после хода | 93 | 132 → 155 | 158 → 168 | 172 → 182 | консольный вывод E1; сырой лог `out/e1.jsonl` |
| ACP-адаптер: node + app-server, суммарный RSS, МБ | 209 (108+101) | 249 → 278 | 282 → 296 | 300 → 292 | консольный вывод первого прогона E4 (не сохранён); повтор на 2 сессиях — `out/e4_run2.txt` |
| то же, PSS, МБ | 172 | 213 → 243 | 247 → 261 | 265 → 250 | E4 |

| Процесс на сессию | RSS | Источник |
|---|---|---|
| TUI `codex --no-daemon` (встроенный сервер), простаивает, без хода | 182 МБ | E8 (1) |
| TUI-клиент `codex --remote` (тонкий) | 98 МБ (PSS 69) | E8 (2) |
| `codex exec --json resume` (пик, VmHWM) | 155 МБ | `scripts/e9_exec_resume.py` |
| Управляемый демон: сервер / апдейтер | 148–189 МБ / 84–89 МБ | E11, `ps` |
| stdio MCP-сервер (python-заглушка), на каждый тред свой | ≈18 МБ × N | E7 |

**[ВЫВОД]** Предельная цена треда в общем app-server ≈ 5–10 МБ против ≈155–180 МБ на процесс: на 4 агентах 182 МБ вместо ≈730 МБ,
на 15 агентах (лимит `agents.max_threads` у владельца) — порядка 0.3 ГБ вместо ≈2.7 ГБ (экстраполяция, **[НЕ ПРОВЕРЕНО]** на длинных
историях: история треда живёт в памяти процесса). ACP-адаптер стоит фиксированные ≈100 МБ RSS сверху. В старте app-server
кратковременно порождает `git`/`git-remote-https` (≈30 МБ; назначение по коду не выяснял), а на первый ход сессии — login-shell
пользователя для снимка окружения (`zsh -lc`, `mise hook-env`, `gc completion zsh`, суммарно ≈145 МБ на пару секунд,
`out/e4_run2.txt:10`) — транзиентные пики, не входящие в цифры таблицы.

### 2.4. Фича × путь

| Фича | app-server (JSON-RPC) | ACP (`codex-acp` 2.6.0) | `codex exec --json` | tmux + TUI |
|---|---|---|---|---|
| Много сессий в процессе | да (E1) | да, треды одного дочернего app-server (E4) | нет, процесс на ход | нет |
| Несколько клиентов / attach человека | да: unix/ws, `codex --remote`, `codex agents` (E2, E8) | нет (stdio, 1 клиент) | нет | да, `tmux attach` |
| Resume / fork | `thread/resume`, `thread/fork` (E3) | `session/load`, `session/resume`, fork (E4) | `exec resume|fork <id>` (E9) | `codex resume|fork` |
| Модель / effort на сессию и на ход | `thread/start{model}`, `turn/start{model,effort}` (E3) | configOptions `model`, `reasoning_effort` (есть баг #336) | `-m`, `-c` | `-m`, `/model` |
| Sandbox / approvals | `sandbox`, `approvalPolicy`, серверные запросы approval | режимы `read-only…agent-full-access`, `session/request_permission` | флаги | UI |
| Произвольный конфиг на сессию | `config{}` = слой SessionFlags на тред (E3, E10) | нет: только `CODEX_CONFIG` на процесс (issue #215, #229 открыты) | `-c`, `--profile` | `-c`, `--profile` |
| MCP на сессию | `config.mcp_servers`, статус с `threadId` (E3) | `session/new{mcpServers}` stdio/http (E7) | `-c mcp_servers…` | конфиг |
| AGENTS.md | да, по `cwd` (`instructionSources`, E3) | да (E4: `OK WS3-AGENTS`) | да | да |
| Hooks | да, + нотификации `hook/started|completed`, `hooks/list` (E3) | тот же core; доверие через `_codex/hooks/list|trust` (`src/CodexHookTrust.ts:5-6`) — под ACP не испытано | да, `--dangerously-bypass-hook-trust` | да |
| Skills / plugins | `skills/list`, `plugin/*` | `/skills`, скиллы как slash-команды | да | да |
| Subagents | дочерние треды (`parentThreadId`, `agentRole`) | нативные child-сессии по черновому RFD либо tool-call | да | да |
| Compaction | `thread/compact/start` (E3), авто, хуки pre/postCompact | `/compact` | авто | `/compact` |
| Steering посреди хода | `turn/steer` (E2) | `_session/steering` → `injected` (E7), до-стандартное расширение | нет | ввод в pane |
| Interrupt | `turn/interrupt` (E2) | `session/cancel` (баг #641) | SIGINT | Esc |
| Очередь сообщений | `thread/queue/*` (experimental), `codex queue` | нет | нет | очередь в TUI |
| Типизированные ошибки | `codexErrorInfo` + `willRetry` (E5) | только с AIR-расширением; иначе текст (E6, E7) | `turn.failed{message}`, rc=1 | парсинг экрана |
| Лимиты подписки | `account/rateLimits/read|updated` | `_meta.quota` (токены), лимиты — нет | нет | статус-строка |
| Токены на ход | `thread/tokenUsage/updated` | `usage` в ответе, `usage_update` | `turn.completed.usage` | — |

### 2.5. Конфиг и изоляция профилей

Слои (приоритет по возрастанию): packaged defaults → MDM → system → enterprise → user (`$CODEX_HOME/config.toml` + `--profile` =
`$CODEX_HOME/<name>.config.toml`) → project (`.codex/` от `cwd`) → SessionFlags (`-c`, `thread/start.config`) → legacy managed
(`config/src/config_layer_source.rs:35-49`) **[ПРОВЕРЕНО]**. В app-server `config` запроса сливается с CLI-оверрайдами и конфиг
собирается заново на каждый тред с его `cwd` (`app-server/src/config_manager.rs:656-700`).

E3 (`scripts/e3_config_hooks_kill.py`): в одном процессе три треда — T1 без всего; T2 с проектными хуками
(`ws2/.codex/hooks.json`, `config.bypass_hook_trust=true`) — сработали `sessionStart`, `userPromptSubmit`, `stop`, payload хука содержит
`session_id` = thread id и `transcript_path`; T3 с другой моделью (`gpt-5.6-luna`), effort `medium`, `developerInstructions` и своим
`mcp_servers` — ответ `OK DEV-T3 WS3-AGENTS` **[ПРОВЕРЕНО]**. E10: `config.shell_environment_policy.set` даёт разный env командам разных
тредов (`agent=alpha` / `agent=beta`) **[ПРОВЕРЕНО]**.

Что **не** делится на тред: `CODEX_HOME` (auth, `skills/`, `plugins/`, sqlite-состояние), окружение процесса (хуки запускаются с env
сервера — issue openai/codex#48500: «Managed app-server runs hooks with the first client's TMUX_PANE/TMUX»), и в 0.166 — провайдер:
ход на треде с `modelProvider`, отличным от провайдера процесса, отклонён `modelProviderMismatch` (первый прогон E5, `out/e5.jsonl`)
**[ПРОВЕРЕНО]**; README на HEAD описывает уже иное поведение («Resume and input use the thread's saved provider»,
`app-server/README.md:360`) — пример дрейфа между релизами **[ВЫВОД]**.

Хуки codex: события `preModelRequest, postModelRequest, preToolUse, permissionRequest, postToolUse, preCompact, postCompact,
sessionStart, sessionEnd, userPromptSubmit, subagentStart, subagentStop, stop, interrupt`; обработчики `command | mcpTool | prompt |
agent`; источники `system | user | project | sessionFlags | plugin | mdm…`; у каждого `trustStatus` и хэш
(`lab/codex/schema-ts/v2/HookEventName.ts`, `HookMetadata.ts`, сгенерированы из 0.166; JSON-схемы входов/выходов —
`src/codex/codex-rs/hooks/schema/generated/`) **[ПРОВЕРЕНО]**. Gas City ими уже пользуется: `.gc/agents/…/plan-reviewer-codex/.codex/hooks.json`
(SessionStart → `gc prime --hook`, UserPromptSubmit → `gc nudge drain`, PreCompact → `gc handoff`) **[ПРОВЕРЕНО]**.

MCP: 2 ACP-сессии с одинаковым stdio-MCP ⇒ 2 процесса `mini_mcp.py`; `session/close` за 2 с их не убрал (E7) **[ПРОВЕРЕНО]**.
Открытые баги про утечки MCP-процессов: openai/codex#25015, #38754, #11324.

### 2.6. Сбои

E5/E6/E7 (`scripts/e5_failures.py`, `e6_acp_failures.py`, `e7_acp_ext.py`; выводы `out/e5_run.txt`, `e6_run.txt`, `e7_run.txt`):

| Сбой | app-server | ACP v1 без расширений | HTTP-попыток |
|---|---|---|---|
| Usage limit (429, `type: usage_limit_reached`, заголовки `x-codex-*`) | `account/rateLimits/updated{primary{usedPercent:100, windowDurationMins:300, resetsAt}, secondary{…10080…}}` → `error{willRetry:false, codexErrorInfo:"usageLimitExceeded"}` → `turn/completed failed`, тред `systemError` | JSON-RPC error `-32603 Internal error`, `data.codexErrorInfo:"usageLimitExceeded"`, время сброса только текстом | 1 |
| Обычный 429 | `codexErrorInfo:{responseTooManyFailedAttempts:{httpStatusCode:429}}`, `failed` | **`stopReason: end_turn`**, текст ошибки как `agent_message_chunk`; единственный след — `_meta.codex.threadStatus.type="systemError"` | 1 (не ретраится) |
| 503 overloaded | `codexErrorInfo:"serverOverloaded"` через 3.3 с | `end_turn` + текст | 5 |
| 401 | 5 × `error{willRetry:true, responseStreamDisconnected:{401}}` («Reconnecting… n/5»), затем `httpConnectionFailed:{401}` | error `-32000 Authentication required` | 6 |
| Context overflow | `codexErrorInfo:"contextWindowExceeded"` | `end_turn` + текст, `usage_update used == size` | 1 |

- 429 не ретраится по коду: `retry_429: false, retry_5xx: true, retry_transport: true`
  (`model-provider-info/src/lib.rs:502-504`), лимиты попыток 4/5 (`:74-75`) **[ПРОВЕРЕНО]**. Бэкофф на 429 — забота оркестратора.
- Разбор usage-limit: `codex-api/src/api_bridge.rs:217-262` (`resets_at`, `plan_type`, `limit_window_minutes`, варианты `usage_not_included`,
  `insufficient_quota`) **[ПРОВЕРЕНО]**. Структурного `resetsAt` в самой ошибке нет — он в снимке `rateLimits`.
- С декларацией клиента `_meta.jetbrains.air.capabilities:["sessionFailure"]` адаптер кладёт в ответ
  `sessionFailure{category:"limit", severity:"error", actions:[]|["retry"]}` — но всё равно при `stopReason: end_turn` (E7; контракт —
  `src/codex-acp-new/docs/air-extensions.md:895+`). Проблема известна: codex-acp#647, #340 (открыты) **[ПРОВЕРЕНО]**.
- `codex exec --json`: `{"type":"error","message"}` + `{"type":"turn.failed","error":{"message"}}`, код возврата 1; `codexErrorInfo` нет **[ПРОВЕРЕНО]**.
- У владельца из-за прокси `account/rateLimits/read` → `codex account authentication required to read rate limits`, а в реальных ходах
  `rateLimits.primary/secondary = null` (E1): проценты и время сброса ему недоступны при любом драйвере **[ПРОВЕРЕНО]**.
- Истечение ChatGPT-токена: в режиме `chatgpt` codex обновляет сам; в `chatgptAuthTokens` шлёт серверный запрос
  `account/chatgptAuthTokens/refresh` (документация app-server) **[НЕ ПРОВЕРЕНО вживую]**.

**Краш.** E3: ход с реальным `sleep 77`, `kill -9` app-server → stdout EOF за 0.00 с, `sleep` и хост code-mode умерли вместе с ним
(сирот нет). Новый процесс, `thread/resume` → `status: idle`, прерванный ход `interrupted` с одним `userMessage`; на вопрос «завершилась
ли команда» модель ответила `UNKNOWN` **[ПРОВЕРЕНО]**. E4: `kill -9` дочернего app-server под адаптером → `session/prompt` сразу вернул
`error{code:1001, data:{signal:"SIGKILL", restartable:true}}`; следующий `session/prompt` на той же и на другой сессии прошёл —
адаптер сам поднял app-server и возобновил треды (есть `CrashLoopGuard`). `kill -9` самого адаптера → потомков не осталось;
новый адаптер: `session/list` видит сессии, `session/load` реплеит историю, работа продолжается **[ПРОВЕРЕНО]**. Тред без единого
сообщения при рестарте теряется — rollout пишется с первым сообщением (`src/codex-acp-new/src/app-server-recovery/AppServerExit.ts:72-79`).

### 2.7. Логи и идентификаторы

Rollout: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<ts>-<threadId>.jsonl`; путь отдаётся в `thread.path` и в payload хука
(`transcript_path`). ACP `sessionId` == `thread.id` (`src/codex-acp-new/src/CodexAcpClient.ts:881`) == id для `codex resume` /
`codex exec resume`. Проверено в обе стороны: `session/list` адаптера показал треды, созданные сырым app-server и `codex exec`; тред
app-server открыт TUI и exec (E4, E8, E9). Параллельно идёт миграция на «paginated thread history» (`historyMode: "paginated"`,
`thread_history_1.sqlite`, `codex migrate-rollouts`) — формат хранения в движении **[ПРОВЕРЕНО наличие; детали НЕ ПРОВЕРЕНЫ]**.

### 2.8. Подписка и политика

app-server поддерживает `apikey`, `chatgpt` (OAuth, авто-refresh), `chatgptAuthTokens` (experimental); при этом документация:
app-server-аутентификация «has never been permitted for commercial or hosted services», рекомендован переход на Sign in with ChatGPT
(https://learn.chatgpt.com/docs/app-server) **[ПРОВЕРЕНО по документу]**. Вторичный разбор не нашёл запрета на сторонний инструмент,
который запускает официальный CLI под логином пользователя локально (https://dbggr.dev/articles/claude-chatgpt-subscription-coding-tools/)
**[НЕ ПРОВЕРЕНО по первичным условиям]**. Адаптер заявляет ChatGPT-логин как auth-метод (README). Вживую OAuth-подписка не проверялась.

### 2.9. Зрелость

- Стабильные релизы codex: 0.164 (23.10), 0.165 (27.10), 0.166 (29.10), 0.167 (06.11); от 0.137 (08.06) до 0.167 — 30 минорных за
  151 день; между ними десятки пререлизов (`gh api repos/openai/codex/releases`) **[ПРОВЕРЕНО]**.
- Адаптер: 2.0.0 (28.09) → 2.6.0 (08.11), 20 релизов с 20.08; 176 открытых issues; каждый релиз поднимает пин codex.
- openai/codex: 640 открытых issues с «app-server» в заголовке. Значимые для оркестрации: #48500 (хуки с env первого клиента),
  #41849 (зависший app-server держит writer-lock треда), #25015/#38754 (утечки MCP-процессов), #18960 (reconnect-loop websocket),
  #45019 («queued follow-up no longer exists»).
- codex-acp: #647 (ошибки как `end_turn`), #641 (`session/cancel` теряется), #310 (sandbox/approval из `config.toml` игнорируются),
  #336 (effort сбрасывается после хода), #447 (usage только за последний запрос), #229 (нет профилей), #215 (нет проброса конфига).

## 3. Вердикты

**H1 (tmux не нужен, всё решается через ACP) — для codex: первая половина подтверждена, вторая опровергнута.**
tmux не нужен: квота, 429, 5xx, 401, переполнение контекста, краш и resume полностью наблюдаемы и управляемы без терминала — но через
**app-server**, где это типизированные события. Через стандартный ACP — нет: 429/503/overflow неотличимы от успешного хода, время сброса
лимита не передаётся. Вылечить можно только вендорным `_meta`-расширением (и то без `resetsAt`).

**H3 (codex нормально работает через ACP, ACP достаточно как единственный транспорт) — частично.**
Базовый цикл работает хорошо и устойчиво (N сессий, load/resume/fork, steering, авто-рестарт app-server). Недостаточно для оркестратора:
нет типизированных сбоев, нет конфига на сессию (профили, хуки, env), один клиент на процесс, лишний слой с собственными багами и
собственным пином версии codex. ACP для codex — второй сорт относительно родного протокола.

**H4 (один процесс на много сессий экономит память) — подтверждена измерением**, ≈5–10 МБ на тред против ≈155–180 МБ на процесс.
Цена: общий blast radius (падение сервера рвёт все ходы и убивает их команды), один env/`CODEX_HOME`/провайдер на процесс, MCP всё равно
по процессу на тред.

**H9 (общий MCP, стандартные хуки, слоёный конфиг) — для codex как харнесса:** слоёный конфиг и хуки у codex есть и доступны на
тред; общего MCP нет — его должен дать оркестратор (один HTTP-MCP на город). Утверждение «в Gas City хуков нет» неточно: Gas City
уже ставит codex-хуки агентам, но как внутренний механизм, а не как пользовательский стандарт.

**H10 (человек подключается к тому же серверу вместо главной tmux-сессии) — подтверждена для app-server, опровергнута для ACP-адаптера.**
`codex --remote unix://…` даёт полноценный TUI на треды оркестратора с взаимной видимостью. ACP-адаптер — stdio на одного клиента;
«подключиться к ACP» вторым клиентом нельзя, нужен свой мультиплексор.

## 4. Следствия для дизайна и варианты

1. **Драйвер codex — app-server напрямую, ACP — максимум как общий знаменатель.** Интерфейс драйвера в новой системе стоит
   определять по возможностям app-server (тред, ход, steer, interrupt, типизированная ошибка, снимок лимитов), а ACP-драйверы
   для claude/pi подгонять под него, а не наоборот.
2. **Варианты процессной модели:**
   - **A. Один app-server на город** (один `CODEX_HOME`), тред на агента, различия — через `thread/start.config`. Минимум памяти,
     attach человека из коробки; максимум blast radius, одна учётка/провайдер.
   - **B. app-server на «профиль»** (набор скиллов/плагинов/учётка = свой `CODEX_HOME`) или на rig. Компромисс; совпадает с интуицией
     владельца «разные серверы для разных сетов хуков и скиллов», только хуки и MCP на самом деле можно различать и внутри процесса.
   - **C. Процесс на агента** (`codex app-server` stdio или `codex exec --json resume`). Изоляция как сейчас, без tmux, ≈155–180 МБ
     на агента, человек подключается только через тот же сервер.
   - **D. ACP-адаптер на профиль.** Единый код с claude/pi; +100 МБ, обязательна декларация `sessionFailure` и чтение
     `_meta.codex.threadStatus`, иначе сбои невидимы.
3. **Сервер должен принадлежать оркестратору, а не «общему демону codex».** Демон сам обновляется и перезапускается, держит pid-файлы
   и сокет в `/tmp`, наследует env первого запуска; у владельца он уже в сломанном состоянии. Запускать `codex app-server --listen
   unix://<свой путь 0700>` своим супервизором и пинить версию.
4. **Обработка сбоев — в оркестраторе:** 429 не ретраится codex'ом; usage-limit — ждать до `resetsAt` из `account/rateLimits/updated`
   (у владельца через прокси недоступно — нужен проброс заголовков `x-codex-*` либо разбор текста); после краша — `thread/resume` и
   повторная постановка задачи с явной сверкой состояния, потому что результат прерванного tool-call неизвестен.
5. **Один писатель на тред** — оркестратор обязан быть единственным владельцем тредов; «человек открыл `codex resume` рядом»
   получит отказ. Это упрощает модель: attach только через сервер.
6. **Env на агента** задавать через `shell_environment_policy.set` в конфиге треда; хуки идентифицировать по `session_id` из payload,
   а не по env.
7. **Идентичность сессии бесплатна:** thread id = ACP session id = id для CLI = имя rollout-файла; отдельный реестр соответствий не нужен.
8. **Версионная дисциплина:** генерировать схему из пина (`generate-json-schema`), держать контрактные тесты на 5 сценариев сбоев
   (фейковый провайдер из `lab/codex/scripts/fake_provider.py` — готовый прототип), обновлять codex осознанно.

## 5. Открытые вопросы и непроверенное

- Лимиты ChatGPT-подписки (5h/weekly), `account/rateLimits/read`, истечение OAuth — вживую не проверены (нет логина; только
  симуляция по формату из кода).
- Память на длинных тредах (сотни тысяч токенов, много tool-output) и при 10–15 одновременных ходах не измерялась.
- `thread/queue/*` и `codex queue` (durable-очередь сообщений в тред — прямой аналог nudge) не испытаны; API experimental.
- Нативные ACP-субагенты, ACP v2 и поведение `session/cancel` (#641) не испытаны.
- Долговечность: суточная работа app-server с десятками тредов, утечки MCP (#25015) — не проверены.
- Расхождение по `modelProviderMismatch` между 0.166 и HEAD не разобрано по коду.
- Политика OpenAI для локального оркестратора над ChatGPT-подпиской — первичные условия не читались (сайт отдавал обрыв соединения),
  опора на документ app-server и вторичный разбор.
- Старый Rust-адаптер (in-process) не замерялся: он архивирован и привязан к codex 0.137.
- Командное выполнение под sandbox на этой машине не работает (`bubblewrap needs access to create user namespaces`), поэтому
  долгие команды гонялись в `danger-full-access`; поведение sandbox при краше не проверено.
