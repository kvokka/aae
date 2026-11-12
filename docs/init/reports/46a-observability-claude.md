# 46a — Как видеть «жива / зависла / ждёт / сыплет ошибками» у сессии Claude Code: OTel и всё остальное

Дата: 2026-11-11. `claude` 2.1.322. Документация скачана 11.11:
- `WEB/monitoring-usage.md` (вариант `.md` отдаётся, 1845 строк);
- `WEB/network-config.md`, `WEB/statusline.md`, `WEB/tools-reference.md`, `WEB/agent-sdk-typescript.md`;
- остальное — из `CCWEB/` (сохранено сегодня другим агентом).

Сокращения:
- `LAB` = `tmp/gc-alt-research/lab/observability/claude`, `WEB` = `LAB/web`, `CCWEB` = `lab/cc-native/web`;
- `R*`/`F*`/`G*` — прогоны, сырьё лежит в `LAB/runs/<tag>/`: `otel.jsonl`, `hooks.jsonl`, `stream.jsonl`, `samples.jsonl`, `agents.jsonl`, сводный `timeline.txt`.

Метки: **[П]** проверено (опыт или строка документации), **[В]** вывод, **[НП]** не проверено.

**Бюджет.**
- Реальных прогонов на haiku 3: R1 и R2 — `-p`, R3 — `--bg`. Суммарно ≈ $0,014 (`result.total_cost_usd` и `cost.usage`).
- Фейковых прогонов `claude -p` 11: F0–F6, G1, G2 и два служебных. Они шли против локального фейкового Anthropic API (`LAB/scripts/fake_api.mjs`) и не тратят квоту.
- Одновременно работали не больше двух сессий.

Всё запускалось с изолированным `CLAUDE_CONFIG_DIR=LAB/cc-<tag>`. `~/.claude` только читался: токен брался из `.credentials.json` в env. Проверено, что копий токена в `LAB` нет.

## 1. Выводы за 60 секунд

1. **Гипотеза «OTel задним числом» подтверждена.** **[П]**
   - Логи, метрики и спаны приходят только по завершении действия.
   - Пустых экспортов нет даже при интервалах 1 с.
   - В R2 Bash-вызов на 65 с дал паузу в 61,7 с по `/v1/logs`, 66 с по `/v1/metrics` и 65 с по `/v1/traces`.
   - Ожидание разрешения в 40 с тоже прошло в полной тишине.
   - `active_time.total` не пульс: он приходит одним числом в конце interaction (`108.444` в R2).
2. **Долгий tool, ожидание разрешения и висящий стрим в OTel различимы, но только косвенно.**
   - `tool_decision{decision=accept}` приходит **до** исполнения tool, `tool_result` — после. Открытая пара по `tool_use_id` значит «tool X идёт с T0» (R2: решение на +2,8 с, результат на +67,9 с). **[П]**
   - Разрешение ждёт ход, где `llm_request` закончился с `stop_reason=tool_use`, а `tool_decision` ещё нет. **[П]**
   - Если зарегистрирован хотя бы один `PermissionRequest`-хук, OTel получает `hook_execution_start{hook_event=PermissionRequest}` сразу в начале ожидания. **[П]**
   - Долгую генерацию и зависший стрим OTel не различает никак: в обоих случаях тишина. **[П]** (F1)
3. **Штатный heartbeat есть, но в stream-json / Agent SDK, а не в OTel.**
   - `tool_progress{heartbeat:true, elapsed_time_seconds}` приходит каждые 30 с, пока идёт tool главного треда (R2: 30 и 60 с). **[П]**
   - Внутри субагентов heartbeat-ов нет (`WEB/agent-sdk-typescript.md:5687`). **[П, док]**
   - Во время ожидания разрешения heartbeat тоже не шлётся (R2). **[П]**
4. **Состояние сессии.**
   - `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1` добавляет в stream-json событие `session_state_changed` со значениями `running|idle|requires_action`. **[П]**
   - `requires_action` выставляется только для SDK-разрешений (`--permission-prompt-tool=stdio`, `canUseTool`), F4. **[П]**
   - При MCP-разрешении через `--permission-prompt-tool mcp__…` состояние остаётся `running`. **[П]** (R2)
5. **Ошибки API в OTel видны только финалом.**
   - `api_error` шлётся, лишь когда ретраи исчерпаны. **[П]** (`WEB/monitoring-usage.md:1604`, F3)
   - 529 и 500, после которых пришёл успех, не оставили в OTel ни одного `api_error`. **[П]** (F1)
   - stream-json даёт `system/api_retry` на каждую попытку сразу. **[П]**
   - С `CLAUDE_CODE_RETRY_WATCHDOG=1` сессия ждёт сброса лимита молча: один `api_retry` с `retry_delay_ms=3599960`, в OTel ничего. **[П]** (F5)
   - `resetsAt` и `utilization` есть только в stream-json `rate_limit_event` и в statusline, в OTel их нет. **[П]**
6. **`claude agents --json` — штатное программное «прямо сейчас».** **[П]**
   - Поля: `state` (`working|blocked|done|failed|stopped`), `status` (`busy|waiting|idle`), `waitingFor`.
   - В `--bg` сессия перешла `working → blocked/waiting/"permission prompt"` меньше чем за 2 с (R3).
   - `-p`-сессии в списке тоже есть, как `kind: interactive`.
   - Мёртвые pid отфильтрованы (F6).
   - **Но `busy` ≠ «жив»:** `busy` держался при висящем стриме (F1), при ожидании сброса лимита (F5) и при ожидании MCP-разрешения (R1, R2).
7. **Хуки дают точные переходы с T0, но с дырами.** **[П]**
   - Что есть:
     - `PreToolUse` → `PostToolUse`/`PostToolUseFailure`;
     - `PermissionRequest` сразу;
     - `Notification{permission_prompt}` примерно через 6 с;
     - `SubagentStart`/`SubagentStop` с `agent_id`;
     - `StopFailure{error=rate_limit|server_error}`.
   - Дыры:
     - у завершения фонового Bash своего хука нет;
     - вызов, отбитый валидатором (`Blocked: sleep 45 …`), не виден ни хукам, ни OTel.
8. **Метки из `OTEL_RESOURCE_ATTRIBUTES` (`gc.agent`/`gc.rig`/`gc.run`) доходят до каждого сигнала.** **[П]**
   - Они стоят на каждом log record, datapoint и span, плюс в resource.
   - Это верно и для субагентов, и для `--bg`-сессии: ноль пропусков во всех прогонах (`summary.json → missing_custom_attr`).
   - Сессию идентифицирует `session.id`, субагента — `agent_id`/`parent_agent_id`, прогон workflow — `workflow.run_id` (последнее только по доке).
9. **Env Gas City не включает экспорт.** Gas City передаёт `CLAUDE_CODE_ENABLE_TELEMETRY=1` и `OTEL_RESOURCE_ATTRIBUTES` без `OTEL_*_EXPORTER` (`internal/telemetry/subprocess.go:155-191`). С таким env Claude Code **не создаёт ни одного экспортёра**:
   - так и с endpoint и protocol (G1), и без них (G2);
   - ни на sink, ни на дефолтные 4318 и 4317 не пришло ни одного запроса;
   - в debug-логе: `getOtlpReaders: types=[]`, `Created 0 log exporter(s)`, `Event dropped (no event logger initialized)`.

   **[П]**
10. **Самое неожиданное.** **[П]**
    - `sleep 45` в foreground Claude Code отбивает сам («Blocked: sleep 45 … use run_in_background»). Ни хуков, ни OTel; след есть только в stream-json.
    - Агент-тул async-субагента в OTel «завершён» за 7 мс. Реальный конец — `subagent_completed{is_async:true, duration_ms:1778}`.
    - Ошибка `Read` пришла с `error_type="TelemetrySafeError"`, а не `Error:ENOENT` из доки.
    - После `kill -9` файл `<cfg>/sessions/<pid>.json` остаётся со `status:"busy"`, а `SessionEnd` не срабатывает.
11. **Что придётся писать самим.** Штатно «жива или зависла» одним числом не получить. Минимум своего кода:
    - адаптер stream-json (или хуков с `claude agents --json`) → OTel-gauges со своим периодическим экспортом;
    - правило `now − last_signal > budget(state)`.

    Это ≈ 200–300 строк плюс 5–6 правил алертов (§5). **[В]**

## 2. Что OTel Claude Code даёт по документации

**Включение.** **[П]** (`WEB/monitoring-usage.md:19-36, 108-123, 162-175`)
- Обязательны `CLAUDE_CODE_ENABLE_TELEMETRY=1` и хотя бы один из селекторов `OTEL_METRICS_EXPORTER`/`OTEL_LOGS_EXPORTER`/`OTEL_TRACES_EXPORTER`. Протокол по умолчанию не задан: его ставят явно.
- Интервалы по умолчанию:
  - `OTEL_METRIC_EXPORT_INTERVAL` — 60000 мс;
  - `OTEL_LOGS_EXPORT_INTERVAL` — 5000 мс;
  - `OTEL_TRACES_EXPORT_INTERVAL` — 5000 мс.
- Traces в beta, включаются парой `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1` и `OTEL_TRACES_EXPORTER`.
- Детальный режим (`ENABLE_BETA_TRACING_DETAILED` + `BETA_TRACING_ENDPOINT`) добавляет спан `claude_code.hook`, но перенаправляет логи и трейсы (`:329-335`).
- Переменные `OTEL_*` не доходят до Bash, хуков и MCP (`:71`).

**Метрики (8, все счётчики, по умолчанию delta).** **[П]** (`:644-653`)
- `session.count{start_type}`;
- `lines_of_code.count`, `pull_request.count`, `commit.count`;
- `cost.usage`, `token.usage{type,model,query_source,agent.name,…}` — растут «после каждого API-запроса» (`:698`, `:719`);
- `code_edit_tool.decision`;
- `active_time.total{type=user|cli}`.

Gauge-метрик, «in-flight» и uptime нет.

**События (logs).** Перечислены в `:778-1558`. Для liveness важны такие. **[П]**

| Событие | Когда шлётся | Полезные атрибуты |
|---|---|---|
| `user_prompt` | ход начался, в том числе ход, который Claude начал сам | `prompt.id`, `prompt_length` |
| `api_request` | **после** ответа модели | `duration_ms`, `ttft_ms`, `query_source`, `agent_id`; `stop_reason` нет |
| `api_error` | **финальная** ошибка, после ретраев | `status_code`, `attempt`, `error` |
| `api_retries_exhausted` | вместе с финальным `api_error` | `total_attempts`, `total_retry_duration_ms` |
| `tool_decision` | решение по разрешению, **до** исполнения | `tool_use_id`, `decision`, `source` (`config`/`hook`/`user_temporary`…), `permission_mode` |
| `tool_result` | **после** исполнения | `tool_use_id`, `success`, `duration_ms`, `error_type` |
| `hook_execution_start` / `_complete` | вокруг каждого события хуков, только если хук зарегистрирован | `hook_event`, `hook_name` (`PreToolUse:Bash`); `tool_use_id` нет (`:1689`) |
| `subagent_completed` | субагент закончил | `agent_type`, `is_async`, `duration_ms`, `total_tool_uses` |
| `mcp_server_connection`, `internal_error`, `compaction`, `api_refusal`, `permission_mode_changed`, `auth`, … | по факту | — |

`safety_event` с версии 2.1.312 не шлётся (`:1050-1052`).

**Спаны.** **[П]** (`:191-370`)
- Корень — `claude_code.interaction`. Под ним `llm_request` (`stop_reason`, `attempt`, события `gen_ai.request.attempt`) и `tool`.
- У `tool` два дочерних спана: `tool.blocked_on_user` (время ожидания разрешения, `decision`, `source`) и `tool.execution`.
- Спаны субагента вложены в `claude_code.tool` агент-тула.
- Экспорт идёт batch-ом по **окончании** спана. Незакрытый спан снаружи не виден: R2, спан `tool` на 65 с приехал на +68,9 с.

**Идентификация.** **[П]** (`:587-615`, `:612`, `:614`)
- Сессия: `session.id`, причём `/clear` даёт новый. Для облачных сессий есть `ccr.session.id`.
- Процесс: `event.sequence` монотонен в пределах процесса.
- Субагент: `agent_id` и `parent_agent_id` на API- и tool-событиях и на `subagent_completed`.
- Прогон workflow: `workflow.run_id` (`wf_…`) и `workflow.name`.

**`OTEL_RESOURCE_ATTRIBUTES`.** Ставится и в resource, и атрибутом на каждый datapoint и event (`:423`). Пробел в значении молча отключает копирование ключей (`:478`). **[П]**

**Режимы, в которых работает OTel.**
- **`-p`.** Работает. **[П]** (R1, R2)
  - `app.entrypoint=sdk-cli`.
  - Есть входящий `TRACEPARENT`, так что спаны Claude становятся детьми трейса оркестратора (`:185-189`). **[П док]**
- **`--bg`.** Работает. **[П]** (R3)
  - OTel-переменные были и в env, и в `env` из `--settings`; на `--bg` переносится `--settings` (`CCWEB/agent-view.md:490-497`), а из env оболочки — только `PATH`, провайдер и шлюз (`:699-718`).
  - `app.entrypoint=cli`.
  - Какой из двух каналов сработал, по отдельности не проверялось. **[НП]**
- **Субагенты.** Работает. **[П]** (R1)
- **Workflows.** Только по доке. **[П док]** **[НП опытом]**

**Кандидаты в heartbeat.** **[П]** (R1, R2)
- `active_time.total` — нет. Дока говорит «incremented during … tool execution and AI response generation» (`:745`), но экспорт приходит одной суммой после хода.
- Metric reader при delta temporality не шлёт пустых батчей, поэтому живой, но молчащий процесс неотличим от мёртвого.

## 3. Опыт: стенд и таймлайны

**Стенд.**
- Приёмник: `LAB/../sink/otlp_sink.py`, протокол `http/json`, все три интервала по 1000 мс.
- Метки: `OTEL_RESOURCE_ATTRIBUTES=gc.agent=x,gc.rig=lab,gc.run=<tag>`.
- Хуки: все значимые события → `scripts/hook.py`, время в мс.
- `stream-json` с `--include-partial-messages --include-hook-events`, каждая строка с отметкой времени.
- Сэмплер раз в 0,5 с: `<cfg>/sessions/*.json`, транскрипт (размер, mtime, последняя запись), дерево процессов.
- `claude agents --json` раз в 3 с (в R3 раз в 2 с).
- Разрешения:
  - через MCP-тул `scripts/perm_mcp.py`, который ждёт N с и отвечает allow (R1, R2);
  - SDK-стилем через `scripts/stdio_driver.py`, который отвечает на `control_request can_use_tool` (F4).

**Сценарий R1.** **[П]**
- `sleep 45` отбит валидатором, поэтому долгий tool повторён в R2 через `python3 -c 'time.sleep(65)'`.
- Субагент `probe` запустился **асинхронно**: в 2.1.322 субагенты по умолчанию фоновые.
- Фоновый `sleep 30; echo bg-done`.
- `Read` несуществующего файла.
- `touch` через MCP prompt tool с задержкой 20 с.
- После `STEPS-DONE` `-p` ждал фоновую команду 5 с, затем сделал ещё один ход с уведомлением о задаче (`user_prompt` с `prompt_length=441`).

**Таймлайн R2.** Колонки — время приёма сигнала в секундах от старта (`LAB/runs/r2/timeline.txt`). **[П]**

| t, с | Что происходит | OTel | Хуки | stream-json | agents / реестр / процессы |
|---|---|---|---|---|---|
| 1,2 | старт | — (первый экспорт на 2,2) | `SessionStart` | `session_state_changed: running` | `busy` |
| 2,8 | модель вызвала Bash на 65 с | 3,8: `api_request`, **`tool_decision accept`**, спаны `llm_request(stop_reason=tool_use)` и `blocked_on_user` | `PreToolUse` 2,81 | `assistant tool_use`; 5,8: `task_started{local_bash, is_backgrounded:false}` | `busy`; дети `bash`, `python3` |
| 3,8 → 68,9 | tool идёт | **ничего 65 с** (только посторонний `retention_sweep` на 7,2) | — | **`tool_progress heartbeat` на 32,8 (30 с) и 62,8 (60 с)** | `busy`; транскрипт не пишется с 2,9 по 68,0 |
| 67,9 | tool закончился | 68,9: `tool_result 65042ms`, спаны `tool` и `tool.execution` | `PostToolUse` | `task_notification`, `tool_result` | — |
| 68,9 | `touch` ждёт MCP-разрешения 40 с | 69,9: **`hook_execution_start PermissionRequest`**, потом тишина 40 с | `PermissionRequest` 68,95 | **тишина 40 с**, `session_state` не меняется | **`busy`, `waitingFor: null`**; дочернего shell нет |
| 109,0 | allow | 109,75: `tool_decision{source=user_temporary}`, спан `blocked_on_user 40027ms`, `active_time.total=108.444` | `PostToolUse`, `Stop` | `result`, `session_state: idle` | файл реестра удалён на выходе |

**Субагент и фоновый процесс (R1).** **[П]**

| Сущность | OTel | Хуки | stream-json | Процессы |
|---|---|---|---|---|
| async-субагент `probe` | `tool_result Agent 7 ms success` в момент запуска; события субагента с `agent_id`, `query_source="agent:custom"`; спаны `llm_request.context=tool`; в конце `subagent_completed{is_async:true}` | `SubagentStart`/`SubagentStop` и `PreToolUse` внутри с `agent_id`, `agent_type` | `task_started`/`task_progress`/`task_notification`; сообщения с `parent_tool_use_id` | — |
| фоновый Bash | `tool_result` запуска (4 мс), дальше **ничего** до `user_prompt` следующего хода | `PreToolUse`/`PostToolUse` с `bg:true` в момент запуска; у завершения хука нет | `task_started` → `task_updated`/`task_notification{completed}` (+30 с), `background_tasks_changed` | `bash` и `sleep` под claude все 30 с |
| ошибка tool (`Read`) | `tool_result{success:false, error_type:"TelemetrySafeError"}`; спан `execution` с `status=ERROR` | `PostToolUseFailure{error:"File does not exist…"}` | `tool_result is_error` | — |
| блок валидатора (`sleep 45`) | **нет** | **нет** | `tool_result is_error "<tool_use_error>Blocked: sleep 45 …"` | — |

**Ожидание разрешения в трёх режимах.** **[П]**

| Режим | stream-json | `claude agents --json` / реестр | Хуки | OTel |
|---|---|---|---|---|
| `-p` + MCP `--permission-prompt-tool` (R1, R2) | тишина | `busy`, `waitingFor: null` | `PermissionRequest` сразу, `Notification` нет | `hook_execution_start PermissionRequest`, если хук есть; потом тишина |
| `-p` + `stdio` в стиле SDK (F4) | **`session_state_changed: requires_action`** и `control_request can_use_tool` сразу | реестр `waiting / "permission prompt"` ≤ 0,5 с; agents то же ≤ 3 с | `PermissionRequest` сразу; `Notification{permission_prompt}` через 6,0 с | то же; до решения только посторонний `retention_sweep` (паузы 14 с в логах, 19,5 с в метриках) |
| `--bg` без человека (R3) | — | **`state: blocked`, `status: waiting`, `waitingFor: "permission prompt"`** ≤ 2 с; `jobs/<id>/state.json`: `needs: "approve Bash: touch …"`, `tempo: blocked` | `PermissionRequest`; `Notification{permission_prompt}` +6 с | тишина 52 с, до `claude stop` |

**Ошибки API (фейковый API).** **[П]**

| Сценарий | stream-json | OTel | Хуки |
|---|---|---|---|
| F1: 529 → 500 → стрим висит после `message_start` (byte watchdog прижат к 10 с) → успех | `api_retry` ×2 сразу (`attempt`, `error_status`, `error`, `retry_delay_ms`); на 10 с висящего стрима **ничего**; после watchdog **без `api_retry`** — сразу non-stream fallback | **ни одного `api_error`**; в конце `api_request duration_ms=10024`, у спана `llm_request` 4 события `gen_ai.request.attempt` | `Stop` |
| F3: 500 ×3, `MAX_RETRIES=2` | `api_retry` ×2; `result{is_error, api_error_status:500}` | в конце: `api_error{attempt:3}`, `api_retries_exhausted{total_attempts:3}`, `internal_error`; спан `error_class=server_error` | `StopFailure{error:server_error}` |
| F2: 429 «квота» с unified-заголовками сброса через 1 ч | `assistant{error:rate_limit}`, `result{terminal_reason:api_error, 429}`; **`rate_limit_event` нет** (ключ API) | `api_error 429`, `error_class=rate_limit`; **`resetsAt` нет** | `StopFailure{error:rate_limit}` без `error_details` |
| F5: то же + `CLAUDE_CODE_RETRY_WATCHDOG=1` | один `api_retry{attempt:1, max_retries:300, retry_delay_ms:3599960, error:rate_limit}` | **ничего**; на SIGTERM пришёл только спан `interaction` | — |
| R1, R2 на реальной подписке | `rate_limit_event{status:allowed, resetsAt, rateLimitType:seven_day, utilization:0.06}` после ответа | про лимиты ничего (grep `resets`/`utiliz` = 0) | — |

Зависший стрим штатно ловит только сам Claude, своими watchdog-ами (`WEB/network-config.md:195-210`). **[П док]**
- Byte-level срабатывает через 180 с на прямом API и через 300 с за `ANTHROPIC_BASE_URL`; минимум для `CLAUDE_BYTE_STREAM_IDLE_TIMEOUT_MS` — 10 с.
- Event-level срабатывает через 300 с, меньше 5 минут поставить нельзя.
- Субагентам и агентам workflow отдельно положен `CLAUDE_ASYNC_AGENT_STALL_TIMEOUT_MS` = 10 мин (`CCWEB/env-vars.md:190`).

**Смерть процесса (F6, `kill -9` во время висящего стрима).** **[П]**
- В OTel ничего: финального flush нет.
- `SessionEnd` не срабатывает.
- `<cfg>/sessions/<pid>.json` остаётся со `status:"busy"`.
- У хозяина в `~/.claude/sessions` 24 из 29 файлов принадлежат мёртвым pid, и в 8 из них стоит `busy`. Проверял через `/proc/<pid>`, только чтением.
- Живость надо определять по pid вместе с `procStart`, а не по полю `status`.
- `claude agents --json` мёртвый pid отфильтровал и вернул `[]`.
- Дочерний MCP завершился сам.

**Env Gas City (G1, G2, фейковый API).** **[П]** Записать отдельной строкой:
> `CLAUDE_CODE_ENABLE_TELEMETRY=1` + `OTEL_RESOURCE_ATTRIBUTES`, без `OTEL_METRICS_EXPORTER`/`OTEL_LOGS_EXPORTER` → ноль экспортёров и ноль запросов. Так и с `OTEL_EXPORTER_OTLP_ENDPOINT`+`PROTOCOL=http/json` (G1), и без них (G2: ничего не пришло на 4318, ни одного TCP-подключения к 4317). Debug: `[3P telemetry] getOtlpReaders: types=[] … Created 0 log exporter(s)`, `Event dropped (no event logger initialized)` (`LAB/runs/g1/debug.log`, `g2/debug.log`).

Значит, в нынешнем Gas City OTel у Claude фактически выключен. Исключение — если `OTEL_*_EXPORTER` попадает в env сессии каким-то другим путём; наследует ли сессия gc env контроллера, я не проверял. **[НП]**

## 4. Состояние сессии → чем ловится штатно

Задержки измерены в прогонах этого отчёта. «Надёжность» — оценка **[В]** по опыту.

| Состояние | Лучший штатный сигнал | Задержка | Надёжность | Что в OTel |
|---|---|---|---|---|
| **Жива и думает** | stream-json: `system/status{requesting}`, затем `stream_event` с дельтами (`--include-partial-messages`), `thinking_tokens` | дельты ≈ каждую секунду генерации (R1) | высокая для `-p`/SDK; в `--bg`/интерактиве — только `agents --json: busy` | ничего до конца запроса (`api_request` после ответа) |
| **В tool call** | хук `PreToolUse` (T0, tool, `tool_use_id`) → `Post*`; `tool_progress` heartbeat каждые 30 с; `task_started` через ≈ 3 с; дочерний shell в таблице процессов | 0 с хук; 30 с пульс | высокая в главном треде; в субагенте без пульса; отбитый валидатором вызов невидим | косвенно: `tool_decision` без `tool_result` (≈ 1 с); `hook_execution_start PreToolUse`, если хук есть |
| **Ждёт разрешения** | `--bg`/интерактив: `agents --json` `blocked/waiting/"permission prompt"`; SDK: `requires_action` и `control_request`; везде хук `PermissionRequest` сразу, `Notification{permission_prompt}` через ≈ 6 с | 0–2 с | высокая, **кроме MCP prompt tool**: там состояние `busy/running` и только хук | `hook_execution_start PermissionRequest`, если хук есть; потом тишина; спан `blocked_on_user` задним числом |
| **Ждёт ввода** | `agents --json`: `blocked` + `"input needed"` (вопрос) либо `done` (ход окончен); stream-json `result` + `idle`; хуки `Stop`, `Notification{idle_prompt}` через 60 с | 0–3 с | высокая (`--bg`); в `-p` значит «процесс завершился или ждёт stdin» | конец спана `interaction`, дальше тишина, неотличимая от зависания |
| **Упала на лимите** | без watchdog: `StopFailure{rate_limit}`, `result{api_error_status:429}`; на подписке `rate_limit_event{resetsAt, utilization}`; в интерактиве `Notification{quota_auto_resume_*}`, statusline `rate_limits` | 0 с | высокая; с watchdog сессия **ждёт молча** (виден один `api_retry` с большим `retry_delay_ms`, `busy`) | `api_error 429` без `resetsAt`; с watchdog ничего |
| **Сыплет ошибками** | stream-json `api_retry` на каждую попытку; ошибки tool — хук `PostToolUseFailure` | 0 с | высокая | только финальный `api_error`/`api_retries_exhausted`; попытки видны как span-события по окончании; `tool_result{success:false}` ≈ 1 с |
| **Фоновый процесс идёт** | stream-json `task_started` / `background_tasks_changed` / `task_notification`; дети в таблице процессов; в `-p` процесс держится открытым | 0 с | высокая | запуск `tool_result` за 4 мс, завершения нет |
| **Зависла (стрим молчит)** | **прямого сигнала нет**: собственный watchdog Claude (180–300 с) с retry/fallback | 3–5 мин, и то внутри Claude | низкая; снаружи только «нет событий при `running` и без tool» | тишина; потом длинный `api_request.duration_ms` |
| **Процесс мёртв** | pid + `procStart` в таблице процессов; `agents --json` убирает запись; supervisor `--bg` перезапускает | сразу / ≤ период опроса | высокая; **файл `sessions/<pid>.json` после `kill -9` врёт (`busy`)** | тишина, без flush и без `SessionEnd` |

Дополнительно:
- **statusline** — периодический источник (`refreshInterval` ≥ 1 с, `WEB/statusline.md:67,139-154`) с `rate_limits.*.resets_at`, `cost`, `context_window`. Но он есть только при отрисованном TUI. Работает ли он для неприкреплённой `--bg`-сессии и в `-p`, не проверено. **[НП]**
- **Транскрипт JSONL** в момент tool и в момент разрешения выглядит одинаково: последняя запись `assistant/tool_use`, файл не меняется (R2: 2,9 → 68,0 и 69,0 → 109,1 с). Типы записей внутренние (`atis-latch`, `cost-state`, `api-request`) и нестабильны (`WEB/monitoring-usage.md:772`). Как отдельный сигнал жизни он слабее хуков. **[П]**

## 5. Вердикты по гипотезам

- **H1 (tmux не нужен; квота, 429 и краш решаются без него).** В части наблюдаемости для Claude подтверждается. **[В на основе П]**
  - Всё, ради чего Gas City читает pane, доступно программно, без скрейпинга: разрешение, лимит с `resetsAt`, ретраи, ошибки, жив или мёртв.
  - Источники: stream-json/SDK, `claude agents --json`, хуки, таблица процессов.
- **H8 (тики против событий).** Частично. Переходы приходят событиями (хуки, stream-json). Но «зависла» — это **отсутствие** события, и без таймера её не поймать. **[В]**
  - Нужен дедлайн на сессию (пересчитывается на каждом сигнале), а не глобальный тик-реконсайл.
  - Сам OTel событий «в процессе» не даёт.
- **H3 (ACP как единственный транспорт).** Не моя часть. Замечание: heartbeat-ы `tool_progress`, `session_state_changed` и `api_retry` — сообщения SDK-протокола. Пробрасывает ли их `claude-agent-acp` в ACP, не проверено. **[НП]** Если нет, через ACP «жива или зависла» хуже видно, чем через SDK.

## 6. Предложение: минимальный свой код для «жива или зависла» per session

**Что берём штатно, без кода.**
1. OTel Claude Code — для аналитики задним числом: стоимость, токены, длительности tool и API, финальные ошибки. Сшивается через `session.id` и `gc.*` из `OTEL_RESOURCE_ATTRIBUTES`. Gas City здесь должен добавить `OTEL_METRICS_EXPORTER`/`OTEL_LOGS_EXPORTER` (и протокол), иначе не уходит ничего (G1, G2).
2. Сигналы «в моменте»:
   - stream-json или SDK: `tool_progress`, `session_state_changed`, `api_retry`, `rate_limit_event`, `task_*`, дельты;
   - `claude agents --json --all`;
   - хуки;
   - таблица процессов.

**Что пишем сами (≈ 200–300 строк, Python или Go).** **[В]**

*Вариант A — флот на `-p`/SDK; это и есть дизайн «оркестратор сам запускает сессии».* Оркестратор и так читает stream-json каждой сессии. Добавляется в нём:
1. **Автомат на сессию.**
   - Состояния:
     - `requesting` — после `status:requesting`, до первой дельты;
     - `generating`;
     - `tool(name, since)` — по `assistant tool_use` или `task_started`, до `tool_result`;
     - `awaiting_permission(since)` — по `control_request` или `requires_action`, либо по хуку `PermissionRequest`, если разрешения идут через MCP;
     - `retrying(error, until=now+retry_delay_ms)`;
     - `rate_limited(resetsAt)`;
     - `idle`;
     - `bg_tasks(n)`;
     - `exited(rc)`.
   - `last_signal_ts` обновляется на **любой** строке stream-json, включая `tool_progress` и дельты.
2. **Экспорт через OTel SDK самого оркестратора как observable gauges.** Они уходят каждый интервал независимо от активности, поэтому сам экспорт и есть heartbeat оркестратора:
   - `gc_session_state{state}`;
   - `gc_session_last_signal_age_seconds`;
   - `gc_session_tool_elapsed_seconds{tool}`;
   - `gc_session_retry_attempt{error}`;
   - `gc_session_rate_limit_reset_ts`;
   - `gc_session_bg_tasks`;
   - `gc_session_up` (pid жив и `procStart` совпадает).

   Метки: `session.id`, `gc.agent`, `gc.rig`. Ресурс тот же, что у Claude, — тогда дашборды склеиваются.
3. **Правила алертов в бэкенде, 5–6 штук:**
   - **висит модель:** `state∈{requesting,generating}` и `age > 120 s`. Дельты идут каждую секунду; собственный watchdog Claude срабатывает только на 180–300 с;
   - **висит tool:** `state=tool` и `age > 45 s`. В главном треде `tool_progress` приходит каждые 30 с, так что молчание дольше 45 с значит, что встал сам процесс. Второе условие: `tool_elapsed > budget(tool)`. Foreground Bash по умолчанию уходит в фон через 120 с (`BASH_DEFAULT_TIMEOUT_MS`), у stdio-MCP idle-таймаут 30 мин;
   - **ждёт человека:** `awaiting_permission > N мин`. Это не «зависла», а повод кого-то позвать;
   - **лимит:** `rate_limited` — не алерт, а «спит до `resetsAt`». Так же `retrying` с `retry_delay_ms` больше 5 мин;
   - **ошибки:** скорость `api_retry` на сессию больше X в минуту;
   - **мертва:** `up==0` без `result` или `exited`.

*Вариант B — `--bg` и интерактивные сессии, где stream-json не наш.* Нужен сайдкар:
- опрос `claude agents --json --all` раз в 2–5 с: `state`, `status`, `waitingFor`;
- `command`-хуки `PreToolUse`/`PostToolUse*`/`PermissionRequest`/`Stop`/`StopFailure`/`SubagentStart`/`SubagentStop`/`SessionEnd`. Хук дописывает строку в сокет или файл адаптера.

Хуки не получают `OTEL_*` env (`WEB/monitoring-usage.md:71`), поэтому адрес адаптера передаётся своей переменной, например `GC_LIVE_URL`. Пульса для tool здесь нет: `tool_progress` в хуки не приходит. Поэтому «висит tool» = `PreToolUse` без `Post*` дольше `budget(tool)` плюс проверка детей в таблице процессов. **[В]**

*Вариант C — «только OTel», без своего транспорта. Не рекомендую.*
- Пустой хук на `PreToolUse`/`PermissionRequest` заставляет OTel слать `hook_execution_start` в начале ожидания.
- Но `tool_use_id` в нём нет, а «зависла» всё равно требует состояния и таймера на стороне коллектора.
- Получается тот же автомат, только хуже. **[В]**

**Граница «штатно / своё».**

| Что | Статус |
|---|---|
| 30-секундный пульс tool главного треда, `session_state_changed`, `api_retry` по попыткам, `rate_limit_event` с `resetsAt`, `task_*`, состояния `agents --json` | **штатно** |
| пульс для генерации модели (кроме дельт) | **нет штатно** |
| пульс внутри субагентов | **нет штатно** |
| любой сигнал «в процессе» в OTel | **нет штатно** |
| `resetsAt` в OTel | **нет штатно** |
| состояние «ждёт» при MCP prompt tool | **нет штатно** |
| сводное «жива или зависла» одним числом | **нет штатно** |

**[П]** Закрываем это адаптером из A или B.

## 7. Открытые вопросы и что не проверено

- **Детальная beta-трассировка** (спан `claude_code.hook`, `ENABLE_BETA_TRACING_DETAILED`) не включалась: она перенаправляет логи и трейсы на `BETA_TRACING_ENDPOINT`. **[НП]**
- **Workflows:** `workflow.run_id` на событиях и спанах есть только по доке; прогон workflow не делался ради экономии запросов. **[НП опытом]**
- **`OTEL_METRICS_EXPORTER=prometheus`** как сигнал «процесс жив» (scrape отвечает) не проверялся. Порт 9464 общий, на N сессий без своего порта не масштабируется. **[НП]**
- **statusline с `refreshInterval`** в неприкреплённой `--bg`-сессии. **[НП]**
- **`tool_progress` в субагентах:** по доке его нет, опытом не проверялось. **[НП опытом]**
- **`claude-agent-acp`:** пробрасывает ли heartbeat, `session_state` и `api_retry` в ACP. **[НП]**
- **Стандартные таймауты watchdog-ов** (180–300 с) — только по доке; опыт шёл с прижатым к 10 с byte watchdog.
- **`busy` при ожидании фоновой команды после хода в `-p`** — проверено (R1, 31–36 с). А `CLAUDE_CODE_BG_TASKS_REPORT_RUNNING=0` не пробовался.
- **Наследует ли сессия Gas City `OTEL_*_EXPORTER`** из env контроллера (мимо `OTELEnvMap`). **[НП]**
- **Персональные данные в OTel:** в OTel без спроса уходят `user.email`, `user.account_uuid` и `organization.id` (R1). В общем коллекторе флота это стоит фильтровать. **[П]**

Сырьё и скрипты:
- `LAB/scripts/`: `run.sh`, `run_stdio.sh`, `run_bg.sh`, `run_gcenv.sh`, `fake_api.mjs`, `perm_mcp.py`, `stdio_driver.py`, `hook.py`, `sampler.py`, `tcp_probe.py`, `analyze.py`, `posts.py`;
- `LAB/runs/{r1,r2,r3,f0..f6,g1,g2}/`, в каждой `timeline.txt`, `summary.json` и исходные jsonl;
- документация в `LAB/web/`.
