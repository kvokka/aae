# 46 — Мониторинг сессий claude / codex / pi: OTel, живое состояние, «жива или зависла»

Дата: 2026-11-11. Сводка трёх разборов с опытами:
- `46a-observability-claude.md` — Claude Code 2.1.322: 3 прогона на haiku и 11 на фейковом API;
- `46b-observability-codex.md` — codex 0.166.0: 5 прогонов, исходники `rust-v0.166.0`;
- `46c-observability-pi-acp-gc.md` — pi 1.5.0, ACP v1/v2, код Gas City, бэкенды.

Сырьё лежит в `lab/observability/`. Общий приёмник OTLP — `lab/observability/sink/otlp_sink.py`.

Метки: **[П]** — проверено опытом или по коду/доке со строкой; **[В]** — вывод; **[НП]** — не проверено.

## 0. Ответ за минуту

1. **OTel у всех трёх харнессов отвечает на вопрос «что было», а не «что сейчас»** **[П]**. События уходят по завершении
   действия, спаны — на end, счётчики — только при активности: delta-метрики без событий не экспортируются вовсе.
   Опыты:
   - claude: tool на 65 с — 61–66 с без единого POST при интервалах 1 с;
   - codex: `sleep 45` — 26 с тишины, ожидание approval — 25 с, зависание до заголовков — 40 с;
   - pi: штатного экспорта нет вообще.

   **Долгая команда, ожидание разрешения, зависший стрим и мёртвый процесс в OTel выглядят одинаково — как тишина.**
2. **Живое состояние у харнессов есть, но в их собственных потоках, а не в OTel** **[П]**:
   - claude: stream-json/SDK — `tool_progress` heartbeat раз в 30 с, `session_state_changed`, `api_retry`,
     `rate_limit_event{resetsAt}`; для `--bg` и интерактивных сессий — `claude agents --json`; плюс хуки;
   - codex: app-server — `thread/status/changed` (`idle|active[waitingOnApproval|waitingOnUserInput]|systemError`),
     `item/started inProgress`, `error{willRetry,codexErrorInfo}`, `account/rateLimits/updated{resetsAt}`;
   - pi: rpc — `agent_start/settled`, `tool_execution_*`, `auto_retry_*`, `agent_end{willRetry}`;
   - ACP v2 (Draft) прямо стандартизует то, что нужно: `state_update running|idle|requires_action`, `background`,
     `stopReason:"error"`. Реализован в claude-agent-acp и codex-acp, в адаптерах pi — нет.
3. **Штатного сигнала «зависла» нет ни у кого** **[П]**. «Зависла» — это *отсутствие* события. Его ловит только таймер:
   `now − last_signal > budget(state)`. Собственные watchdog-и харнессов срабатывают поздно:
   - claude — через 180–300 с;
   - codex — через 300 с × до 5 повторов, а до заголовков ответа таймаута нет совсем;
   - pi — таймаута нет.
4. **Рецепт** **[В]**: источник правды — драйвер, а не OTel харнессов. Драйвер и так читает поток каждой сессии. Он
   ведёт автомат состояний на сессию и **сам** экспортирует OTel observable gauges. Gauge с callback уходит на каждом
   интервале независимо от активности, поэтому **сам экспорт и есть heartbeat**. Бэкенд — VictoriaMetrics +
   VictoriaLogs + Grafana. Алерты — правила в конфиге. Своего кода ≈ 300–500 строк на три харнесса.
   OTel харнессов включается как обогащение: токены, стоимость, длительности, ошибки API.
5. **В Gas City сейчас этого нет** **[П]**:
   - проброс телеметрии Claude Code мёртв: gc даёт `CLAUDE_CODE_ENABLE_TELEMETRY=1` без `OTEL_*_EXPORTER`, и Claude
     создаёт 0 экспортёров (опыт G1, G2);
   - в живом городе телеметрия выключена целиком;
   - живость определяется скрейпингом tmux и `idle_timeout`;
   - ACP-рантайм gc выбрасывает статусы tool и берёт только время последнего update.

## 1. Почему OTel харнессов не отвечает на «жива ли сейчас»

| | Claude Code | codex | pi |
|---|---|---|---|
| Штатный OTel | metrics + logs; traces в beta | logs (`exporter`), traces, metrics | **нет**: только контракт `pi-telemetry` без exporter; есть community-расширения `pi-otel` и `@langfuse/pi-observability-plugin` |
| Включение | `CLAUDE_CODE_ENABLE_TELEMETRY=1` **и** `OTEL_{METRICS,LOGS,TRACES}_EXPORTER`, протокол явно | `[otel]` в `config.toml` (`otlp-http` `binary\|json`, `otlp-grpc`) | расширение `-e` |
| Маркер «началось» | `tool_decision` до исполнения tool; `hook_execution_start`, если хук зарегистрирован | `tool_decision`, только для auto-approved | — |
| Долгий tool в OTel | тишина до `tool_result` | тишина до yield (≤30 с) или конца | — |
| Ожидание разрешения | тишина; спан `blocked_on_user` приходит задним числом | тишина | — |
| Ретраи API | только финальный `api_error`; удачные ретраи не видны | каждая HTTP-попытка как `api_request{attempt}` | — |
| Лимит | `api_error 429` без `resetsAt` | `429` без типа (usage или rate) и без `resetsAt` | — |
| Ошибка команды | `tool_result success=false` | **`success=true` при exit 2** | — |
| Id сессии | `session.id` на всём; субагент — `agent_id`/`parent_agent_id`; workflow — `workflow.run_id` | `conversation.id` только в logs/spans, **в метриках нет** | — |
| Свои метки (`OTEL_RESOURCE_ATTRIBUTES`) | на каждом event, datapoint и span, включая субагентов и `--bg` | в resource, метка на процесс, не на тред | — |
| Сюрпризы | без спроса уходят `user.email`, `account_uuid`, `organization.id` | **по умолчанию метрики уходят в OpenAI** (`statsig` → `ab.chatgpt.com`), если не `[analytics] enabled=false`; у владельца выключено | `PI_TELEMETRY` — пинг установки на pi.dev |

Все ячейки **[П]** (46a §2–3, 46b §1–2, 46c §2.1–2.3).

**Вывод** **[В]**. OTel харнессов годится для дашбордов задним числом: стоимость, токены, TTFT, частота ошибок API,
длительности инструментов. Для «какая сессия и как живёт в моменте» он не годится в принципе: это свойство самой
модели экспорта, а не недоработка. GenAI semconv (`invoke_agent`, `execute_tool`) тоже не помогает: она целиком
в статусе Development, а gauge «in-flight» в ней нет (46c §2.7).

## 2. Где живое состояние есть: состояние → сигнал

Лучший штатный сигнал на каждый харнесс; задержки измерены в опытах.

| Состояние | claude | codex | pi | Без протокола (любой харнесс) |
|---|---|---|---|---|
| **Думает / генерирует** | stream-json: `status{requesting}`, затем дельты примерно раз в секунду | app-server: `active[]`, `*/delta`; hooks `PreModelRequest` без `Post…` | `message_update` | — |
| **В tool call** | `PreToolUse` (T0), `tool_progress` раз в 30 с (только главный тред) | `item/started inProgress`; `PreToolUse` с `tool_use_id` | `tool_execution_start`; пульса нет | дети процесса; у codex в env детей `CODEX_THREAD_ID` и `CODEX_TOOL_CALL_ID` |
| **Ждёт разрешения** | `agents --json` → `blocked/waiting/"permission prompt"` ≤2 с; SDK `requires_action`; хук `PermissionRequest`. **Дыра:** при MCP prompt tool состояние остаётся `busy/running` | `active[waitingOnApproval]` + server request; `server/diagnostics app.server_requests.pending` | extension-UI запрос | — |
| **Ждёт ввода / ход окончен** | `result` + `idle`; `agents --json` `done` или `"input needed"` | `turn/completed` + `idle`; `waitingOnUserInput` | `agent_settled` | — |
| **Лимит** | `rate_limit_event{resetsAt,utilization}`, `StopFailure{rate_limit}`. С `RETRY_WATCHDOG=1` ждёт молча: один `api_retry` с `retry_delay_ms≈1 ч` | `error{usageLimitExceeded}` + `rateLimits{usedPercent,resetsAt}` | `auto_retry_*`; квоту различать по тексту | — |
| **Сыплет ошибками** | `api_retry` на каждую попытку; `PostToolUseFailure` | `error{willRetry}`; HTTP-попытки видны только в OTel | `auto_retry_start{attempt}` | — |
| **Фон идёт** | `task_started`, `background_tasks_changed`, `task_notification`; хука на завершение нет | `item/started` без `completed` после хода, `thread/backgroundTerminals/list`; статус треда при этом **`idle`** | режима фона нет; только дерево процессов | `/proc/*/children`, cgroup |
| **Зависла** | прямого сигнала нет; свой watchdog 180–300 с | нет; idle timeout 300 с × ≤5 повторов; **до заголовков без ограничения** | нет | только таймер тишины |
| **Мёртв** | `agents --json` убирает запись. **`sessions/<pid>.json` остаётся `busy`**: у владельца 8 таких мёртвых файлов | EOF stdio | EOF; дочерний `bash` остаётся сиротой | `waitpid`/pidfd + `procStart` |

Все ячейки **[П]**, кроме codex `waitingOnUserInput` и codex hook `PermissionRequest` — они **[НП]** вживую.

**ACP как единый поток** **[П по коду]**:
- v1 не сообщает состояние сессии явно: «занята» означает «висит незавершённый `session/prompt`»;
- долгий tool «тикает» только у claude-agent-acp (`elapsedTimeSeconds`); codex-acp и pi молчат до вывода;
- v2 добавляет `state_update` и `background`. Спецификация прямо говорит: «Output is not a liveness signal» и
  «Clients detect … an exited Agent process themselves».

Значит, даже с ACP v2 таймер тишины и наблюдение за процессом остаются на стороне драйвера.

## 3. Рекомендуемая схема

```
харнесс ──поток──▶ драйвер (один процесс на город)
  claude: stream-json / SDK            ┌─ автомат на сессию:
          (или ACP v2 через адаптер)   │   state ∈ starting|idle|model|tool|awaiting_permission|
  codex:  app-server JSON-RPC          │           awaiting_input|retrying|rate_limited|compacting|dead
          (или ACP v2 через codex-acp) │   tools_inflight[{id,name,since}], background[], last_signal_ts,
  pi:     rpc                          │   last_error{class}, resets_at, usage
  процесс: pidfd/waitpid, /proc ──────▶└─ OTel SDK драйвера: observable gauges каждые 5–15 с
                                              │  (сам экспорт = heartbeat драйвера)
харнесс ──OTLP (по желанию)───────────────────┤
                                              ▼
                             VictoriaMetrics + VictoriaLogs (+ Grafana)  ── алерты-правила
```

**Метрики драйвера.** Свои, потому что в semconv их нет. Метки — `session.id` (= `gen_ai.conversation.id`),
`agent`, `rig`, `harness`.

| Метрика | Тип | Зачем |
|---|---|---|
| `agent.session.state{state}` | one-hot gauge | что сессия делает сейчас |
| `agent.session.state_age_seconds` | gauge | сколько в этом состоянии |
| `agent.session.last_signal_age_seconds` | gauge | тишина: основа правила «зависла» |
| `agent.tool.inflight`, `agent.tool.inflight_oldest_age_seconds{tool}` | gauge | что выполняется и как долго |
| `agent.background.count` | gauge | фоновые процессы |
| `agent.session.up` | gauge | pid жив и `procStart` совпадает |
| `agent.ratelimit.resets_at{window}`, `agent.ratelimit.used_ratio` | gauge | парковка до сброса |
| `agent.provider.errors{class=rate_limit\|quota\|auth\|overloaded\|other, will_retry}` | counter | частота ошибок |
| `agent.session.transitions{from,to}` | counter + лог-событие | таймлайны в VictoriaLogs |

Длительности и токены можно дополнительно мапить на GenAI semconv (`gen_ai.execute_tool.duration`,
`gen_ai.client.inference.usage.*`) с пином версии **[В]**.

**Правила алертов.** Это конфиг, а не код. Пороги — на инструмент и на харнесс. **[В]**

| Правило | Условие | Почему такой порог |
|---|---|---|
| висит модель | `state∈{model}` ∧ `last_signal_age > 120 с` | дельты идут примерно раз в секунду; watchdog claude — 180–300 с, codex — до 30 мин |
| висит tool (claude, главный тред) | `state=tool` ∧ `last_signal_age > 45 с` | `tool_progress` приходит раз в 30 с |
| tool дольше бюджета | `inflight_oldest_age > budget(tool)` | у codex и pi пульса нет; бюджет задаётся per tool |
| ждёт человека | `awaiting_permission > N мин` | не «зависла», а «позови человека» |
| лимит | `rate_limited` | не алерт: «спит до `resets_at`» |
| ошибки | `rate(provider.errors[5m]) > X` | — |
| мертва | `up == 0` без штатного `exited` | — |
| слеп сам мониторинг | нет экспорта драйвера N интервалов | драйвер или канал мёртв |

**Бюджет своего кода** **[В]** (по прототипам из опытов):

| Часть | Строк |
|---|---|
| редьюсер codex app-server → состояние (прототип `lab/observability/codex/scripts/state_reducer.py`, проверен на записях E2/E3) | 89 |
| экспорт pi расширением (прототип E1, без npm-зависимостей) | 111 |
| редьюсер claude stream-json | ≈ 80–120 |
| OTel gauges + правило тишины | ≈ 40–60 на Python, 100–150 на Go |
| скан `/proc` (`CODEX_THREAD_ID`, дети, сироты) | ≈ 40 |
| **Итого на три харнесса** | **≈ 300–500** |

## 4. Если флот на штатном Claude Code (вариант из отчёта 45)

- **Сессии через `claude -p` из внешнего цикла `city-tick`.** Цикл и так читает stream-json, ради `resetsAt`. В него
  добавляются автомат из §3 и gauges, ≈ 200–300 строк **[В]**. Сигналы:
  - `tool_progress` раз в 30 с;
  - `session_state_changed` (opt-in `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1`);
  - `api_retry`;
  - `rate_limit_event`;
  - `task_*` для фона;
  - `subagent_*`.

  Внутри субагентов пульса нет **[П док]**. Их зависание ловит только `CLAUDE_ASYNC_AGENT_STALL_TIMEOUT_MS`
  (10 мин) или бюджет на шаг workflow.
- **`--bg` и интерактивные сессии.** Сайдкар опрашивает `claude agents --json --all` раз в 2–5 с, плюс `command`-хуки
  `PreToolUse`, `PostToolUse*`, `PermissionRequest`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`,
  `SessionEnd`, которые пишут строку в сокет драйвера. Хуки не получают `OTEL_*` env, поэтому адрес передаётся своей
  переменной. **`busy` ≠ «жив»**: `busy` держится и при висящем стриме, и при ожидании лимита, и при MCP-разрешении
  **[П]**.
- **OTel самого Claude** — полный набор env для дашбордов стоимости (§5).

## 5. Как включить OTel харнессов правильно (обогащение)

**Claude Code** **[П]**. Без явного экспортёра телеметрия молча не работает: это ровно то, что сейчас происходит в
gc.

```bash
CLAUDE_CODE_ENABLE_TELEMETRY=1
OTEL_METRICS_EXPORTER=otlp
OTEL_LOGS_EXPORTER=otlp
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf              # VictoriaMetrics принимает только protobuf
OTEL_EXPORTER_OTLP_METRICS_ENDPOINT=http://127.0.0.1:8428/opentelemetry/v1/metrics
OTEL_EXPORTER_OTLP_LOGS_ENDPOINT=http://127.0.0.1:9428/insert/opentelemetry/v1/logs
OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=cumulative   # VM плохо дружит с delta
OTEL_RESOURCE_ATTRIBUTES=agent=<имя>,rig=<rig>,city=<city>      # без пробелов: пробел молча отключает копирование
# опционально: CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1 + OTEL_TRACES_EXPORTER=otlp
```

Адреса — дефолтные адреса VictoriaMetrics и VictoriaLogs (их же использует gc). Если нужен один адрес на все
сигналы, ставят otel-collector (`OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318`). Если коллектор общий для флота,
`user.email` и `account_uuid` стоит вырезать на коллекторе. Для `--bg` OTel-переменные надёжнее передать в `env` из `--settings`: на `--bg` из env оболочки
переносится только часть переменных (46a §2).

**codex** **[П]**:

```toml
[analytics]
enabled = false          # иначе metrics_exporter по умолчанию = statsig → ab.chatgpt.com

[otel]
environment = "city"
exporter        = { otlp-http = { endpoint = "http://127.0.0.1:4318/v1/logs",    protocol = "binary" } }
trace_exporter  = { otlp-http = { endpoint = "http://127.0.0.1:4318/v1/traces",  protocol = "binary" } }
metrics_exporter = { otlp-http = { endpoint = "http://127.0.0.1:4318/v1/metrics", protocol = "binary" } }
```

- В метриках codex нет id треда. Если app-server один на много тредов, разбивку по сессиям дают только логи и спаны
  (`conversation.id`) **[П]**.
- `[otel]` читается один раз на процесс; работает и под `codex-acp`, адаптер спавнит app-server с тем же env **[П E4]**.
- Кроме мониторинга, стоит снизить `stream_idle_timeout_ms` (по умолчанию 300 000) и `stream_max_retries`
  (по умолчанию 5). Иначе codex сам ждёт зависший стрим до получаса **[В]**.

**pi** **[П]**. Свой экспорт делается расширением на ~110 строк (прототип E1) или берётся готовый `pi-otel`.
Расширение не видит `auto_retry_*`, `willRetry` и ответы 4xx/5xx. Это видит только драйвер на rpc, поэтому для
мониторинга основной источник — rpc.

**Бэкенд** **[В на основе П]**. На одну машину — VictoriaMetrics + VictoriaLogs + Grafana: два бинарника плюс
Grafana, OTLP принимают напрямую, это дефолт gc.

Готовые дашборды показывают usage и стоимость, а не состояние сессии:
- Grafana 25255 и 25645;
- `VictoriaMetrics-Community/vibe-coding-dashboards`;
- `anthropics/claude-code-monitoring-guide` — устарел на 15 месяцев.

Langfuse и Phoenix подходят для разбора трасс после факта, но не для «жива ли».

## 6. Что это значит для Gas City и для альтернативы

**Находки в gc** **[П]**:

1. Gas City пробрасывает агентам `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_RESOURCE_ATTRIBUTES` и `BD_OTEL_*`, но не
   `OTEL_*_EXPORTER` и не протокол (`internal/telemetry/subprocess.go:177-191`). В опытах G1 и G2 Claude Code с таким
   env не создал ни одного экспортёра (`getOtlpReaders: types=[]`). Для codex и pi gc не настраивает ничего.
2. В живом городе нет ни одной переменной `OTEL_*`/`GC_OTEL_*`, порты 8428, 9428, 4317 и 4318 никто не слушает,
   `program_status` выключен. Живость держится на `idle_timeout` по `tmux #{window_activity}` у 41 агента и на
   регулярках по экрану.
3. ACP-рантайм gc из `session/update` берёт только `markActivity(now)` и текст. Статусы tool, `usage_update`,
   `notice` и v2-state отбрасываются (`internal/runtime/acp/conn.go:261-292`).
4. Метрики gc считают события контроллера (32 инструмента). Gauge один — `gc.bead_store.healthy`. Observable gauges
   «running agents» из roadmap не сделаны.

Пункт 1 — это небольшая правка в gc. Она по смыслу попадает в C1-минимум (FINAL-REPORT §5), но вне рамок
исследования **[В]**.

**Что перенести в альтернативу** **[В]**:
- opt-in через стандартные `OTEL_*`;
- уникальный `service.instance.id` на процесс;
- ресурсные метки agent/rig/city;
- словарь состояний `program_status`, отображённый на ACP v2 `running|idle|requires_action` + `error`;
- lifecycle-счётчики: starts, stops, crashes, quarantines.

**Что выбросить** **[В]**: скрейпинг экрана, `window_activity`, OSC-tap, «время последнего update» как суррогат
живости и половинчатый проброс без экспортёров.

**Влияние на гипотезы** **[В]**:
- **H1** (tmux не нужен) — для мониторинга подтверждена. Всё, что gc выжимает из pane, есть структурно в
  stream-json, app-server, rpc и ACP v2.
- **H8** (события против тиков) — уточнена. Переходы приходят событиями, но «зависла» — это отсутствие события.
  Нужен дедлайн на сессию, который пересчитывается на каждом сигнале, а не глобальный тик.
- **H2/H3** — ACP-драйвер должен разбирать `tool_call` статусы и v2 `state_update`, а не только время активности.

## 7. Не проверено

- ACP v2 `state_update`/`background` у claude-agent-acp и codex-acp вживую (только по коду).
- Пробрасывает ли claude-agent-acp `session_state_changed` и `api_retry` (кроме `elapsedTimeSeconds`).
- Workflows: `workflow.run_id` в OTel; `tool_progress` в субагентах.
- codex: `waitingOnUserInput`, hook `PermissionRequest`, SSE во время долгого reasoning без summary, `otlp-grpc` и
  `binary`.
- statusline как периодический источник у неприкреплённой `--bg`-сессии.
- Нагрузка и память VM, VL и Grafana; код community-расширений pi.
- Настоящая квота подписки — только фейковые 429 и метки `rate_limit_event{allowed}` с реальной подписки.

Побочные эффекты: нет.
- Claude запускался с изолированным `CLAUDE_CONFIG_DIR`, codex — с `CODEX_HOME` в lab, pi — с `PI_CODING_AGENT_DIR`
  в lab.
- `~/.claude/sessions` и `/proc/<pid>/environ` процессов города только читались.
- Процессов лаборатории не осталось: проверено по `ps`.
- Платные ходы: haiku ≈ $0,014; codex `gpt-6-luna` low — 3 коротких хода.
