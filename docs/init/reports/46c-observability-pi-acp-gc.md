# 46c. Мониторинг сессий: pi, ACP как поток живого состояния, что умеет Gas City, бэкенд

Дата: 2026-11-11. Объекты: pi 1.5.0 (`lab/pi/node_modules/@earendil-works/*`, исходники `src/pi-mono` @ 815541a ≈ v1.5.1); ACP @ ab82539 (`src/agent-client-protocol`, схема v1 и v2 2.0.0-alpha.14); адаптеры `src/claude-agent-acp` 0.102.0, `src/codex-acp-current` 2.6.0, `src/pi-acp-svkozak` 0.0.34, `src/pi-acp-openma` 0.1.8; Gas City @ f1f75f53c. Сырьё: `lab/observability/pi-acp-gc/`.

Метки: **[П]** — проверено (код или документ с путём и строкой, эксперимент); **[В]** — вывод; **[НП]** — не проверено. Пути к коду Gas City даны от корня репозитория gascity, к коду pi — от `src/pi-mono/packages/`.

Бюджет LLM: потрачено 0 запросов. Был один прогон pi в режиме `--mode rpc` на локальном mock-провайдере.

## 1. Выводы за 60 секунд

- **У pi нет штатного OTel-экспорта** [П]. Есть только vendor-neutral контракт `@earendil-works/pi-telemetry`: spans, metrics, propagation, без exporter. `pi-ai` умеет писать в него `pi.ai.request` и метрики токенов и стоимости, но `pi-coding-agent` 1.5.0 этот контекст не передаёт. Настройка «telemetry» в pi — это пинг установки на pi.dev, к наблюдаемости отношения не имеет.
- **Расширение примерно на 110 строк закрывает OTel для pi, включая in-flight** [П, опыт E1, 0 LLM-запросов]. Прототип раз в секунду отправлял heartbeat-метрики: состояние сессии, число инструментов в работе и возраст самого старого, возраст последнего токена модели. Видно было всё: `bash sleep 6` в работе 0,5→5,5 с, зависший стрим модели 0,5→5,5 с, ошибки 429 и квоты. После `kill -9` heartbeat пропал — это единственный сигнал смерти процесса.
- **API расширений pi беднее, чем rpc-поток** [П]. Событий `auto_retry_*` и `agent_end.willRetry` расширение не видит, а `after_provider_response` при ответах 4xx/5xx не срабатывает: хук вызывается только после успешного `withResponse()` (`ai/src/api/openai-completions.ts:384-392`). Различить «будет ретрай» и «финальная ошибка» расширение может только по последующему `agent_settled`. Драйверу на rpc всё это приходит готовым.
- **Готовых OTel- и Langfuse-расширений для pi больше десятка** [П, npm]. Крупнейшие: `pi-otel` (около 20 тыс. загрузок в месяц) и официальный `@langfuse/pi-observability-plugin` (около 10 тыс., «experimental»). Все дают traces, токены и стоимость. Gauge «что выполняется прямо сейчас» в README нет ни у одного [П по README].
- **ACP v1 даёт живой поток, но без явного состояния сессии и без heartbeat** [П]. Занятость означает «есть незавершённый `session/prompt`». Статусы `tool_call`: `pending | in_progress | completed | failed`. Есть `usage_update` (stable с 2026-06-05) и `notice` (по capability). Таймаутов и пингов в протоколе нет.
- **ACP v2 (Draft с 2026-07-20) — ровно то, что спрашивает владелец** [П]. Это `state_update` со значениями `running | idle | requires_action`, список `background` (терминалы и tool calls), `stopReason:"error"` с объектом ошибки и `notice` для ретраев. Но спецификация прямо говорит: «Clients detect a lost connection or an exited Agent process themselves» и «Output is not a liveness signal». v2 уже реализуют `claude-agent-acp` (`src/v2/prompt.ts:92-111`) и `codex-acp` (`src/AcpV2ForegroundState.ts`); адаптеры pi — нет.
- **`in_progress` у долгой команды держится, но тикает только у claude** [П по коду]. claude-agent-acp пересылает heartbeat-ы SDK (`_meta.claudeCode.toolResponse.elapsedTimeSeconds`). codex-acp и pi-acp молчат до вывода или до конца. Возраст tool call драйвер должен считать сам по своим часам.
- **В ACP нет своей телеметрии** [П]. RFD «Agent Telemetry Export» (через `OTEL_*` env) удалён 2026-07-02 (PR #1581), RFD «Agent-to-Client Logging» закрыт 2026-10-26 (PR #392). Остались зарезервированные `_meta.traceparent/tracestate/baggage`, и ни один из четырёх адаптеров их не читает.
- **OTel GenAI semconv для агентов (`invoke_agent`, `execute_tool`, `gen_ai.agent.*`) целиком в статусе Development** [П]. С мая 2026 конвенции живут в отдельном репозитории без релизов, имена уже менялись (`gen_ai.client.token.usage` → `gen_ai.client.inference.usage.*`). Метрики «in-flight» в них нет. Мапить на них стоит длительности, токены и атрибуты; состояние сессии придётся делать своими метриками.
- **Находка: проброс телеметрии Claude Code в Gas City фактически мёртв** [П код + дока, В итог]. gc выставляет агентам `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_RESOURCE_ATTRIBUTES` и `BD_OTEL_*`, но не `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER`, `OTEL_EXPORTER_OTLP_PROTOCOL` и endpoint (`internal/telemetry/subprocess.go:177-191`). У Claude Code по документации экспортёры опциональны, а протокола по умолчанию нет. Для codex и pi gc не настраивает ничего.
- **В живом городе телеметрия выключена целиком** [П]. У `gc supervisor run` и у сессий claude нет ни одной переменной `GC_OTEL_*` или `OTEL_*`. Порты 8428, 9428, 4317, 4318 никто не слушает. `program_status` не задан, то есть `off`. Liveness в городе — это `idle_timeout` на 41 агенте по `tmux #{window_activity}`.
- **Gas City отвечает на вопрос «зависла ли сессия» эвристиками tmux** [П]. Это регулярки по экрану («esc to interrupt», спиннер), активность окна tmux, хвост транскрипта, OSC 7501 через `pipe-pane` и patrol раз в 30 с. Метрик состояния сессии нет: единственный gauge — `gc.bead_store.healthy`. ACP-рантайм gc статусы `tool_call`, `usage_update` и `notice` игнорирует и берёт только «время последнего update» (`internal/runtime/acp/conn.go:261-292`).
- **Бэкенд для одной машины: VictoriaMetrics + VictoriaLogs (+ Grafana)** [В]. Оба принимают OTLP напрямую, collector не обязателен. Подводные камни [П]: VM принимает OTLP только в protobuf, а delta-метрики (дефолт Claude Code) хранит «как есть». Langfuse и Phoenix подходят для разбора трасс после факта, но не для «жив ли сейчас»: span экспортируется только по завершении.

## 2. Факты

### 2.1. pi: штатная телеметрия

- **[П]** `@earendil-works/pi-telemetry` 1.5.0: «no exporter, global current-span state, or dependency on a telemetry backend… Applications can… provide an adapter for OpenTelemetry» (`lab/pi/node_modules/@earendil-works/pi-telemetry/README.md:13-15`). У span нет публичного `end()`: он закрывается вместе с callback.
- **[П]** `pi-ai` объявляет `AI_TELEMETRY_SCHEMA`: span `pi.ai.request` (provider, model, stop, usage.*, cost.total; статус error при `stop=error`), события `pi.ai.first_chunk` и `pi.ai.block`, метрики `pi.ai.request.duration`, `pi.ai.first_chunk.duration`, `pi.ai.tokens`, `pi.ai.cost` (`pi-ai/dist/telemetry.d.ts`). Включается опцией запроса `telemetry?: TelemetrySpan` (`pi-ai/dist/types.d.ts:50-51`).
- **[П]** `pi-coding-agent` этот span не передаёт: в `dist/` (не bundle) нет ни одного `telemetry:`. Сборка опций запроса — `core/sdk.js:195-209`. Слово «telemetry» в coding-agent относится только к install-пингу `https://pi.dev/api/report-install?version=…` (`coding-agent/src/modes/interactive/interactive-mode.ts:1472-1489`; `PI_TELEMETRY`, `enableInstallTelemetry`).
- **[В]** Подключить адаптер pi-telemetry → OTel можно только через SDK, встраивая pi в свой процесс и передавая span в `streamFn`. Из CLI и rpc такой точки входа нет.
- **[П]** OSC 7501 (Program Status Protocol терминала Rex) pi шлёт только в интерактивном TUI (`coding-agent/src/modes/interactive/program-status-reporter.ts:12`; `docs/terminal-setup.md:225-235`). В rpc-режиме он не нужен.

### 2.2. pi: события rpc и extension API, эксперимент E1

Что видно в каждом канале [П по `docs/json.md:50-196`, `src/core/extensions/types.ts:1643-1708` и эксперименту]:

| Сигнал | rpc / json-поток | extension API |
|---|---|---|
| ход начался / окончательно закончился | `agent_start` / `agent_settled{aborted}` | то же |
| будет ли ретрай | `agent_end{willRetry}`, `auto_retry_start{attempt,maxAttempts,delayMs,errorMessage}`, `auto_retry_end{success,finalError}` | **нет** (`AgentEndEvent` без `willRetry`, `types.ts:986-989`) |
| HTTP-статус провайдера | только текст `errorMessage:"429: …"` | `after_provider_response{status,headers}`, **только при успехе** |
| инструмент | `tool_execution_start/update/end{durationMs,isError}` | то же |
| компакция | `compaction_start/end{reason,willRetry}` | `session_before_compact` / `session_compact` / `session_compact_failed` |
| очередь | `queue_update{steering,followUp}` | `ctx.hasPendingMessages()` |
| контекст и стоимость | `get_session_stats` (команда) | `ctx.getContextUsage()` |
| ошибки расширений | `extension_error` | — |

Эксперимент E1 (`lab/observability/pi-acp-gc/e1/`). Устройство: один процесс `pi --mode rpc -e pi-otel-state.js`, mock OpenAI-совместимого провайдера (`mock-tool.mjs`), приёмник OTLP/JSON (`otlp_sink.py`). Расширение — 111 строк без npm-зависимостей: голый `fetch` в `/v1/metrics` и `/v1/logs`. Таймлайн — `out/timeline.txt`, сводка rpc — `out/rpc-summary.txt`.

| Сценарий | Что видно в OTLP (heartbeat 1 с) | Что видно в rpc |
|---|---|---|
| 429, 429, затем tool `sleep 6` | 2× лог `turn.error` с текстом `429: …`; `pi.provider.responses` считает только `200` | 2× `auto_retry_start{attempt:1/2, delayMs:300/600}` → `auto_retry_end{success:true}` |
| tool в работе | `state=tool`, `pi.tool.inflight=1`, `inflight_oldest_age_s` 0,5→5,5 c `{gen_ai.tool.name=bash}` | `tool_execution_start`, один `update` через 20 мс, **6 с тишины**, `end{durationMs:6022}` |
| 429 `insufficient_quota` | `turn.error` → `agent.settled` через 10 мс | `agent_end{willRetry:false}` → `agent_settled{aborted:false}`, без ретрая |
| стрим модели завис на 6 с | `state=model`, `model.last_delta_age_s` 0,5→5,5 | ни одного события между `message_start` и концом |
| `kill -9` во время `sleep 60` | последний heartbeat за 1 с до kill, затем тишина; `session.shutdown` не пришёл | EOF, exit по `SIGKILL` |
| после `kill -9` | — | `/bin/bash -c sleep 60; echo done` (pid 612608) жив через 5 с, убит мной по PID |

- **[П]** `after_provider_response` вызывается из `onResponse` только после `retryProviderRequest(...withResponse())` (`ai/src/api/openai-completions.ts:384-392`, `anthropic-messages.ts:943`). Ответ 4xx/5xx бросает исключение раньше.
- **[П]** `tool_execution_update` для молчащей команды не периодический. «Сколько уже идёт» считает потребитель по своему таймеру.
- **[П]** pi убивает отслеживаемых детей только по SIGHUP/SIGTERM (`coding-agent/src/utils/shell.ts:198-215`). При SIGKILL остаются сироты, что подтверждает отчёт 16.
- **[П]** Фоновых процессов «как у Claude Code» в pi нет: у `bash` нет режима background и нет таймаута по умолчанию (`src/core/tools/bash.ts:41`). Процессы, запущенные через `&`, держат пайпы (`bash.ts:96-98`, #11613). **[В]** Фон pi виден только по дереву процессов (`/proc/<pid>/task/*/children`, cgroup).
- **[П]** Ошибка провайдера в rpc — это `message_end.stopReason:"error"` со строкой `errorMessage`. Типизированного класса нет; классификатор нужен свой (см. отчёт 16).
- **[В]** Ограничения прототипа. Состояние `retry_wait` выставляется на любую ошибку и снимается только на `agent_settled`. `last_delta_age` между ходами не обнуляется. Для продукта это мелочи, а для вывода «хватит ли ~N строк» несущественны.

### 2.3. pi: готовые расширения (npm, данные на 2026-11-11)

| Пакет | Загрузок/мес | Что даёт | Источник |
|---|---|---|---|
| `pi-otel` 0.3.0 (nikiforovall) | 20 205 | traces (по умолчанию), metrics и logs опционально; `spanNaming:"genai"` → `invoke_agent pi` / `chat {model}` / `execute_tool {tool}`; `TRACEPARENT` в bash | `npm-readmes/pi-otel.md:55-79` |
| `@langfuse/pi-observability-plugin` 0.1.2 (мейнтейнеры Langfuse) | 10 107 | трасса на prompt, генерации с токенами, стоимостью и TTFT, tool calls, сабагенты; «experimental» | `npm-readmes/_langfuse_pi-observability-plugin.md` |
| `@amaster.ai/pi-telemetry` 0.1.38 | 5 352 | Langfuse, Phoenix, OTel | npm search |
| `pi-langfuse` 1.7.1 | 2 440 | Langfuse | npm search |
| `@thomas-fazzari/pi-otlp` 0.1.1 | 480 | metrics `pi.token.usage`, `pi.cost`, `gen_ai.client.operation.duration`, `pi.tool.calls/duration`; spans `invoke_agent` → `pi.turn` → `chat`/`execute_tool`; логи `pi.model.request.failed` и др. | `npm-readmes/_thomas-fazzari_pi-otlp.md:48-52` |
| `pi-codex-otel` 0.0.9 | 346 | события pi как Codex-логи `codex.*`, совместимые с codex-дашбордами | `npm-readmes/pi-codex-otel.md:74-86` |

Полный список с метаданными — `lab/observability/pi-acp-gc/pi-otel-extensions-npm.txt`. Готового gauge «что выполняется прямо сейчас» в README нет ни у одного [П]. Код пакетов я не читал и не запускал [НП].

### 2.4. ACP v1: что приходит в `session/update`

- **[П]** Варианты v1 stable: `user_message_chunk`, `agent_message_chunk`, `agent_thought_chunk`, `tool_call`, `tool_call_update`, `plan`, `available_commands_update`, `current_mode_update`, `config_option_update`, `session_info_update`, `usage_update`, `notice`, `compaction_update`, `compaction_summary_chunk`. В unstable дополнительно: `plan_update`, `plan_removed`, `subagent_update`, `session_message[_chunk]`. Источник — `schema/v1/schema*.json`, выгрузка `acp_schema_enum.py`.
- **[П]** `ToolCallStatus`: `pending` («input is either streaming or awaiting approval»), `in_progress`, `completed`, `failed` (`docs/protocol/v1/tool-calls.mdx:254-272`). Значения `StopReason`: `end_turn | max_tokens | max_turn_requests | refusal | cancelled`, значения `error` нет. Ошибка хода приходит JSON-RPC ошибкой на `session/prompt`.
- **[П]** `usage_update{used,size,cost?}` стабилизирован 2026-06-05 (`docs/announcements/session-usage-stabilized.mdx`). `notice{severity,title,description}` отправляется только при `clientCapabilities.session.notices` (`docs/protocol/v1/prompt-turn.mdx:213-248`).
- **[П]** Heartbeat нет. Единственное упоминание — в RFD session-status: «Transport-level heartbeats… only show the connection is alive» (`docs/rfds/session-status.mdx:227`). Для удалённого транспорта liveness — «up to the implementer» (`docs/rfds/streamable-http-websocket-transport.mdx:77,88`). Таймаутов в протоколе нет: их строит агент поверх `terminal/*` (`docs/protocol/v1/terminals.mdx:251`).

Поведение адаптеров. Ниже — чтение кода моим субагентом; ключевые строки я проверил выборочно.

| | claude-agent-acp 0.102 | codex-acp 2.6 | pi-acp 0.0.34 | @openma/pi-acp 0.1.8 |
|---|---|---|---|---|
| `in_progress` | только по SDK-биту `tool_progress`; старт = `pending` (`tool-calls/renderer.ts:259`, `acp-agent.ts:8687`) | сразу при `item/started` (`tool-calls/reporters/CommandReporter.ts:197`) | при `tool_execution_start` | при `tool_execution_start` |
| тикает ли долгий tool | да: повторные `_meta.claudeCode.toolResponse.elapsedTimeSeconds` | нет, только дельты вывода | нет | нет |
| busy/idle | v2 `state_update` | `_meta.codex.threadStatus` (`idle/active/systemError`), v2 `state_update` | `_meta.piAcp{queueDepth,running}` | нет |
| фон | v2: `background` (терминалы, вывод опрашивается раз в 500 мс) | только для клиента JetBrains AIR (`async_task_*`) | нет | нет |
| ретрай | только AIR | `_meta.codex.error{willRetry:true}` (`CodexEventHandler.ts:1128-1138`) | только текстом | `_meta.pi{event:"auto_retry"}` |
| финальная ошибка | -32603 + `errorKind` | текст + `end_turn` (usage limit → -32603) | проглатывается → `end_turn` (`acp/agent.ts:887-893`) | -32603 + `_meta.pi.failure` |
| смерть харнесса в ходе | ход failed, сессия выселена | ход failed (1001), app-server перезапускается | `session/prompt` висит | в одном процессе |
| OTel | env наследуется, traceparent не читается | то же | то же | нет |

### 2.5. ACP v2 (Draft): явное состояние сессии

- **[П]** Статус: v2 опубликован как Draft 2026-07-20 (`docs/updates.mdx:50-54`), схема `2.0.0-alpha.14` от 2026-11-09 (`schema/v2/CHANGELOG.md`). Фоновую работу добавили 2026-10-31 (alpha.12, #2738).
- **[П]** `state_update`: `running | idle | requires_action`. Агент **MUST** сообщить `running` при старте foreground-работы и `idle` с `stopReason` в конце (`docs/protocol/v2/prompt-lifecycle.mdx:66-96`). Сбой: `idle` + `stopReason:"error"` + `error{code,message}` (`:852-886`). Восстановимые сбои (ретрай запроса к модели, упавший инструмент) идут через `notice` и `failed` у tool call и состояние не меняют (`:888`).
- **[П]** `background`: снимок-список `{type:"terminal",terminalId}` и `{type:"tool_call",toolCallId}`. `[]` значит «ничего не идёт», а пропуск поля — «неизвестно» (`:98-134`). Цитата: «Output is not a liveness signal: a Client MUST NOT treat a terminal as hung or exited because its output is missing or stale» (`:102`).
- **[П]** «Clients detect a lost connection or an exited Agent process themselves. No update can report them» (`:892`).
- **[П]** RFD `session/status` (Draft, unstable): `open | saved`, в v2 плюс снимок `state` (`docs/rfds/session-status.mdx:96-124`). Запрос без побочных эффектов, его можно дёргать как дешёвый probe.
- **[П]** v2 реализован в claude-agent-acp (`src/v2/prompt.ts:92-111`, согласование в `src/v2/initialize.ts:85`) и codex-acp (`src/AcpV2ForegroundState.ts`, `CodexAcpServer.ts:610`). В адаптерах pi `state_update` нет (grep).

### 2.6. ACP и телеметрия

- **[П]** `_meta` резервирует `traceparent`, `tracestate` и `baggage` под W3C Trace Context (`docs/protocol/v1/extensibility.mdx:37-39`; v2 — `:94-96`). Субагент по grep не нашёл, чтобы его читал хоть один из четырёх адаптеров.
- **[П]** RFD «Agent Telemetry Export» (PR #298, 2025-12): клиент поднимает OTLP-приёмник и инжектит `OTEL_EXPORTER_*` в агент. Удалён 2026-07-02: «doesn't translate to the new remote transports… clients and agents… can adopt standard OpenTelemetry SDK environment variable configuration directly» (`docs/rfds/updates.mdx:233-237`, PR #1581). Реализации так и не появилось, в комментарии #298 автор пишет «not yet».
- **[П]** RFD «Agent-to-Client Logging» (`log`-нотификация, PR #392) закрыт 2026-10-26: «haven't heard a lot of demand» (`lab/.../gh-acp/pr-392.json`).
- **[В]** Канал телеметрии в ACP — это сам поток `session/update` плюс обычные `OTEL_*` env для процесса агента. Отдельного стандарта нет и не планируется.

### 2.7. OTel GenAI semconv для агентов

- **[П]** Репозиторий `open-telemetry/semantic-conventions-genai` создан 2026-05-05, релизов нет. Все документы — agent spans, client invoke_agent, execute-tool, metrics, token metrics, mcp — помечены `**Status**: Development` (`web-semconv/*.md:7`).
- **[П]** Spans: `create_agent`, `invoke_agent {gen_ai.agent.name}` (client и internal), `plan`, `execute_tool {gen_ai.tool.name}`. Атрибуты: `gen_ai.agent.id/name/version`, `gen_ai.conversation.id`, `gen_ai.tool.call.id`, `error.type` (stable).
- **[П]** Метрики: `gen_ai.client.operation.duration`, `gen_ai.invoke_agent.duration`, `gen_ai.invoke_agent.inference_calls`, `gen_ai.invoke_agent.tool_calls`, `gen_ai.execute_tool.duration`, `gen_ai.client.inference.usage.{input,output,cache_read.input,cache_write.input,reasoning.output}_tokens`. Прежнего `gen_ai.client.token.usage` в новом репо нет. MCP: `mcp.client.operation.duration`, `mcp.client.session.duration`.
- **[П]** UpDownCounter-ов и gauge «active/in-flight» нет (grep по `gen-ai-metrics.md`).
- **[В]** Spans экспортируются по завершении, поэтому зависший `execute_tool` в трассах не виден, пока не закончится. Traces отвечают на «что было», а на «что сейчас» отвечают gauges и heartbeat.

### 2.8. Gas City: что уже есть

**Телеметрия gc** (`internal/telemetry/`, 3145 строк вместе с тестами):

- **[П] Включение.** Только по opt-in. Нужен `GC_OTEL_METRICS_URL` или `GC_OTEL_LOGS_URL`, иначе используется `OTEL_EXPORTER_OTLP_ENDPOINT`; `OTEL_SDK_DISABLED=true` выключает (`telemetry.go:146-168`).
- **[П] Адреса и протокол.** Дефолты VM-специфичные: `http://localhost:8428/opentelemetry/api/v1/push` и `http://localhost:9428/insert/opentelemetry/v1/logs` (`:79-82`). Экспорт метрик раз в 30 с, OTLP/HTTP.
- **[П] Ресурс.** Атрибуты: `gc.agent`, `gc.rig`, `gc.city` и уникальный `service.instance.id`, без которого кумулятивные счётчики разных процессов gc сливаются в одну серию (`telemetry.go:170-214`, `subprocess.go:11-30`).
- **[П] Метрики.** 32 инструмента, все про события controller. Lifecycle: `gc.agent.{starts,stops,crashes,quarantines,idle_kills,max_age_kills}.total`, `gc.reconcile.cycles.total`, `gc.session.nudges.total`, `gc.session.turn_interrupts.total`, `gc.sling.dispatches.total`, `gc.pool.*`, `gc.mail.operations.total`, `gc.drain.transitions.total`, `gc.bd.calls.total`/`duration_ms`, `gc.http.*`, `gc.nudge_queue.lock.*` (`recorder.go:108-212`). Из транскриптов считаются `gc.agent.tokens.{input,output,cache_read,cache_creation}` и `gc.agent.invocation.cost_usd` (`recorder_invocation.go:42-70`, вызов в `internal/worker/invocation_telemetry.go:208`). `gc.agent.invocation.latency_ms` объявлен, но production-вызова нет. Gauge один — `gc.bead_store.healthy`.
- **[П] Логи.** 22 события: `agent.start/stop/crash/quarantine/idle_kill/max_age_kill`, `reconcile.cycle`, `session.nudge`, `sling.dispatch`, `bd.call`, `bd.slow`, `pool.*`, `drain.transition`, `session.turn_interrupt` и др. (`recorder.go:314-778`).
- **[П] Roadmap.** Phase 3 — «observable gauges for running agent counts» — не сделана (`engdocs/archive/backlogs/telemetry-roadmap.md:70-71`). `RegisterCallback` и observable gauges в коде нет. `gt.agent.state_changes.total` отложен: «gc uses beads not states» (`:25`).

**Проброс агентам и находка:**

- **[П]** В env каждой сессии попадают `OTELEnvMap()`: `BD_OTEL_METRICS_URL`, `BD_OTEL_LOGS_URL`, `CLAUDE_CODE_ENABLE_TELEMETRY=1` и `OTEL_RESOURCE_ATTRIBUTES` (`subprocess.go:177-191` → `internal/processenv/provider.go:253`). Стандартные `OTEL_EXPORTER_OTLP_ENDPOINT` и `OTEL_SDK_DISABLED` намеренно не пробрасываются: «nested sessions see those only through ambient inheritance» (`telemetry.go:21-28`).
- **[П]** Документация Claude Code (`web-cc-docs/monitoring-usage.md`):
  - `CLAUDE_CODE_ENABLE_TELEMETRY` — «(required)»;
  - выбор экспортёров «both are optional — configure only what you need» (`:23-25`);
  - «Claude Code has no default protocol, so set this… for each `otlp` exporter you enable» (`:113`);
  - temporality по умолчанию `delta` (`:131`).
- **[В]** Итог: при включённой телеметрии gc агент получает «включи телеметрию», но не получает ни экспортёра, ни endpoint, ни протокола, а адрес `BD_OTEL_*` Claude Code не читает. Сам по себе проброс ничего не шлёт. Чтобы заработало, нужны `OTEL_METRICS_EXPORTER=otlp`, `OTEL_LOGS_EXPORTER=otlp`, `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`, endpoint и `…TEMPORALITY_PREFERENCE=cumulative` из окружения супервизора. Сам Claude Code в этом режиме я не запускал: вывод по документации [НП эмпирически].
- **[П]** Для codex (`[otel]` в `config.toml`) и pi gc ничего не настраивает: grep `otel` в `internal/worker`, `internal/materialize`, `internal/runtime`, `cmd/gc` нашёл только комментарии.

**Живой город** (`lab/.../live-city-env.txt`, `listening-ports.txt`, `live-config-explain.txt`):

- **[П]** У `gc supervisor run` (pid 574080) нет ни одной переменной `OTEL_*`, `GC_OTEL_*`, `BD_OTEL_*` или `CLAUDE_CODE_ENABLE_TELEMETRY`. У двух сессий claude из `OTEL`-подобного есть только `GC_AGENT`, `GC_ALIAS`, `GC_CITY`. LISTEN-сокетов на 8428, 9428, 4317, 4318, 9090, 3000, 3100 и 6006 нет (`/proc/net/tcp*`). Утилиты `ss` на машине нет.
- **[П]** `program_status` в `city.toml` не задан, значит `off` (`internal/config/config.go:1790-1795`). Из настроек liveness реально используется только `idle_timeout`: у 41 агента, от 15m до 2h.

**Как gc решает «жива или зависла»** (свод субагента; выделенные строки я проверил):

- **[П] program_status (OSC 7501).** Состояния `idle | working | done | blocked{permission,question,auth} | error` (`internal/runtime/program_status.go:14-29`). Читается через tap на tmux `pipe-pane` в опции панели `@gc_ps` (`internal/runtime/programstatus/tap/publisher.go:17-19`). Залипшее значение держится не дольше 10 мин (`program_status.go:123`). Без tmux механизм неприменим.
- **[П] Без OSC.** Регулярки по экрану: `esc to interrupt`, `Press Esc or Ctrl+C to cancel`, спиннеры claude (`internal/runtime/tmux/tmux.go:6206-6216`). `WaitForIdle` требует 2 подряд idle-опроса с шагом 200 мс. Готовность определяется по `ReadyPromptPrefix` или `ReadyDelayMs`. Последняя активность — `#{window_activity}` минус собственные нажатия gc (`tmux.go:4756-4808`). Хвост транскрипта используется для фазы worker.
- **[П] Patrol.** Это reconcile-тик `[daemon] patrol_interval`, по умолчанию 30 с. Настройки `idle_timeout`, `max_session_age`, `progress_stall_timeout` и `claim_holder_stall_timeout` по умолчанию выключены. Есть work ladder (grace 90 с, backoff от 3 мин до 60 мин) и crash loop `max_restarts=5/1h` → quarantine.
- **[П] 429 и квота.** Скрейпинг последних 120 строк мёртвой панели (`internal/runtime/dialog.go:1946-2012`). Дальше `TurnFailure.RateLimited/Quota` из транскриптов (`internal/sessionlog/turns.go:161-198`) и quarantine на 30 мин.
- **[П] `gc trace`.** Пишет только решения reconciler (`cycle_*`, `decision`, `operation`, `mutation`…) в `.gc/runtime/session-reconciler-trace/`. На каждую сессию там лишь `state` и `sleep_reason`, активности нет.
- **[П] ACP-рантайм gc.** `handleUpdate` ставит `markActivity(now)` и складывает текст и заголовки tool в псевдо-экран. `status`, `usage_update`, `plan`, `notice` и `state_update` не разбираются (`internal/runtime/acp/conn.go:261-292`). Активность пишется в sidecar не чаще раза в 5 с (`activity_publisher.go`).

### 2.9. Бэкенд (оценка по документации, ничего не ставил)

| Стек | Компоненты | Сигналы | За | Против |
|---|---|---|---|---|
| **VictoriaMetrics + VictoriaLogs** (+ Grafana или vmui) | 2 бинарника (+1) | metrics, logs; traces — отдельный VictoriaTraces | OTLP напрямую, без collector; дефолт gc; есть готовые дашборды | OTLP только protobuf; delta хранится как есть, «works best with cumulative» [П, docs.victoriametrics.com, integrations/opentelemetry] |
| otel-collector + Prometheus + Loki + Tempo + Grafana | 5 | все три | стандарт, `deltatocumulative` в collector | больше движущихся частей; в Prometheus метрики приходят через collector |
| `grafana/docker-otel-lgtm` | 1 контейнер | все три | быстрый старт | Docker; «для dev» [НП] |
| Langfuse | web + worker + Postgres + ClickHouse + Redis + S3 | только traces (OTLP HTTP, `/api/public/otel`) | GenAI semconv, разбор сессий, стоимости | тяжело; liveness не показывает [П langfuse.com/self-hosting, …/opentelemetry] |
| Phoenix | контейнер `arizephoenix/phoenix` | traces | легче Langfuse | то же; детали хранилища не проверял [НП] |

- **[П]** VictoriaMetrics принимает и `/opentelemetry/api/v1/push` (дефолт gc), и `/opentelemetry/v1/metrics` (`app/vminsert/main.go:235`, через `gh api`).
- **[П]** Блог VictoriaMetrics (2026-01-09) настраивает Claude Code на VM так: `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf` и `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=cumulative` («VictoriaMetrics does not support delta»). Codex там экспортирует только logs и traces.
- **[П] Дашборды.**
  - `anthropics/claude-code-monitoring-guide`: последний push 2025-07-29, стек otel-collector + Prometheus + Grafana с `:latest`, то есть устарел на 15 месяцев.
  - Grafana.com: 25255 «Claude Code Metrics (Prometheus)», 25645 «AI CLI Tools — Overview» (VM + VictoriaLogs), 24202 «Codex».
  - `VictoriaMetrics-Community/vibe-coding-dashboards`: последний push 2026-01-06, 9★, есть Claude Code, Codex, Gemini, Qwen.
  - **[В]** Все они про usage и стоимость, а не про «сессия X прямо сейчас в tool 14 минут».
  - Ссылки: https://grafana.com/grafana/dashboards/25255-claude-code-metrics-prometheus/, https://grafana.com/grafana/dashboards/25645-ai-cli-tools-overview/, https://victoriametrics.com/blog/vibe-coding-observability/

## 3. Вердикты по гипотезам (в части наблюдаемости)

- **H1 (без tmux всё решается) — подтверждена для мониторинга.** Все сигналы, которые gc выжимает из tmux (busy, idle, blocked, rate limit), в rpc pi и в ACP v2 приходят структурно [П]. Остаются две оговорки. Смерть процесса и сироты протокол не сообщает: драйвер видит их через `waitpid` или pidfd и дерево процессов [П E1]. Классификацию квоты и 429 по тексту нужно делать самому [П].
- **H2 (ACP в gc плох из-за gc) — для наблюдаемости подтверждена.** gc выбрасывает из ACP-потока статусы tool, usage, notice и v2-state и оставляет только время последнего update [П `conn.go:261-292`]. Протокол v1 при этом действительно беднее, чем нужно: явного состояния сессии нет, а «долгий tool жив» тикает только у claude [П].
- **H3 (ACP достаточно для всех трёх) — частично.** Для claude и codex ACP v2 даёт почти полное живое состояние [П по коду]. Для pi адаптеры v1-only, у svkozak ошибки проглатываются и ход висит при смерти pi [П]. Значит, pi через rpc и синтез тех же состояний в драйвере [В].
- **H8 (тики → события) — подтверждена для мониторинга.** Patrol в gc идёт раз в 30 с, экспорт метрик тоже раз в 30 с. Живое состояние приходит событиями через `session/update` и rpc, а метрики — это лишь периодический снимок этих событий [В].
- **H11 — в части наблюдаемости переносить из gc почти нечего.** Подробности в разделе 4.

## 4. Следствия для дизайна

**Главное [В]: источник правды о живом состоянии — драйвер, а не OTel харнесса.** Драйвер и так держит поток каждой сессии: ACP для claude и codex, rpc для pi. Отсюда state machine на сессию и экспорт OTLP из одного процесса. Настраивать OTel в трёх харнессах для этого не нужно.

Модель состояния на сессию [В], выровненная по ACP v2:

| Поле | claude/codex (ACP) | pi (rpc) | уровень процесса |
|---|---|---|---|
| `state`: `starting/idle/running/requires_action/stopping/dead` | v2 `state_update`; v1: незакрытый `session/prompt`, `_meta.codex.threadStatus` | `agent_start` → running, `agent_settled` → idle, extension-UI запрос → requires_action | `dead` по exit или pidfd |
| `running.sub`: `model/tool/retry_wait/compacting` | `tool_call` статусы; `notice`; `_meta.codex.error.willRetry` | `tool_execution_*`, `auto_retry_*`, `compaction_*` | — |
| `tools_inflight[]{id,name,started_at}` | `tool_call` до финального статуса | `tool_execution_start/end` | дети процесса |
| `background[]` | v2 `background` | нет, только дерево процессов | `/proc/*/children`, cgroup |
| `last_event_at`, `last_token_at` | любой update / `agent_message_chunk` | любое событие / `message_update` | — |
| `last_error{class,code,text}` | JSON-RPC error, `errorKind`, `idle+error` | `stopReason:"error"` + свой классификатор | exit code, signal |
| `usage{used,size,cost}` | `usage_update` | `get_session_stats` | RSS, CPU |

**Метрики** (OTLP push раз в 5–15 с; heartbeat = сам push) [В]:

- свои, потому что в semconv их нет:
  - `agent.session.state{state}` (one-hot gauge), `agent.session.state_age_s`;
  - `agent.tool.inflight`, `agent.tool.inflight_oldest_age_s{gen_ai.tool.name}`;
  - `agent.session.last_event_age_s`, `agent.model.last_token_age_s`;
  - `agent.background.count`, `agent.process.children`;
  - `agent.provider.errors{class=rate_limit|quota|auth|overloaded|other}`, `agent.retries`;
- по semconv, с пином версии, потому что Development: `gen_ai.execute_tool.duration`, `gen_ai.invoke_agent.duration`, `gen_ai.client.inference.usage.*`;
- атрибуты: `gen_ai.agent.name`, `gen_ai.conversation.id` (= id сессии), `gen_ai.provider.name`, `error.type`, плюс свои `rig`, `city`, `harness`.

**Логи:** каждый переход состояния — событие (VictoriaLogs), из них строятся таймлайны. **Traces** — опционально, для разбора после факта.

**«Зависла» — это правило в конфиге, а не код** [В]. Примеры:

- `inflight_oldest_age_s > X` для данного инструмента;
- `last_event_age_s > Y` при `state=running` и без tool в работе (зависший стрим);
- `requires_action > Z`;
- нет heartbeat N интервалов (процесс драйвера или канал мёртв).

ACP v2 сам запрещает считать молчание вывода признаком зависания. Значит, пороги должны быть на конкретный инструмент и настраиваемые.

**OTel харнессов — как обогащение, по желанию** [В]. Claude Code — `claude_code.*` с полным набором env (см. 2.8). Codex — `[otel]`, только logs и traces. pi — `pi-otel` или `pi-codex-otel`, если нужны общие дашборды. Это даёт токены, стоимость и детали API, но не нужно, чтобы видеть «жива ли».

**Варианты:**

1. **Рекомендую: драйвер экспортирует состояние, бэкенд VM + VL + Grafana.** Один источник, единая модель для трёх харнессов, OTel-настройка харнессов не нужна. Цена — свой экспорт примерно на 200–400 строк в драйвере [В, по объёму прототипа E1].
2. **Только OTel харнессов плюс готовые дашборды.** Видно usage и стоимость. In-flight и «зависла» не видно; у pi штатного экспорта нет; три разных словаря имён.
3. **Langfuse или Phoenix как основной инструмент.** Хорош для разбора сессий и стоимости, но живое состояние не показывает, а инфраструктура тяжёлая (Langfuse). Разумен как дополнение к варианту 1.

**Что перенести из gc и что выбросить:**

| Перенести | Выбросить |
|---|---|
| opt-in через стандартные `OTEL_*` и `OTEL_SDK_DISABLED` | VM-специфичные дефолтные URL и легаси `BD_OTEL_*` с трансляцией |
| уникальный `service.instance.id` на процесс (`telemetry.go:178-184`) | «половинчатый» проброс `CLAUDE_CODE_ENABLE_TELEMETRY` без экспортёров |
| ресурсные атрибуты agent/rig/city (как `gen_ai.agent.name` + свои) | скрейпинг экрана, `window_activity`, OSC 7501 tap, `WaitForIdle`, `ReadyDelayMs` |
| словарь состояний program_status → маппинг на `running/idle/requires_action` + `error` | activity sidecar-файлы и «время последнего update» как суррогат liveness |
| lifecycle-счётчики: starts, stops, crashes, quarantines, idle_kills | tmux-скрейпинг лимитов (`dialog.go`) — заменить структурными событиями и классификатором текста |
| идея work ladder (rate limited → пауза → рестарт) как настраиваемая политика | `gc trace` в нынешнем виде (решения reconciler, без liveness) |
| токены и стоимость, но из `usage_update` и `get_session_stats`, а не из парсинга транскриптов | |

## 5. Открытые вопросы и что не проверено

- Claude Code с env от gc не запускал. Вывод «проброс мёртв» сделан по коду gc и документации Claude Code, эмпирически не подтверждён [НП].
- ACP v2 `state_update` и `background` у claude-agent-acp и codex-acp вживую не проверял, только по коду. Нужен один прогон с v2-клиентом (стоит LLM-запросов).
- Частота heartbeat-ов `tool_progress` в Claude Agent SDK неизвестна [НП].
- Код community-расширений pi не читал и не запускал; их нагрузку на процесс pi не мерил.
- Потребление памяти VM, VL, Grafana, Langfuse и Phoenix на одной машине не мерил [НП].
- `@earendil-works/pi-durable` с `DURABLE_TELEMETRY_SCHEMA` не смотрел. Это потенциальный штатный путь телеметрии pi в будущем.
- Можно ли драйверу объявить себя клиентом JetBrains AIR и получить от адаптеров типизированные отказы и `async_task_*` — не проверял [В, идея].
- pi 1.5.2 вышла 2026-11-10 (npm), исследовался 1.5.0 и исходники ≈ 1.5.1.

Побочные эффекты: все процессы эксперимента (sink, mock, pi, осиротевший `bash`) остановлены по PID. `~/.pi` не тронут (`PI_CODING_AGENT_DIR=lab/.../e1/agent-dir`). Вне `RD/` ничего не менялось.
