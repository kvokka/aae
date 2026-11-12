# 14. Claude Code под оркестратором без tmux: ACP-адаптер и нативный headless

Дата: 2026-11-10. Версии: `claude` 2.1.322 (установлен), `@agentclientprotocol/claude-agent-acp` 0.102.0 (несёт `@anthropic-ai/claude-agent-sdk` 0.3.321 и собственный бинарник `claude` 2.1.321), node 26.12, Linux arm64.
Лаборатория: `/home/ubuntu/proj/work/gascity/tmp/gc-alt-research/lab/claude/` (скрипты `exp*.mjs`, сырые логи JSON-RPC в `logs/`).
Пути к коду адаптера ниже даны относительно `/home/ubuntu/proj/work/gascity/tmp/gc-alt-research/src/claude-agent-acp/`.

## 1. Выводы за 60 секунд

- **H4 для claude опровергнута измерением.** Адаптер — это один node-процесс плюс **отдельный процесс `claude` на каждую ACP-сессию**. Дерево на 4 сессии: 1028 МБ PSS через адаптер против 978 МБ у четырёх нативных `claude -p`. Адаптер не экономит память, а добавляет около 100–115 МБ.
- **Адаптер — обёртка над нативным stream-json.** Каждый дочерний процесс запускается как `claude --output-format stream-json --input-format stream-json --permission-prompt-tool=stdio …`. Выбор «ACP или нативный протокол» — это выбор слоя над одним и тем же процессом.
- **Без tmux claude управляется полностью.** Проверено: cancel за 16 мс, очередь промптов, steering в середине хода, фоновые задачи, восстановление после `kill -9` дочернего процесса (`session/resume` за 0,4 с, контекст цел) и после `kill -9` адаптера (`session/load` за 0,5 с).
- **Сбои в нативном потоке структурированы, в ACP v1 — огрублены.** Нативно: `system/api_retry` с номером попытки и задержкой, `rate_limit_event` с `resetsAt`, `result` с `api_error_status`. В ACP v1 обычный клиент получает JSON-RPC `-32603` с текстом и `data.errorKind`; ретраи ему не видны.
- **Обход есть, но нестандартный.** `_meta.claudeCode.emitRawSDKMessages` пробрасывает сырые SDK-сообщения как `_claude/sdkMessage`; лимиты подписки едут в `usage_update._meta["_claude/rateLimit"]`. Это расширения одного адаптера, а не ACP.
- **В спеке ACP квот нет.** RFD session-usage прямо выносит rate limits и quotas за рамки.
- **«Ход закончен, агент ждёт» в ACP v1 не выражается.** После ответа на `session/prompt` фоновая задача будит агента, и обновления приходят вне хода без границы. Черновик ACP v2 это чинит (`state_update: running/idle` + список `background`), адаптер его уже отдаёт, но черновик может меняться.
- **Конфиг Claude Code подхватывается полностью:** `~/.claude/settings.json`, проектный `.claude/settings.json`, hooks, skills, commands, plugins, `CLAUDE.md`, `.mcp.json`.
- **Гипотеза «на разные наборы хуков и скиллов нужны разные серверы» для claude неверна.** В одном процессе адаптера сессии получают разные `settingSources`, хуки, system prompt, MCP и даже `CLAUDE_CONFIG_DIR`. Причина та же: каждая сессия — свой процесс.
- **MCP не шарится.** Каждый stdio-MCP стартует заново в каждой сессии. Общий на город MCP возможен только как HTTP/SSE-сервер.
- **Подписка работает и разрешена для личного использования.** Адаптер без настроек взял OAuth-логин Max. Справка Anthropic (обновления 15.06 и 07.10.2026): Agent SDK и `claude -p` при входе по плану «still draw from your subscription limits». Запрещено встраивать claude.ai-логин в продукт для чужих пользователей.
- **Политический риск остаётся.** Перевод Agent SDK и `claude -p` на отдельный кредит был объявлен на 15.06.2026 и поставлен на паузу с обещанием предупредить заранее.
- **Живого «подключения человека» к сессии адаптер не даёт.** Транспорт только stdio, один клиент на процесс. Второй процесс на тот же session id молча создаёт ветку в том же JSONL.
- **Адаптер очень подвижен:** 43 релиза за 90 дней, 97 открытых issues, главные потребители — Zed и JetBrains AIR. Проблемы автономных долгих сессий чинятся медленно.

## 2. Факты и доказательства

### 2.1. Устройство адаптера и память (H4)

- **[ПРОВЕРЕНО]** Пакет переехал: `@zed-industries/claude-agent-acp` помечен deprecated («renamed to @agentclientprotocol/claude-agent-acp», `npm view`). Актуальная версия 0.102.0 от 2026-11-09, dist-tag `preview` публикуется с каждого пуша в main.
- **[ПРОВЕРЕНО]** Сессия создаётся вызовом `query()` из Agent SDK, который сразу порождает CLI: `src/acp-agent.ts:12487-12497` (комментарий «`query()` spawns the CLI at once»). Путь к бинарнику — `CLAUDE_CODE_EXECUTABLE`, иначе бинарник из пакета SDK: `src/claude-cli.ts:20-30`.
- **[ПРОВЕРЕНО]** Командная строка дочернего процесса (exp4, `/proc/<pid>/cmdline`): `…/claude-agent-sdk-linux-arm64/claude --output-format stream-json --verbose --input-format stream-json --permission-prompt-tool=stdio --disallowedTools=AskUserQuestion --tools=default --setting-sources=user,project,local --permission-mode=auto --allow-dangerously-skip-permissions --include-partial-messages --session-id=<uuid> --replay-user-messages`.
- **[ПРОВЕРЕНО]** Адаптер по умолчанию запускает свой бинарник 2.1.321, а не установленный 2.1.322. На машине живут две версии Claude Code, если не задать `CLAUDE_CODE_EXECUTABLE`.

Замеры (exp1, exp2, exp9; сумма по дереву процессов из `/proc/<pid>/smaps_rollup`; PSS честнее RSS, потому что бинарник 341 МБ делится между процессами):

| Конфигурация | Процессов | RSS, МБ | PSS, МБ |
|---|---|---|---|
| Адаптер, 0 сессий | 1 | 127 | 97 |
| Адаптер, 1 сессия без хода | 4 | 453 | 374 |
| Адаптер, 2 сессии без хода | 7 | 780 | 567 |
| Адаптер, 4 сессии без хода | 13 | 1428 | 945 |
| Адаптер, 1 / 2 / 4 сессии после одного хода | 4 / 7 / 13 | 489 / 836 / 1524 | 414 / 631 / 1028 |
| Нативный stream-json, 1 / 2 / 4 процесса после хода | 3 / 6 / 12 | 359 / 714 / 1444 | 276 / 512 / 978 |
| TUI живого города (pid 381251, 381299, аптайм 40 мин) | по 1 | 207 / 255 | 188 / 233 |
| Интерактивные TUI человека (pid 77108, 387463) | по 1 | 514 / 582 | 492 / 580 |

- **[ПРОВЕРЕНО]** При четырёх сессиях один процесс `claude` весит 164–170 МБ PSS под адаптером и 156–158 МБ нативно; анонимной памяти 140–147 МБ в обоих случаях.
- **[ПРОВЕРЕНО]** Сам адаптер занимает 97–118 МБ PSS и растёт с числом сессий.
- **[ПРОВЕРЕНО]** Под каждой сессией поднимается свой набор MCP-серверов из пользовательских плагинов: `deja mcp` — это node (23–29 МБ) плюс бинарник (30–41 МБ) на сессию.
- **[ПРОВЕРЕНО]** `session/close` завершает дочерний процесс: после закрытия двух из четырёх сессий осталось 7 процессов из 13.
- **[ВЫВОД]** Стоимость сессии claude — 160–250 МБ плюс MCP-серверы при любом драйвере. Экономить память можно только тем, что простаивающие сессии закрываются и поднимаются по требованию (`session/resume` занял 0,4 с), и переводом MCP на общий HTTP-сервер.

### 2.2. Что реализовано в ACP

Реальный ответ `initialize` для v1 (exp1, `logs/exp1-initialize.json`):

```json
{"protocolVersion":1,
 "agentCapabilities":{"_meta":{"claudeCode":{"promptQueueing":true},"authStatus":{}},
  "promptCapabilities":{"image":true,"embeddedContext":true},
  "mcpCapabilities":{"http":true,"sse":true},"auth":{"logout":{}},"providers":{},
  "loadSession":true,
  "sessionCapabilities":{"additionalDirectories":{},"close":{},"delete":{},"fork":{},
   "list":{},"resume":{},"systemPrompt":{"mode":{"override":{}}}}},
 "authMethods":[],"_meta":{"steering":{"supported":true}}}
```

- **[ПРОВЕРЕНО]** Зарегистрированные методы (`src/acp-agent.ts:14925-14956`): `initialize`, `session/new|load|resume|fork|list|delete|close`, `session/set_mode`, `session/set_config_option`, `session/prompt`, `session/cancel`, `authenticate`, `logout`, `providers/list|set|disable`, расширения `_session/steering`, `_session/async_task/stop`, `_session/goal`.
- **[ПРОВЕРЕНО]** `session/new` вернул режимы `default, acceptEdits, plan, auto, bypassPermissions` и config options `mode`, `model` (default / opus / claude-fable-5-1[1m] / sonnet / haiku), `effort` (low…max), `fast`, `ultracode`. Стартовые значения взяты из пользовательских настроек (`auto`, `opus`, `high`).
- **[ПРОВЕРЕНО]** Usage приходит в ответе на промпт (`usage` плюс `_meta.quota` с разбивкой по моделям) и в `usage_update` (`used`, `size`, `cost`, `_meta["_claude/model"]`).
- **[ПРОВЕРЕНО]** MCP от клиента: stdio-сервер из `session/new.mcpServers` подключился (`/mcp` показал `acpmcp: 1 tool`). В issue #883 утверждается обратное для версий до 0.70.
- **[ПРОВЕРЕНО по коду, не экспериментом]** Subagents по умолчанию видны как обычные tool calls; дочерними сессиями они становятся, если клиент объявил `clientCapabilities.subagents` (README, раздел «Subagent sessions»).
- **[ВЫВОД по коду]** Делегирования fs/terminal клиенту нет: инструменты исполняет сам процесс `claude` на хосте адаптера. В `initialize` нет соответствующих capabilities, вызовы `client.readTextFile/writeTextFile` в коде — только проходные обёртки (`src/acp-agent.ts:10258-10264`).

Что обрезано или работает не так:

- **[ПРОВЕРЕНО]** Модель нельзя задать через `_meta.claudeCode.options.model` или `options.settings.model`: сессия всё равно стартует на модели из `settings.json` (exp5 и exp6 ушли на opus). Причина — `src/session-model.ts:555-569`, открытый issue #1056. Работает только `session/set_config_option` после создания или `ANTHROPIC_MODEL` на весь процесс.
- **[ПРОВЕРЕНО]** После `session/resume` модель сбросилась с haiku на opus (exp4). Оркестратор должен переустанавливать её после каждого восстановления.
- **[ПРОВЕРЕНО по issues]** `plan`-обновления на текущих моделях не приходят (#1380). `CLAUDE.md` с `@`-импортами не раскрывается (#1343). `session/load` после компакции воспроизводит историю только с последней компакции (#1297).
- **[ПРОВЕРЕНО по коду]** Типизированные ошибки и состояние фоновых задач доступны только клиенту, объявившему расширения JetBrains AIR (`docs/air-extensions.md:1118-1130`; issues #1285, #1530).

### 2.3. Конфиг Claude Code под адаптером (exp6)

Проект `work/proj` содержал `CLAUDE.md`, `.claude/settings.json` с хуками, skill, command, agent и `.mcp.json`.

| Что | Сессия A (по умолчанию) | Сессия B (тот же адаптер, тот же cwd, свои опции) |
|---|---|---|
| Команд и скиллов в `available_commands_update` | 60, включая `labskill`, `labcmd`, пользовательский `grilling`, плагин `deja-vu:*` | 51, ничего из перечисленного |
| MCP (`/mcp`) | 4 подключено: плагин, `projmcp` из `.mcp.json`, `acpmcp` от клиента, коннектор claude.ai | «No MCP servers are configured» |
| Хуки проекта | сработали `SessionStart`, `UserPromptSubmit`, `Stop` | 0 срабатываний |
| Хук из `options.settings.hooks` | не задавался | сработал |
| `CLAUDE.md` | кодовое слово названо без инструментов | прочитано инструментом Bash, то есть в контекст не загружалось |
| `_meta.systemPrompt.append` | не задавался | применился (ответ кончается на `ZEBRA9`) |

- **[ПРОВЕРЕНО]** Опции сессии B: `_meta.claudeCode.options = {settingSources: [], strictMcpConfig: true, settings: {hooks: …}}`.
- **[ПРОВЕРЕНО]** Сессия C с `options.env.CLAUDE_CONFIG_DIR=<lab>/cfgC` создалась, а на первом промпте за 100 мс получила `-32000 Authentication required`. Отдельный config dir на сессию работает, но учётные данные нужно доставлять туда самому (`CLAUDE_CODE_OAUTH_TOKEN`).
- **[ПРОВЕРЕНО]** Побочный эффект: `SettingsManager` адаптера читает настройки из config dir самого процесса, поэтому сессия C показала `mode=auto, model=opus` из чужого каталога.
- **[ПРОВЕРЕНО по коду]** Адаптер ставит `settingSources: ["user","project","local"]` (`src/acp-agent.ts:12273`), а затем накладывает опции клиента (`:12293`). Почти весь `Options` SDK проходит через `_meta`: `agents`, `plugins`, `skills`, `allowedTools`, `sandbox`, `maxTurns`, `maxBudgetUsd`, `fallbackModel` и другие (`:1746-1820`, `:1886-1915`).
- **[ПРОВЕРЕНО]** stdio-MCP стартуют на каждую сессию: в `logs/mcp-starts.log` родитель обоих серверов — pid дочернего `claude`.

### 2.4. Сбои: как выглядят (exp4, exp7, exp8)

Ошибки API сняты на локальном фальшивом API (`fake-api.mjs`, отдельный `CLAUDE_CONFIG_DIR`, фиктивный ключ); квота не тратилась.

| Сбой | Нативный stream-json | ACP v1, обычный клиент |
|---|---|---|
| 429 | `system/api_retry {attempt, max_retries, retry_delay_ms, error_status:429, error:"rate_limit"}` на каждую попытку, затем `assistant {error:"rate_limit", model:"<synthetic>"}` и `result {subtype:"success", is_error:true, api_error_status:429, terminal_reason:"api_error"}` | ретраи не видны; `agent_message_chunk` с текстом ошибки; ошибка `-32603 "Internal error: API Error: Request rejected (429)…"`, `data:{errorKind:"rate_limit"}` |
| 529 | то же с `error:"overloaded"`; финал `assistant.error:"server_error"`, `api_error_status:529` | `-32603`, `data.errorKind:"server_error"` |
| 529, потом успех | два `api_retry`, затем обычный ответ | обычный `end_turn`, следов нет |
| 401 | `api_retry error:"authentication_failed"`, затем `result is_error`, 401 | `-32000 "Authentication required"` |
| Нет логина | не снималось | `-32000` за 100 мс |
| Переполнение контекста (400) | `assistant.error:"invalid_request"`, `result {is_error:true, api_error_status:400, terminal_reason:"prompt_too_long"}` | `-32603`, `errorKind:"invalid_request"`; затем tool call `compact` с «Compaction failed» |
| `kill -9` дочернего `claude` в середине хода | процесс исчез | через 9 мс `-32603 "The Claude Agent process exited unexpectedly. Please start a new session."` без `data`; адаптер жив; следующий промпт — `-32002 Resource not found` |
| `kill -9` адаптера | не применимо | клиент видит EOF |

Детали:

- **[ПРОВЕРЕНО]** Ретраи харнесс делает сам: по умолчанию `max_retries: 10` с экспоненциальной задержкой (замерено 0,5 → 1 → 2,3 → 4,9 → 9,7 → 19,9 → 32,8 с). Настраивается `CLAUDE_CODE_MAX_RETRIES`. С неверным API-ключом 401 тоже ретраится: за 45 с прошло 7 попыток из 10.
- **[ПРОВЕРЕНО]** Ловушка: ошибочный ход приходит как `result` с `subtype:"success"` и `is_error:true`. Смотреть нужно на `is_error` и `api_error_status`.
- **[ПРОВЕРЕНО]** После ошибки API процесс и сессия живы, следующий промпт идёт в ту же сессию.
- **[ПРОВЕРЕНО]** Исчерпание лимита подписки, реальные записи в транскриптах этой машины: `{"type":"assistant","error":"rate_limit","isApiErrorMessage":true,"apiErrorStatus":429,"message":{"model":"<synthetic>","content":[{"text":"You've hit your session limit · resets 6:20am (Asia/Bangkok)"}]}}` (сессия `319983e9…`, 2026-09-07). Время сброса в транскрипте — только текстом.
- **[ПРОВЕРЕНО]** Структурированное время сброса даёт `rate_limit_event`, снят вживую: `{"status":"allowed","resetsAt":1794286800,"rateLimitType":"seven_day","overageStatus":"rejected","unifiedWindows":{"seven_day":{"utilization":0.41,…}}}`. Типы: `status: allowed|allowed_warning|rejected`, `rateLimitType: five_hour|seven_day|…` (`sdk.d.ts:6000-6026`).
- **[ПРОВЕРЕНО]** Адаптер кладёт это событие в `usage_update._meta["_claude/rateLimit"]` (`src/acp-agent.ts:8739-8756`; видно в `logs/exp5.ndjson`).
- **[ВЫВОД по коду, вживую не снято]** При исчерпании квоты ACP-клиент получит `-32603` с текстом «You've hit your…» и `errorKind:"rate_limit"`. Отличить квоту от обычного 429 можно только по префиксу текста (`USAGE_LIMIT_ERROR_PREFIXES`, `sdk.d.ts:10419`) или по предшествующему `_claude/rateLimit.status == "rejected"`.
- **[ПРОВЕРЕНО]** Восстановление после смерти дочернего процесса (exp4): `session/resume` за 398 мс, новый процесс `claude`, кодовое слово из первого хода названо верно.
- **[ПРОВЕРЕНО]** Смерть адаптера (exp8c): простаивающий `claude` завершается меньше чем за секунду. Процесс в середине хода **доделывает ход без наблюдателя** и выходит (в тесте через 17 с). Новый адаптер делает `session/load` за 539 мс с replay истории.
- **[ПРОВЕРЕНО]** Компакция: клиенту без capability `session.compaction` она показывается синтетическим tool call (`src/context-compaction.ts:13-21`). Адаптер умеет компактить простаивающие сессии до истечения кэша (`src/idle-compaction.ts`).

### 2.5. Управление ходом (exp5, exp10)

- **[ПРОВЕРЕНО]** `session/cancel`: ответ `stopReason:"cancelled"` через 16 мс, следующий промпт отработал.
- **[ПРОВЕРЕНО]** Второй `session/prompt` во время первого ставится в очередь и выполняется следом; оба запроса получили `end_turn`.
- **[ПРОВЕРЕНО]** `_session/steering` в середине хода вернул `{"outcome":"injected"}`, агент сменил ответ. В v2 steering не обслуживается (issue #1498).
- **[ПРОВЕРЕНО]** Фоновая команда: ход закрылся `end_turn`, через 13 с пришли `agent_message_chunk` **вне какого-либо промпта**. Единственный маркер — `usage_update._meta["_claude/origin"] = {"kind":"task-notification"}`. Об этом открытые issues #864 (с июля, без ответа мейнтейнеров) и #1285.
- **[ПРОВЕРЕНО]** ACP v2 (`protocolVersion: 2`): `session/prompt` сразу возвращает `{messageId}`, затем идут `state_update {state:"running", background:[]}` и `state_update {state:"idle", stopReason:"end_turn", usage}`.
- **[ПРОВЕРЕНО по типам]** Нативно то же даёт `system/session_state_changed {state: idle|running|requires_action}`, в SDK названный «authoritative turn-over signal» (`sdk.d.ts:6268-6274`).
- **[ПРОВЕРЕНО]** `claude agents --json` отдаёт реестр живых сессий со статусом. В нём видны и TUI-сессии города: `{"id":"5daa6b75…","kind":"interactive","status":"idle","pid":381251}`.

### 2.6. Логи и переносимость сессий (exp8)

- **[ПРОВЕРЕНО]** Транскрипт лежит в `~/.claude/projects/<cwd, где не-буквоцифры заменены на ->/<sessionId>.jsonl`. ACP `sessionId` равен session id Claude.
- **[ПРОВЕРЕНО]** Сессия, созданная через ACP, продолжается нативным `claude -p --resume <id>`. Сессия, созданная нативно, загружается через `session/load` с replay и видна в `session/list`.
- **[ПРОВЕРЕНО]** Параллельный доступ рассинхронизирует. Пока ACP-сессия жила, `claude -p --resume <id>` дописал ход с вторым кодовым словом. Живая сессия его не увидела, а свежий resume назвал только первое слово. В JSONL это две ветки от одного `parentUuid`; resume идёт по самой свежей.
- **[НЕ ПРОВЕРЕНО]** Открытие ACP-сессии в TUI: `claude --resume <id>` в pty остановился на диалоге доверия каталогу. Принимать его я не стал, чтобы не менять `~/.claude.json`. Механизм resume общий с `-p`.

### 2.7. Политика и подписка

- **[ПРОВЕРЕНО]** Технически адаптер без настроек использовал OAuth-логин Max: `_auth/status_update {"kind":"account","label":"Claude Max"}`, ходы прошли, в нативном `init` стоит `apiKeySource:"none"`.
- **[ПРОВЕРЕНО]** Справка Anthropic, https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan. Баннер 15.06.2026: «We've paused the previously-announced changes to Claude Agent SDK usage. For now, nothing has changed: Claude Agent SDK, `claude -p`, and third-party app usage still draw from your subscription limits». Баннер 07.10.2026: планы Max, Ultra и Team получили месячные API-кредиты для запусков с API-ключом; «when you're signed in with your Claude plan, the Claude Agent SDK and `claude -p` still draw from your subscription limits».
- **[ПРОВЕРЕНО]** Юридическая страница, https://code.claude.com/docs/en/legal-and-compliance:
  - «Advertised usage limits for Pro, Max, and Ultra plans assume ordinary, individual usage of Claude Code and the Agent SDK».
  - «Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users».
  - «Nor does it prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription, including where a platform hosts Claude Code».
  - «Anthropic reserves the right to take measures to enforce these restrictions and may do so without prior notice».
- **[ПРОВЕРЕНО]** Обзор Agent SDK, https://code.claude.com/docs/en/agent-sdk/overview: «Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK».
- **[ПРОВЕРЕНО]** `claude setup-token` выпускает годовой OAuth-токен подписки для «CI pipelines and scripts» (https://code.claude.com/docs/en/authentication). `--bare` OAuth не читает.
- **[ПРОВЕРЕНО]** Флаг адаптера `--hide-claude-auth` запрещает биллинг подписки (`src/hide-claude-auth.ts:1-40`). Его включают продукты-дистрибьюторы: в issue #517 JetBrains пишет, что не вправе распространять вариант с логином по подписке. Без флага адаптер подписку использует.
- **[ПРОВЕРЕНО]** Блог Zed, https://zed.dev/blog/anthropic-subscription-changes (14.05.2026, обновление 16.06): перенос ACP-трафика на отдельный «Agent SDK credit» ($200 для Max 20x) не вступил в силу; Anthropic «revising the plan and will share advance notice».
- **[ВЫВОД]** Личный оркестратор владельца на своей машине, с немодифицированным бинарником и своим логином, сегодня в рамках правил и расходует лимиты подписки. Два риска: формулировка «ordinary, individual usage» для круглосуточного флота агентов и возможное возвращение отдельного кредита. Риск одинаков для ACP, SDK и `claude -p` и не касается только интерактивного TUI.

### 2.8. Зрелость адаптера

- **[ПРОВЕРЕНО]** Репозиторий `agentclientprotocol/claude-agent-acp`: 2744 звезды, 97 открытых и 328 закрытых issues, 49 открытых PR, 865 смёрженных. Релизов: 15 за 30 дней, 43 за 90 дней. Около 300 коммитов с 26.08 по 09.11.2026.
- **[ПРОВЕРЕНО]** Авторы последних 300 коммитов: benbrandt — 142; четверо с адресами `@jetbrains.com` — 51; боты — 59. npm-мейнтейнеры: benbrandt, aguzubiaga, cirwin. Anthropic среди мейнтейнеров не видна.
- **[ПРОВЕРЕНО]** `package.json` адаптера называет автором Zed Industries. Принадлежность отдельных мейнтейнеров к Zed я по профилям не сверял.

Открытые баги, значимые для оркестрации:

| № | Суть |
|---|---|
| #864 | нет сигнала active/idle на уровне сессии; с июля без ответа |
| #1285 | обычный клиент не знает, что фоновые задачи ещё идут |
| #1445 | `session/prompt`, поглощённый циклом task-notification, не завершается никогда |
| #1284 | ход, удержанный ради фоновых субагентов, не отвечает при потере нотификации |
| #1269 | если клиент не читает stdout вне хода, сессия встаёт (600 с на tool call) |
| #1193 | регрессия «Query closed before response received» убивает все сессии |
| #1011 | при повторных `session/load` копились дочерние процессы (причина найдена, fix в #1220) |
| #1056 | `options.model` перекрывается `settings.model` (подтверждено экспериментом) |
| #1297 | `session/load` теряет историю до последней компакции |
| #625 | лимиты подписки клиенту не показываются; ответ мейнтейнера: API экспериментальный, «we will wait» |

## 3. Вердикты по гипотезам

**H1 (tmux не нужен, сбои решаются через ACP) — частично подтверждена.**
Первая половина верна: tmux как транспорт для claude не нужен, все сценарии сбоев воспроизведены и восстановлены без него. Вторая половина неточна: «через ACP» работает только с нестандартными `_meta`-расширениями адаптера (`emitRawSDKMessages`, `_claude/rateLimit`) или на черновике v2. Стандартный ACP v1 не несёт ни квот (`src/agent-client-protocol/docs/rfds/session-usage.mdx:207-209`), ни ретраев, ни границы «агент простаивает». Полный набор сигналов даёт нативный stream-json.

**H3 (claude нормально работает через ACP, одного транспорта достаточно) — частично.**
Базовый цикл надёжен во всех моих прогонах. «Нормально» для автономных сессий — с оговорками: проблемы фоновых задач открыты месяцами, модель задаётся только через `set_config_option`, выпуск идёт каждые два дня. Как единственный транспорт годится при условии пина версии и чтения сырых SDK-сообщений.

**H4 (один сервер на много сессий экономит память) — опровергнута для claude.**
Процесс на сессию остаётся; адаптер добавляет 100–115 МБ. Измерения в 2.1.

**H9 (общий MCP, хуки, слоёный конфиг) — уточнение со стороны claude.**
Про отсутствие этого в Gas City я не проверял. Харнесс же даёт всё нужное: хуки, system prompt, MCP и набор источников настроек задаются на сессию через `_meta`, файлы для этого не нужны. Общий MCP достижим только через HTTP/SSE: адаптер объявляет `mcpCapabilities.http/sse`, вживую с HTTP-сервером не проверено.

**H10 (пользователь подключается к ACP как к control pane) — в прямом виде не подтверждается.**
Адаптер говорит только по stdio с одним клиентом (`src/serve.ts:28-46`). Параллельное открытие той же сессии вторым процессом даёт молчаливую ветку. Нужен либо мультиплексор в оркестраторе, либо передача сессии: оркестратор закрывает свою, человек открывает `claude --resume <id>`, потом обратно.

## 4. Следствия для дизайна и варианты

Сравнение драйверов для claude:

| Критерий | ACP-адаптер | Нативный `claude -p` stream-json | Agent SDK напрямую | tmux + TUI |
|---|---|---|---|---|
| Процесс на сессию | да, плюс адаптер | да | да | да |
| Память на 4 сессии (PSS) | 1028 МБ | 978 МБ | примерно как нативный плюс хост SDK [ВЫВОД] | около 190–230 МБ на сессию |
| Ошибки и квоты | текст + `errorKind`; полно только через `_meta` или AIR | полные: `api_retry`, `rate_limit_event`, `result.*` | те же, типизированные | разбор экрана и транскрипта |
| Сигнал «простаивает» | v1 нет; v2 `state_update` | `result`, `session_state_changed` | то же | эвристики, либо `claude agents --json` |
| Cancel, очередь, steering | есть, проверено | очередь проверена; interrupt через control-протокол SDK | `interrupt()`, streaming input | send-keys |
| Resume и replay | `load`/`resume`/`fork`/`list` готовы | `--resume`, `--session-id`, `--fork-session`; replay писать самому | `resume`, `forkSession`, `getSessionMessages` | `--resume` |
| Хуки, скиллы, конфиг | всё, на сессию через `_meta` | всё, флагами | всё, плюс хуки-колбэки в процессе | всё |
| Стабильность интерфейса | релиз каждые 2 дня; v2 — черновик | флаги документированы; control-протокол — внутренний для SDK | официальный API | вывод TUI не контракт |
| Подключение человека | один клиент, нужен прокси | нет | нет | родное |
| Единый транспорт с codex и pi | да | нет | нет | да |

Варианты:

- **A. ACP-адаптер, v1 плюс сырые SDK-сообщения.** Быстрый старт: resume, replay, очередь, steering и разрешения уже сделаны. Обязательны пин версии, `CLAUDE_CODE_EXECUTABLE` на один бинарник, клиент, который всегда читает stdout (#1269), и собственный таймаут на «вечный» промпт (#1445, #1284).
- **B. Нативный stream-json из оркестратора.** Минимум слоёв и полный набор сигналов, минус 100 МБ. Цена: свой код для разрешений, прерывания и replay, и зависимость от недокументированного control-протокола.
- **C. Сайдкар на Agent SDK.** Официальный API, но мост до оркестратора придётся изобретать, и получится второй адаптер.
- **D. `claude --bg` + `claude attach` + `claude agents --json`.** Родной фоновый режим Claude Code с подключением человека. Не испытан, см. раздел 5.

Что выбрал бы я: **A как драйвер по умолчанию**, с интерфейсом драйвера, спроектированным под набор сигналов B (состояние хода, ретрай, rate-limit info, вид сбоя). Причины: владельцу нужен один транспорт на три харнесса, а claude-специфику адаптер уже отдаёт через `_meta`. Решение обратимо: под A и B один и тот же процесс и одни и те же транскрипты, сессии переносятся в обе стороны. Если фоновые задачи под адаптером окажутся ненадёжны на практике, я бы переключил claude на B.

Независимо от варианта:

- Не обещать экономию памяти от ACP. Экономит закрытие простаивающих сессий и общий HTTP-MCP.
- Модель и effort выставлять после каждого `new`, `resume` и `load`.
- Квоту определять по `rate_limit_info.status` и `resetsAt`, а не по тексту.
- Один писатель на session id. Подключение человека — через прокси или передачу сессии.
- Один адаптер на город — единая точка отказа; при его смерти идущие ходы доработают без наблюдателя. Адаптер на rig или пул стоит 100 МБ и изолирует сбои.

## 5. Открытые вопросы и непроверенное

- **Квота подписки вживую в ACP не снималась** (не выжигал). Форма ошибки выведена из кода и реальных транскриптов.
- **Долгие ходы и субагенты** под адаптером не гонялись. Баги #1445, #1284, #1269 я не воспроизводил.
- **`claude --bg` / `attach` / `agents`** не испытаны; видел только `--help` и вывод `claude agents --json`.
- **HTTP-MCP как общий сервис города** не проверен экспериментом.
- **Открытие сессии в TUI** упёрлось в диалог доверия.
- **Истечение OAuth в середине долгой сессии** не воспроизводилось; `loginExpiresAt` на машине — 2026-11-14.
- **ACP v2** проверен на один ход. На v2 `session/set_config_option` с прежними параметрами вернул `-32602`.
- **Память адаптера на часах работы** не измерялась.
- **Побочные эффекты экспериментов.** Конфиг в `~/.claude` не менялся. Появилось 10 каталогов транскриптов `~/.claude/projects/*gc-alt-research-lab-claude*`. В лабораторных сессиях срабатывали пользовательские хуки и плагин deja-vu. Несколько ходов ушли на opus вместо haiku из-за #1056; exp5 показал накопленные $0,157 по API-расценкам, по остальным стоимость не снималась, суммарно порядка десятков центов.
