# 01. Драйвер claude: `claude -p` stream-json — шпаргалка

Стенд всех фактов: claude **2.1.322**, Linux aarch64 (OrbStack), опыты 2026-11-10 и 2026-11-11. Справка CLI сверена 2026-11-12.

**Метки:** **[П]** проверено опытом (указан лог), **[К]** по коду/типам/документации вендора, **[НП]** не проверено.
«[П через ACP]» = снято через `claude-agent-acp` (он запускал свой claude 2.1.321); процесс тот же, но для прямого `claude -p` не подтверждено.

**Источники:** `R14:30` = `reports/14-harness-claude-acp.md:30`; так же `R34` = `34-failure-recovery-spec.md`, `R36` = `36-experiments-…md`,
`R42` = `42-coldstart-floor.md`, `R44` = `44-claude-code-native.md`, `R46a` = `46a-observability-claude.md`. Пути без `R` — от `lab/`.
`OBS` = `lab/observability/claude`. `help` = `claude --help` 2.1.322. `SDKDOC` = `lab/claude/logs/doc-agent-sdk_typescript.md`,
`ENVDOC` = `lab/cc-native/web/env-vars.md` (снимки документации вендора от 2026-11-10/11).

## 1. Командная строка

Базовая форма (долгоживущий процесс на сессию) **[П]** `claude/native.mjs:6-11`, `OBS/scripts/run_stdio.sh:40-43`:

```
claude -p --input-format stream-json --output-format stream-json --verbose \
  --session-id <uuid> --replay-user-messages --permission-prompt-tool=stdio \
  --setting-sources "" --settings <file|json> --strict-mcp-config --mcp-config <file|json> \
  --plugin-dir <dir> --append-system-prompt <text> --model <m> --effort <lvl> --permission-mode <mode>
```

| Задача | Флаг | Метка, источник |
|---|---|---|
| потоковый ввод/вывод | `-p --input-format stream-json --output-format stream-json --verbose` (`--verbose` обязателен для stream-json вывода) | [П] `claude/native.mjs:9`; ENVDOC:284 |
| id новой сессии | `--session-id <uuid>` (валидный UUID); тот же id приходит в `session_id` каждого сообщения | [П] `claude/exp3-native.mjs:11`, `claude/logs/exp3.ndjson:4` |
| resume | `--resume <id>` (+ `--fork-session` для нового id). Одноразовый `-p "<prompt>" --resume` — [П]; с `--input-format stream-json` — [П через ACP], напрямую [НП] | `coldstart/scripts/native.mjs:80`, `claude/exp8-cross.mjs:44`; R34:255 |
| подтверждение приёма | `--replay-user-messages`: эхо `{"type":"user",…,"isReplay":true,"uuid":…}` на stdout. Флаг есть [К] help; адаптер его ставит [П через ACP] R14:30; **форма эха в лаборатории не снята** | SDKDOC:1792-1808 |
| источники настроек (наследование хоста) | `--setting-sources user,project,local`; `""` = ничего не читать. Без флага читается всё | [П] R42:21-23, `coldstart/scripts/native.mjs:57` |
| настройки + хуки сессии | `--settings <файл или JSON>`; формат `{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"…"}]}]}}` | [П] `coldstart/role/settings.json`, R42:41 |
| MCP | `--mcp-config <файл или JSON>` + `--strict-mcp-config` (игнорировать все прочие MCP). HTTP: `{"mcpServers":{"n":{"type":"http","url":…,"headers":{…}}}}`; stdio: `{"command":…,"args":[…]}` | [П] `coldstart/role/mcp.json`, R42:37 |
| плагины/skills | `--plugin-dir <dir>` (повторяемый; каталог плагинов грузит каждого потомка) | [П] R42:37 (5 skills плагина видны в init) |
| системный промпт | `--append-system-prompt <text>` [П, 9,5 КБ] R42:8; `--system-prompt <text>` [К] help | |
| модель, effort | `--model <alias\|id>`, `--effort low\|medium\|high\|xhigh\|max` | [П] `OBS/scripts/run.sh:41`; help |
| режим разрешений | `--permission-mode`: help перечисляет `acceptEdits, auto, bypassPermissions, manual, dontAsk, plan`; в опытах работали `default` (init: `permissionMode:"default"`) и `dontAsk` | [П] `OBS/runs/f4/stream.jsonl:7`, R44:136 |
| обработчик запросов разрешений | `--permission-prompt-tool=stdio` → запросы идут control-сообщениями в stdout (§3.6). Флага нет в списке опций help (упомянут только в описании `--permission-prompts`) | [П] `OBS/scripts/run_stdio.sh:42`, R46a:40 |
| некому отвечать | `--permission-prompts none` — всё, что спросило бы, отклоняется | [К] help |
| субагенты-роли | `--agents <json\|file>` (с `-p` — путь к файлу); у роли свои `hooks`, `mcpServers`, `model`, `disallowedTools` | [П] R44:40-43 |
| бюджет | `--max-budget-usd <n>` [К] help. `--max-turns` в help 2.1.322 **нет** | help |
| события хуков в потоке | `--include-hook-events` → `system/hook_started`, `system/hook_response` | [П] `OBS/runs/f4/stream.jsonl:2-3` |
| дельты текста | `--include-partial-messages` → `stream_event` с дельтами во время генерации | [П] R46a:235 |

Ловушки запуска:
- `--setting-sources ""` не даёт «голого» старта: всё равно грузятся 3 встроенных плагина `cc-plugin-*` (19 skills) **[П]** R42:39.
- `--setting-sources ""` отключает и проектные `.claude/workflows`, `.claude/agents` **[П]** R44:156.
- `--strict-mcp-config` не фильтрует inline-MCP из `--agents`, но молча режет MCP из frontmatter проектных агентов **[П]** R44:195-197.
- Хуки из `--settings` срабатывают и внутри каждого субагента (во входе `agent_type`, `agent_id`); отключить на роль нельзя **[П]** R44:46.
- Хуки агента из `.claude/agents/` недоверенного каталога молча пропускаются, агент работает **[П]** R44:48.
- В `-p` диалог доверия каталогу пропускается; невалидный файл настроек молча игнорируется **[К]** help (`-p`).
- `--bare` не читает OAuth/keychain — только `ANTHROPIC_API_KEY` **[К]** help; R42:114.
- При resume модель передавать явно: сброс на модель из настроек наблюдался **[П через ACP]** R14:77; для `claude -p` **[НП]**.

## 2. Ввод (stdin, JSONL)

Сообщение пользователя **[П]** — оба варианта `content` работали:

```json
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Reply with exactly: OK"}]}}
{"type":"user","message":{"role":"user","content":"Reply…"},"parent_tool_use_id":null,"session_id":""}
```
`claude/logs/exp3.ndjson:3`; `OBS/runs/f4/stream.jsonl:1`.

- **До первого сообщения пользователя процесс ничего не пишет в stdout** (кроме событий хука `SessionStart` при `--include-hook-events`/пользовательских хуках). `system/init` приходит **после** отправки сообщения и **повторяется на каждый ход**. Ждать `init` перед отправкой промпта нельзя — будет дедлок **[П]** `claude/logs/exp3.ndjson:1-4,8-9`; `cc-native/logs/e4idle-*.ndjson` (0 байт у трёх простаивающих процессов).
- Второй промпт во время хода ставится в очередь (не steer) **[П через ACP]** R14:131; в `result` есть `queued_turn_count` **[П]** `claude/logs/exp7-native-429.ndjson:6`.
- Ответ на запрос разрешения — `control_response` (§3.6). Прерывание хода — control-запрос `interrupt` **[К]** SDKDOC:891-916 (поле `cancel_queued`, квитанция `still_queued`); в лаборатории нативно **не снималось [НП]** (отмена за 16 мс — это `session/cancel` через ACP-адаптер, замер 2026-11-10, claude 2.1.321, R14:130). <!-- MEASURE id=claude-acp-cancel-latency date=2026-11-10 ver=claude-2.1.321 env=linux-arm64-orbstack src=reports/14 note="через ACP-адаптер, n=1; нативный interrupt не мерили" -->
- Ключа идемпотентности протокол не даёт: маркер ключа вставлять в текст промпта и искать в транскрипте **[К]** R34:300-304.

## 3. Вывод (stdout, JSONL): что разбирать

Все сообщения несут `session_id`, большинство `uuid`. Порядок ключей не гарантирован (`"type":"result"` стоит в середине строки) — разбирать JSON целиком.

Порядок одного хода **[П]** `OBS/runs/f4/stream.jsonl:4-26`:
`session_state_changed{running}` → `system/init` → [`system/status{requesting}`] → `assistant` (tool_use) → [`session_state_changed{requires_action}` + `control_request`] → `user` (tool_result) → `assistant` (text) → `result` → `session_state_changed{idle}`.

### 3.1 `system/init` (на каждый ход)
```json
{"type":"system","subtype":"init","cwd":"…","session_id":"17fb09de-…","tools":[…],"mcp_servers":[{"name":"perm","status":"connected","source":"dynamic"}],
 "model":"claude-haiku-5-5","permissionMode":"default","apiKeySource":"ANTHROPIC_API_KEY","claude_code_version":"2.1.322",
 "agents":[…],"skills":[…],"plugins":[…],"capabilities":["interrupt_receipt_v1","interrupt_cancel_queued_v1",…],"messaging_socket_path":"/tmp/cc-socks/634166.sock"}
```
**[П]** `OBS/runs/f4/stream.jsonl:7`. `apiKeySource:"none"` = логин подпиской (OAuth) **[П]** `OBS/runs/r2/stream.jsonl:6`, R14:147. По `init` проверять: версию, модель, `mcp_servers[].status` (`connected`/`pending`/…), набор плагинов и skills.

### 3.2 `result` — конец хода
```json
{"type":"result","subtype":"success","is_error":false,"api_error_status":null,"terminal_reason":"completed","stop_reason":"end_turn","result":"FAKE-OK",
 "num_turns":1,"duration_ms":1835,"duration_api_ms":1753,"ttft_ms":1824,"total_cost_usd":2.5e-06,"usage":{…},"modelUsage":{…},
 "permission_denials":[],"subagent_stats":{"spawned":0,"started_in_background":0,…},"queued_turn_count":0,"result_index":0,"session_id":"…","uuid":"…"}
```
**[П]** `claude/logs/exp7-native-529x2ok.ndjson:6`.

- **Ловушка:** при ошибке API `subtype` остаётся `"success"`, а `is_error:true`. Смотреть `is_error`, `api_error_status`, `terminal_reason` (`completed` | `api_error` | `prompt_too_long`) **[П]** `claude/logs/exp7-native-429.ndjson:6`, `…-400ctx.ndjson:4`; R34:76.
- Ход, который харнесс начал сам (разбужен фоновой задачей), даёт ещё один `init` и `result` с `"origin":{"kind":"task-notification","producer":"session-task","runId":"…"}` **[П]** `OBS/runs/r1/stream.jsonl:218,241`. Число `result` ≠ числу промптов.
- `result.subtype:"error_max_turns"|"error_max_budget_usd"|"error_during_execution"` **[К]** R34:64,68.
- После ошибочного `result` процесс жив, следующий промпт идёт в ту же сессию **[П]** `claude/logs/exp7-native-429.ndjson:7-12`.

### 3.3 `system/api_retry` — повтор запроса харнессом
```json
{"type":"system","subtype":"api_retry","attempt":1,"max_retries":10,"retry_delay_ms":567,"error_status":401,"error":"authentication_failed","session_id":"…","uuid":"…"}
```
**[П]** `claude/logs/exp7-native-401.ndjson:3`. Значения `error`: `rate_limit` (429), `overloaded` (529), `authentication_failed` (401) **[П]** там же и `…-429.ndjson:3`, `…-529.ndjson:3`. Приходит сразу на каждую попытку. После срабатывания байтового watchdog на зависшем стриме `api_retry` **не шлётся** — сразу non-stream fallback **[П]** R46a:204.

### 3.4 `rate_limit_event`
```json
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1794286800,"rateLimitType":"seven_day","overageStatus":"rejected",
 "overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"seven_day":{"utilization":0.41,"resetsAt":1794286800,"status":"allowed"}}},"uuid":"…","session_id":"…"}
```
**[П]** `claude/logs/exp3.ndjson:6` (подписка, после ответа модели). `status: allowed|allowed_warning|rejected`; `resetsAt` — unix-секунды; `errorCode:"credits_required"` **[К]** SDKDOC:5909-5927. Форма `rejected` вживую не снята **[НП]** R34:58. **При API-ключе событие не приходит вовсе [П]** R46a:206, `claude/logs/exp7-native-quota.ndjson`.

### 3.5 `system/session_state_changed` — только с `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1`
```json
{"type":"system","subtype":"session_state_changed","state":"requires_action","uuid":"…","session_id":"…"}
```
**[П]** `OBS/runs/f4/stream.jsonl:4,11,16,26`. `state: running|idle|requires_action`. Одно состояние может прийти повторно; `idle` и `result` могут прийти в любом порядке **[К]** SDKDOC:5873-5882 (в опытах `idle` шёл сразу после `result` **[П]** f4:25-26, r2:71-72).

### 3.6 Запрос разрешения и ответ (`--permission-prompt-tool=stdio`)
```json
{"type":"control_request","request_id":"794d57b8-…","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash",
 "input":{"command":"touch stdio-perm-probe.txt","description":"touch probe file"},"description":"touch probe file",
 "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch …"}],"behavior":"allow","destination":"localSettings"},…],
 "blocked_path":"/…/stdio-perm-probe.txt","tool_use_id":"toolu_fake_touch_1"}}
```
Ответ в stdin:
```json
{"type":"control_response","response":{"subtype":"success","request_id":"794d57b8-…","response":{"behavior":"allow","updatedInput":{"command":"touch stdio-perm-probe.txt","description":"touch probe file"}}}}
```
**[П]** `OBS/runs/f4/stream.jsonl:11-16`, `OBS/scripts/stdio_driver.py:33-37`. Отказ: `{"behavior":"deny","message":"…"}` **[К]** SDKDOC:1404-1412 (опытом не снят). Перед запросом приходит `session_state_changed{requires_action}`, после ответа — `{running}`. Ответ, отправленный через 20 с (задержка задана опытом 2026-11-11, claude 2.1.322), принят: своего таймаута у харнесса на этом интервале нет **[П]** f4:12→15. <!-- MEASURE id=claude-permission-wait date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=lab/observability/claude/runs/f4/stream.jsonl note="n=1, поддельный API; 40 с через MCP-обработчик тоже прошли (reports/46a); предел ожидания не искали" -->

- Во время ожидания разрешения `tool_progress` не шлётся **[П]** R46a:37 — сторож не должен считать это тишиной.
- С MCP-обработчиком (`--permission-prompt-tool mcp__srv__tool`) состояние остаётся `running`, в потоке тишина **[П]** R46a:41,196 — для драйвера брать `stdio`.
- В bypass-режимах (`bypassPermissions`, `dontAsk`, `auto` для разрешённого) запросов нет **[К]** R34:292. Переспросит ли харнесс после resume — **[НП]** R34:382.

### 3.7 `tool_progress` — пульс долгого инструмента
```json
{"type":"tool_progress","tool_use_id":"toolu_01Tc…-heartbeat-0","tool_name":"Bash","parent_tool_use_id":"toolu_01Tc…","elapsed_time_seconds":30,"heartbeat":true,"session_id":"…","uuid":"…"}
```
**[П]** `OBS/runs/r2/stream.jsonl:29-30` — на 30-й и 60-й секунде Bash на 65 с (замер 2026-11-11, claude 2.1.322). <!-- MEASURE id=claude-tool-heartbeat date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/46a,lab/observability/claude/runs/r2/stream.jsonl note="один прогон R2, Bash главного треда; интервал 30 с; перемерить на MCP-инструменте и на субагентах" -->
Только для инструментов главного треда; внутри субагентов пульса нет **[К]** SDKDOC:5687 (опытом **[НП]** R46a:333).

### 3.8 Фоновые задачи и субагенты
```json
{"type":"system","subtype":"task_started","task_id":"b1ylzmvdx","run_id":"0mwbfo5j6-b0b248d9","tool_use_id":"toolu_…","description":"…","is_backgrounded":false,"task_type":"local_bash",…}
{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"ab3f090b4b0d6ac16","run_id":"0mwbfljl7-365478e6","task_type":"local_agent","subagent_type":"probe","description":"Lab probe echo"}],…}
{"type":"system","subtype":"task_started","task_id":"ab3f…","tool_use_id":"toolu_…","subagent_type":"probe","is_backgrounded":true,"spawn_depth":1,"task_type":"local_agent","prompt":"Run: echo sub-ok",…}
{"type":"system","subtype":"task_progress","task_id":"ab3f…","usage":{"total_tokens":3165,"tool_uses":1,"duration_ms":1023},"last_tool_name":"Bash",…}
{"type":"system","subtype":"task_updated","task_id":"ab3f…","patch":{"status":"completed","end_time":1794359974006},…}
{"type":"system","subtype":"task_notification","task_id":"ab3f…","tool_use_id":"toolu_…","status":"completed","output_file":"/tmp/claude-1000/…","summary":"sub-ok","usage":{…},…}
{"type":"system","subtype":"background_tasks_changed","tasks":[],…}
```
**[П]** `OBS/runs/r2/stream.jsonl:28,31`; `OBS/runs/r1/stream.jsonl:63,64,78,101-103,124-125,213-215`.

- `background_tasks_changed.tasks` — **полный текущий набор** (и `local_bash`, и `local_agent`); при старте процесса ничего не шлётся — начинать с пустого **[К]** SDKDOC:5821-5827; **[П]** r1:63,103,124,215. `task_updated`/`task_notification` приходят раньше, чем `background_tasks_changed`, убирающий задачу.
- Субагенты в 2.1.322 по умолчанию **асинхронные**: Agent-инструмент сразу возвращает «Async agent launched», `task_started{is_backgrounded:true}` **[П]** r1:64,69; R46a:166.
- Сообщения субагента в потоке несут `parent_tool_use_id` и `agent_id` **[П]** r1:83.
- Foreground Bash тоже даёт `task_started{is_backgrounded:false}` через ≈3 с после вызова (замер 2026-11-11, claude 2.1.322) **[П]** r2:28. <!-- MEASURE id=claude-fg-bash-task-started date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=lab/observability/claude/runs/r2/stream.jsonl note="n=1; tool_use на 1,6 с, task_started на 4,7 с от старта" -->
- Харнесс сам отбивает `sleep N` в foreground: `tool_result is_error "<tool_use_error>Blocked: sleep 45 …"`; хуков и OTel нет, след только в stream-json **[П]** r1:24, R46a:75.
- Ошибка инструмента: `user.message.content[].{type:"tool_result",is_error:true}` **[П]** r1:166 — рядовое событие хода, не сбой сессии.

Прочее, что встречается в потоке (можно игнорировать, но не падать): `system/status{status:"requesting"|"compacting"|null,…}`, `system/thinking_tokens`, `system/hook_started|hook_response`, `stream_event`, `user` (tool_result), `assistant` **[П]** r1, `claude/logs/exp7-native-400ctx.ndjson:7-8`.

## 4. «Idle без фоновых задач»

| Сигнал | Что значит | Метка |
|---|---|---|
| `result` | конец хода главного треда; фон может ещё идти | [П] r1:212-214 (фоновый Bash завершился через 5 с после `result`; 2026-11-11, claude 2.1.322) |
| `session_state_changed{idle}` | по умолчанию (claude ≥ 2.1.269) состояние остаётся `running` после конца хода, пока жив фоновый **агент или workflow**; **фоновые shell-команды `running` не держат**. `CLAUDE_CODE_BG_TASKS_REPORT_RUNNING=0` выключает удержание | [К] ENVDOC:216; опытом [НП] R46a:336 |
| `background_tasks_changed{tasks:[]}` | живых фоновых задач нет (включая shell) | [П] r1:215 |

<!-- MEASURE id=claude-bg-after-result date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=lab/observability/claude/runs/r1/stream.jsonl note="n=1, фоновый sleep 30; result на 29,9 с, task_notification на 35,2 с" -->
Рабочее правило: «idle без фона» = последнее состояние `idle` **и** последний `background_tasks_changed.tasks` пуст (или не приходил с запуска процесса). Сочетание обоих сигналов в одном прогоне не снято **[НП]**: в R1 не было `session_state_changed`, в R2 и F4 не было фона. Это опыт 1 из §16 постановки.

- После завершения фоновой задачи простаивающая сессия **сама начинает ход** (`init` → … → `result` с `origin.kind:"task-notification"`) **[П]** r1:213-241. Финишер может быть вызван и в этом ходе.
- Одноразовый `-p "<prompt>"` со stdin=/dev/null ждёт фоновую команду и делает ещё один ход перед выходом **[П]** R46a:170; **[К]** `claude/logs/doc-headless.md:77`.

## 5. Сбои §7.1 в нативном потоке

| kind | Сигнал | `retry_at` | Ретраит сам | Метка, источник |
|---|---|---|---|---|
| `rate_limited` | `api_retry{error:"rate_limit",error_status:429}`×N → `assistant{error:"rate_limit",is_api_error_message:true,api_error_status:429,message.model:"<synthetic>"}` → `result{is_error:true,api_error_status:429,terminal_reason:"api_error"}` | нет (`retry_delay_ms` — только пауза до следующей попытки) | да | [П] `claude/logs/exp7-native-429.ndjson:3-6` |
| `quota_exhausted` (подписка) | `rate_limit_event{status:"rejected",rateLimitType:"five_hour"\|"seven_day",resetsAt}` + `assistant.error:"rate_limit"` с текстом «You've hit your … limit · resets 6:20am (Asia/Bangkok)» | `rate_limit_info.resetsAt` | **[НП]** | форма [К] R34:58, SDKDOC:5909; текст из реального транскрипта [П] R14:120 |
| квота при API-ключе / поддельном API | 429 с `retry-after: 3600` и unified-заголовками: **без `api_retry`**, сразу `assistant{error:"rate_limit"}` + `result{is_error,429,terminal_reason:"api_error"}`; `rate_limit_event` нет, `retry-after` в `result` не передаётся → классифицировать как `rate_limited` без времени | нет | нет (1 запрос на ход) | [П] `claude/logs/exp7-native-quota.ndjson:3-4`, `claude/logs/exp7-fake-native-quota.log`; R34:78 |
| `overloaded` | `api_retry{error:"overloaded",error_status:529}`×N → `assistant{error:"server_error",api_error_status:529}` → `result{is_error,api_error_status:529}`. 500 даёт тот же `server_error` — различать по `api_error_status` | нет | да | [П] `claude/logs/exp7-native-529.ndjson:3-6`; R46a:205 |
| `auth_required` | `api_retry{error:"authentication_failed",error_status:401}` — **ретраится как временный**: 7 попыток за 45 с (замер 2026-11-10, claude 2.1.322; маркер ниже); финальный `result` в логе **не снят** (опыт оборван на 60 с). Классифицировать по первому же `api_retry` с 401 | нет | да | [П] `claude/logs/exp7-native-401.ndjson:3-9` (в файле нет `result`) |
| нет логина вовсе | нативно не снималось; через ACP — отказ за 100 мс | — | — | [НП]; [П через ACP] R14:95,110 |
| `billing` | `assistant.error:"billing_error"\|"account_on_hold"`; `rate_limit_info.errorCode:"credits_required"` | нет | — | [К] R34:62, SDKDOC:5927; [НП] |
| `context_overflow` | `assistant{error:"invalid_request"}` → `result{is_error,api_error_status:400,terminal_reason:"prompt_too_long"}`; на следующем ходе `system/status{status:"compacting"}` → `{compact_result:"failed",compact_error:"too_few_groups"}` | — | нет | [П] `claude/logs/exp7-native-400ctx.ndjson:3-4,7-10` |
| `config_error` | `assistant.error:"model_not_found"`; сбой старта — `result.subtype:"error_during_execution"` | — | — | [К] R34:64; [НП] |
| `harness_crash` | EOF на stdout + код выхода/сигнал от `wait` | — | — | [П] R34:65, `OBS/runs/f6.out` (rc=137) |
| `tool_failure` | `tool_result.is_error`; MCP — `init.mcp_servers[].status` | — | — | [П] r1:166; [К] R34:71 |
| `stall` (порождает ядро) | сигнала нет: тишина при `running`. Свои watchdog'и claude (умолчания по документации на 2026-11-11, claude 2.1.322): байтовый 180 с (300 с за `ANTHROPIC_BASE_URL`), событийный 300 с, субагенты 10 мин | — | — | [К] R46a:210-213 (по документации; опыт шёл с 10 с) |
| `runaway_turn` (ядро) | при `--max-budget-usd`: `result.subtype:"error_max_budget_usd"` | — | — | [К] R34:68 |
| `transcript_divergence` | сигнала нет: блокировки нет, второй процесс молча ветвит JSONL | — | — | [П] R14:142 |

Ретраи: по умолчанию `max_retries: 10`, задержки 0,57 → 1,0 → 2,3 → 4,9 → 9,7 → 19,9 → 32,8 с (замер 2026-11-10, claude 2.1.322) **[П]** `claude/logs/exp7-native-401.ndjson:3-9`. <!-- MEASURE id=claude-self-retries date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=lab/claude/logs/exp7-native-401.ndjson note="поддельный API, 401; 7 попыток за 45 с, опыт оборван; все 10 попыток и финальный result не сняты; перемерить на 429 без CLAUDE_CODE_MAX_RETRIES" -->
С `retry-after: 1` задержка 1000–1225 мс (замер 2026-11-10, claude 2.1.322, `CLAUDE_CODE_MAX_RETRIES=2`) **[П]** `claude/logs/exp7-native-429.ndjson:3-4,9-10`. <!-- MEASURE id=claude-retry-after-1s date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=lab/claude/logs/exp7-native-429.ndjson note="поддельный API, retry-after: 1, max_retries=2; 4 значения" -->
<!-- MEASURE id=claude-stream-watchdog-defaults date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/46a,lab/observability/claude/web/network-config.md note="значения из документации вендора, не замер; опыт F1 шёл с байтовым watchdog 10 с" -->
Вывод для драйвера: поверх claude не повторять; `retried_by_harness=true` для 429/529/500/401.

В транскрипте тот же сбой: `{"type":"system","subtype":"api_error","level":"error","error":{…status…}}` на попытку и `{"type":"assistant","error":"rate_limit","isApiErrorMessage":true,"apiErrorStatus":429}` (camelCase, не как в потоке) **[П]** `claude/cfgFake/projects/…-work-fail/02491a51-….jsonl:19,23,25`. `rate_limit_event` в транскрипт не пишется **[П]** R34:132.

## 6. Процесс: смерть, EOF, транскрипт

| Событие | Поведение | Метка, источник |
|---|---|---|
| `kill -9` claude посреди хода | EOF + rc=137; хук `SessionEnd` не срабатывает; stdio-MCP завершаются сами по EOF | [П] R46a:215-222, R44:174 |
| дочерний Bash после `kill -9` | **осиротел (ppid=1, своя pgid) и доработал** — `kill(-pgid)` процесса claude его не достаёт. Снято на Bash workflow-агента внутри `claude -p`; для Bash главного треда [НП] | [П] R44:174 (`cc-native/logs/e3*`) |
| мусор после `kill -9` | `<cfg>/sessions/<pid>.json` остаётся со `status:"busy"`; сокет `/tmp/cc-socks/<pid>.sock` остаётся | [П] R46a:218-220, R44:175 |
| EOF на stdin после `result` | выход с кодом 0 через 0,49 с (замер 2026-11-11, claude 2.1.322) | [П] `OBS/runs/f4/stream.jsonl:25-27` |
| EOF на stdin посреди хода | ход **дорабатывает без наблюдателя** (побочные эффекты происходят), затем выход: 17 с в опыте; простаивающий выходит < 1 с (замер 2026-11-10, claude 2.1.321) | [П через ACP] R14:125, R34:267 (`claude/exp8c-orphan.mjs`: смерть адаптера = EOF для claude) |
| смерть родителя (демона) | тот же EOF; `PDEATHSIG` нет → перед resume дождаться выхода старого pid или убить, иначе два писателя | [П через ACP] R34:267,271 |
| resume | контекст цел, до `init` 0,30 с (замер 2026-11-10, claude 2.1.322) | [П] R42:25 |
| resume прерванного хода | ход сам не продолжается; вендорский механизм — `CLAUDE_CODE_RESUME_INTERRUPTED_TURN=1` (+ `…_MAX_AGE_MS`, `CLAUDE_CODE_RESUME_PROMPT`) | [К] ENVDOC:373-376; [НП] |
| код выхода одноразового `-p` | 0 при успехе, **1 при `result.is_error`** (квота, 500 после ретраев) | [П] `OBS/runs/f2/meta.json`, `f3/meta.json` |
<!-- MEASURE id=claude-eof-exit-after-result date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=lab/observability/claude/runs/f4/stream.jsonl note="n=1, поддельный API, t_ms строк 25 и 27; без фоновых задач" -->
<!-- MEASURE id=claude-eof-midturn-finish date=2026-11-10 ver=claude-2.1.321 env=linux-arm64-orbstack src=reports/14,lab/claude/exp8c-orphan.mjs note="через ACP-адаптер, команда sleep 12; n=1; перемерить напрямую на claude -p" -->
<!-- MEASURE id=claude-resume-time date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42 note="одноходовая сессия, haiku, n=6, медиана [0,27–0,32]; через ACP 0,4–0,5 с (reports/14); длинный транскрипт не мерили" -->

**Транскрипт.** `<cfg>/projects/<cwd, где каждый символ не из [a-zA-Z0-9] заменён на ->/<session_id>.jsonl`, `<cfg>` = `$CLAUDE_CONFIG_DIR` или `~/.claude` **[П]** `claude/exp8-cross.mjs:11-12`, каталог `claude/cfgFake/projects/`. Имя каталога можно задать: `CLAUDE_CODE_PROJECT_DIR_NAME` (только вместе с `CLAUDE_CONFIG_DIR`) **[К]** ENVDOC:365.
- Запись промпта: `{"type":"user","promptId":…,"uuid":…,"parentUuid":…,"sessionId":…,"cwd":…,"message":{…}}` **[П]** `…/02491a51-….jsonl:3`. Первые строки — `queue-operation`.
- Во время инструмента и во время ожидания разрешения файл не меняется (последняя запись `assistant/tool_use`) **[П]** R46a:247. Внутренние типы записей (`atis-latch`, `api-request`, …) нестабильны.
- Два писателя дают две ветки от одного `parentUuid`; resume идёт по самой свежей **[П]** R14:142.

**Реестр процесса.** `<cfg>/sessions/<pid>.json`: `{"pid":864632,"sessionId":"…","cwd":"…","procStart":"218699079","status":"busy","entrypoint":"sdk-cli","messagingSocketPath":"/tmp/cc-socks/864632.sock",…}` **[П]** `claude/cfgFake/sessions/864632.json`. Живость определять по pid + `procStart`, а не по `status` **[П]** R46a:220. `--session-id=<uuid>` виден в cmdline **[П через ACP]** R34:271.

## 7. Логин и окружение

- Проверка до старта: `claude auth status --json` **[К]** R34:317 (в лаборатории не запускалась). После старта — `init.apiKeySource`.
- Отдельный `CLAUDE_CONFIG_DIR` без учётных данных → отказ аутентификации **[П через ACP]** R14:95.
- Изолированный `CLAUDE_CONFIG_DIR` + `CLAUDE_CODE_OAUTH_TOKEN=<accessToken из ~/.claude/.credentials.json>` — реальные ходы проходят, `apiKeySource:"none"` **[П]** `OBS/scripts/run.sh:24`, R46a:19. Это короткоживущий токен; долгий выпускает `claude setup-token` **[К]** R14:155. Истечение посреди сессии **[НП]**.

| Переменная | Действие | Метка |
|---|---|---|
| `CLAUDE_CONFIG_DIR` | каталог настроек, транскриптов, реестра сессий, плагинов | [П] `claude/exp7-fail.mjs:18` |
| `CLAUDE_CODE_OAUTH_TOKEN` | OAuth-токен подписки; сильнее keychain | [П] выше; [К] ENVDOC:344 |
| `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL` | ключ и адрес API (так подключается поддельный API) | [П] `claude/exp7-fail.mjs:19-20` |
| `CLAUDE_CODE_MAX_RETRIES` | число повторов (умолч. 10, потолок 15) | [П] `…-429.ndjson:3` (`max_retries:2`); [К] ENVDOC:326 |
| `CLAUDE_CODE_RETRY_WATCHDOG=1` | ждать 429/529 бесконечно, молча: один `api_retry{max_retries:300,retry_delay_ms:3599960}` — **не включать под сторожем** | [П] R46a:46,207 |
| `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1` | включает `session_state_changed` | [П] `OBS/scripts/run_stdio.sh:33` |
| `CLAUDE_CODE_BG_TASKS_REPORT_RUNNING` | §4 | [К] |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `DISABLE_TELEMETRY=1` | для тестов на поддельном API | [П] `claude/exp7-fail.mjs:21-22` |
| `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` | не грузить `CLAUDE.md` и авто-память | [П] `OBS/scripts/run.sh:33` |
| `CLAUDE_BYTE_STREAM_IDLE_TIMEOUT_MS` | байтовый watchdog, минимум 10 с | [П] R46a:204,211 |
| `CLAUDE_CODE_EXIT_AFTER_STOP_DELAY`, `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS` | автовыход после idle; запрет фоновых задач | [К] ENVDOC:299,237; [НП] |

Окружение собирать явно: в опытах из env убирались все `CLAUDE*`, `ANTHROPIC*`, `AI_AGENT*` родителя **[П]** `coldstart/scripts/native.mjs:9-16`.

## 8. Замеры (все — haiku, один стенд)

| Что | Значение | Дата, версия | Источник |
|---|---|---|---|
| spawn → `init` (prompt в argv), роль с MCP/хуками/промптом 9,5 КБ | 0,28 с [0,26–0,34], n=6; «голый» 0,25 с | 2026-11-10, claude 2.1.322 | R42:21,24 |
| то же с личным конфигом оператора | 2,21 с; без его MCP 0,40 с | 2026-11-10, claude 2.1.322 | R42:22-23 |
| spawn → первый токен | 1,9 с (почти всё — TTFT модели) | 2026-11-10, claude 2.1.322 | R42:7 |
| хуки от spawn: SessionStart / UserPromptSubmit / Stop | +0,19 / +0,27 / +1,93 с (Stop — до `result`) | 2026-11-10, claude 2.1.322 | R42:41 |
| память процесса после хода | RSS 216–234 МБ; PSS 156–158 МБ при 4 процессах | 2026-11-10, claude 2.1.322 | R42:21-25, R14:46 |
| 3 простаивающих `claude -p` без хода, каждый со stdio-MCP | 140–160 МБ PSS на процесс | 2026-11-11, claude 2.1.322 | R44:186-187 |
<!-- MEASURE id=claude-coldstart-init date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42 note="haiku, n=3–6, медианы от spawn, prompt в argv; версия взята из CONTEXT.md; перемерить на моделях ролей и в режиме stream-json input" -->
<!-- MEASURE id=claude-host-config-start date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42 note="конфиг владельца, n=6; почти всё время — его MCP-серверы; зависит от набора MCP" -->
<!-- MEASURE id=claude-first-token date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42 note="haiku, --include-partial-messages, первый text_delta; на крупных моделях TTFT больше" -->
<!-- MEASURE id=claude-hook-timing date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42,lab/coldstart/run/hookprobe.log note="n=1, конфиг A3H" -->
<!-- MEASURE id=claude-process-rss date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42,reports/14 note="короткий ход reply OK; на длинных сессиях не мерили" -->
<!-- MEASURE id=claude-idle-pss date=2026-11-11 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/44 note="3 процесса --input-format stream-json без хода, smaps_rollup" -->

## 9. Открытые дефекты и особенности, важные для автономных сессий

- Лимит в headless не ждёт: в `-p` агент падает на лимите (ожидание есть только в интерактиве или с `CLAUDE_CODE_RETRY_WATCHDOG`) **[К]** R44:34.
- 401 ретраится до минуты, прежде чем станет виден **[П]** R34:77.
- Блокировок сессии нет: второй `claude --resume <id>` при живом первом молча ветвит транскрипт **[П]** R14:142.
- Незакрытые после `kill -9` дочерние Bash со своей pgid (§6) — инвариант «сирот нет» не обеспечивается `kill(-pgid)`.
- HTTP-MCP после рестарта сервера claude переподключает фоново через 6,8–20,6 с, в одном прогоне не успел за 25 с и ход сообщил «MCP server failed to start» (замер 2026-11-10, claude 2.1.321) **[П через ACP]** R36:13. <!-- MEASURE id=claude-mcp-reconnect date=2026-11-10 ver=claude-2.1.321 env=linux-arm64-orbstack src=reports/36 note="через ACP-адаптер, 4 прогона, самописный MCP-сервер; на stateless 2026-07-28 рестарт прозрачен" -->
- `tool_progress` нет в субагентах и при ожидании разрешения; долгая генерация и зависший стрим снаружи неразличимы **[П]** R46a:33-37.
- Истечение OAuth посреди сессии, форма `rate_limit_event{rejected}`, судьба запроса разрешения после resume — **[НП]** R34:378-382.

## 10. Расхождения в отчётах

| Тема | Значение А | Значение Б |
|---|---|---|
| финал 401 | R14:109 и R34:61: «затем `result is_error`, 401» со ссылкой на `exp7-native-401:3-9` | в самом логе 9 строк и `result` нет — опыт оборван таймаутом скрипта |
| дети после `kill -9` | R34:254: «[НП]» | R44:174: Bash осиротел, своя pgid [П] |
| resume | R34:18 «0,4–0,5 с» (через ACP) | R42:25 «0,30 с до init» (нативно) |
| память процесса | R14:46: 156–158 МБ PSS | R42:21-25: 216–234 МБ RSS (разные метрики) |
| число повторов | R34:16 «до 10 попыток с задержкой 0,5→33 с» | в логе снято 7 попыток, первая задержка 567 мс |

## 11. Чего здесь нет

Выяснять опытом (отчёты читать **не нужно**, кроме одной оговорки ниже):
- форма эха `--replay-user-messages` и control-запроса `interrupt` на проводе; `--resume` вместе с `--input-format stream-json`; сброс модели после нативного resume;
- поведение `session_state_changed` при живом фоновом субагенте и правило «idle без фона» одним прогоном (§4);
- EOF на stdin и `kill -9` посреди Bash главного треда напрямую на `claude -p`; способ убить потомков со своей pgid (cgroup на Linux, macOS — отдельно);
- финальный `result` после исчерпания ретраев на 401; старт без логина; несуществующая модель; `insufficient_quota`/billing;
- `rate_limit_event{status:"rejected"}` при настоящем исчерпании подписки — только вручную;
- доставка учётных данных при выключенном наследовании хоста (долгий токен, его истечение);
- форма отказа (`deny`) в `control_response`, таймаут ответа на запрос разрешения, повторная выдача запроса после resume;
- подключение человека родным TUI (`claude --resume <id>` упёрся в диалог доверия, R14:143).

Оговорка: полный перечень полей сообщений SDK (типы `SDK*Message`) — в `lab/claude/logs/doc-agent-sdk_typescript.md` (332 КБ, снимок документации вендора, не отчёт); открывать точечно по имени типа.
