# 46b. Мониторинг живой сессии codex: OTel, app-server, hooks, процессы

Дата: 2026-11-11. Стенд: `codex-cli 0.166.0`, исходники — тег `rust-v0.166.0` (6ff1e1c, 2026-10-29), клон в
`lab/observability/codex/src/codex/` (дальше `S/` = `.../src/codex/codex-rs/`). Документация developers.openai.com/codex
(config-advanced, config-reference, app-server, hooks…) — `lab/observability/codex/web/*.html|.txt`. Изолированный
`CODEX_HOME=lab/observability/codex/home`, провайдер — прокси владельца через `env_key` (как в отчёте 15), модель
`gpt-6-luna`, effort `low`. Приёмник — `lab/observability/sink/otlp_sink.py`. Скрипты — `lab/observability/codex/scripts/`,
сырьё и таймлайны — `lab/observability/codex/out/`. Прогонов codex 5: E1, E2, E5 — с LLM, E3 — фейковый провайдер, E4 — без промпта.

Метки: **[П]** — проверено опытом или исходником/докой (с местом), **[В]** — вывод, **[НП]** — не проверено.

## Выводы за 60 секунд

- **Гипотеза подтверждена для OTel, с поправками.** Логи уходят быстро: batch-задержка ≤1 с. Но почти все события описывают уже
  завершённое: `api_request` пишется после прихода заголовков, `tool_result` — после инструмента, `sse_event` — на каждое полученное
  SSE-событие. Спаны экспортируются только на end. Метрики идут delta-временностью, без активности POST нет. Долгая команда
  (E1: 26 с), ожидание approval (E2: 25 с) и зависание до заголовков (E3: 40 с) в OTel выглядят одинаково: **полная тишина** [П].
- **Штатного heartbeat у codex нет нигде**: ни в OTel, ни в `exec --json`, ни в app-server, ни в rollout [П]. Зато **app-server
  отдаёт состояние**: `thread/status/changed` = `idle | active[waitingOnApproval|waitingOnUserInput] | systemError | notLoaded`,
  `item/started … inProgress` без `item/completed`, pull-методы `thread/read`, `thread/backgroundTerminals/list`, `server/diagnostics` [П].
- **Начало команды видно в четырёх местах**: `item.started` в `exec --json` и app-server, hook `PreToolUse` с `tool_use_id`,
  в OTel — `codex.tool_decision` (только для auto-approved). Самое дешёвое — **таблица процессов**: дочерние процессы codex
  несут в env `CODEX_THREAD_ID` и `CODEX_TOOL_CALL_ID`. Это работает при любом драйвере, включая TUI [П E1].
- **OTel не видит падений команд**: `ls` с exit 2 даёт `codex.tool_result success=true` и `codex.tool.call{success=true}` [П E1].
  В `exec --json` и app-server та же команда помечена `failed, exit_code 2`.
- **OTel не различает лимиты**: usage limit и rate limit — оба `codex.api_request status=429 error.message="http 429"`, без
  `resetsAt` [П E3]. В app-server есть `codexErrorInfo:"usageLimitExceeded"` и `account/rateLimits/updated{usedPercent:100,resetsAt}`.
- **Зависание до заголовков ничем не ограничено**: `stream_idle_timeout_ms` его не покрывает. В E3 codex молча ждал 40 с, после
  обрыва соединения так же молча послал повтор. Ни одной нотификации app-server [П]. Зависание посреди стрима обрывается idle
  timeout'ом, по умолчанию **300 с**, `S/model-provider-info/src/lib.rs:71` [П].
- **В метриках нет id сессии**: только `auth_mode, originator, session_source, model, app.version` [П]. Логи и спаны несут
  `conversation.id` = thread id = ACP sessionId. Метки процесса задаются через `OTEL_RESOURCE_ATTRIBUTES` (работает [П E1–E4])
  и `otel.span_attributes`. `[otel]` читается один раз на процесс.
- **Телеметрия работает в `exec`, `app-server`, TUI и `codex-acp`**: адаптер спавнит `codex app-server` с тем же env, и `[otel]`
  из `CODEX_HOME` подхватывается [П E4]. Протоколы: `otlp-http` (`binary|json`) и `otlp-grpc`. Экспортёры раздельные: `exporter`
  для logs, `trace_exporter`, `metrics_exporter` [П].
- **По умолчанию метрики уходят в OpenAI**: `metrics_exporter` по умолчанию = `statsig`, это OTLP/JSON на
  `https://ab.chatgpt.com/otlp/v1/metrics` со встроенным ключом [П]. Выключатель — `[analytics] enabled=false`; свой OTLP
  `metrics_exporter` тоже вытесняет statsig. У владельца analytics выключена [П].
- **Фоновый процесс**: app-server сообщает о нём корректно, но статус треда при этом `idle`. `item/completed` приходит, когда
  процесс завершится, уже после хода [П E2]. `codex exec` убивает фоновые процессы при выходе [П E1].
- **Минимальный свой код**: редьюсер нотификаций app-server в состояние треда занял **89 строк** Python, прототип прогнан на
  записях E2/E3 [П]. Экспорт в OTel gauges — ещё ≈40–60 строк. Скан `/proc` по `CODEX_THREAD_ID` — ≈40 строк [В].

## 1. Документация и исходники

### 1.1. Экспортёры и конфиг `[otel]`

| Ключ | Значения | Источник |
|---|---|---|
| `otel.exporter` (logs) | `none` (умолч.) \| `otlp-http{endpoint,headers,protocol=binary\|json,tls}` \| `otlp-grpc{endpoint,headers,tls}` | `S/config/src/types.rs:611-633,690-697`; `web/config-reference.txt:1763-1790` [П] |
| `otel.trace_exporter` | то же, умолч. `none` | там же `:1815-1842` [П] |
| `otel.metrics_exporter` | `none \| statsig \| otlp-http \| otlp-grpc`, умолч. **`statsig`** | `types.rs:696`, `config-reference.txt:1803-1806` [П] |
| `otel.environment` | тег `env` в resource, умолч. `dev` | `types.rs:56` [П] |
| `otel.log_user_prompt`, `log_agent_responses`, `log_guardian_assessments`, `tool_result.max_bytes` | opt-in контент | `config-reference.txt:1791-1814` [П] |
| `otel.span_attributes`, `otel.tracestate` | атрибуты на каждый спан, W3C tracestate | `S/otel/README.md:54-72` [П] |

- `protocol = "json"` работает: все 5 опытов шли в JSON, декодировать protobuf не понадобилось [П].
- Resource: `service.name` = `codex_exec` | `codex-app-server` (то же под ACP), `service.version`, `env`, `host.name` (только logs),
  `os`/`os_version` (metrics). `Resource::builder()` (`S/otel/src/provider.rs:372-381`) подтягивает `OTEL_RESOURCE_ATTRIBUTES`:
  `lab.agent`/`lab.rig` пришли в resource logs, traces и metrics во всех опытах [П].
- `OTEL_METRIC_EXPORT_INTERVAL` соблюдается: в E1 при 5000 мс POST'ы шли раз в 5 с [П]. По умолчанию интервал 60 с:
  в E2 метрики пришли на +60.1 и +120.1 с [П]. Temporality по умолчанию delta, `aggregationTemporality=1` в E2
  (`S/otel/README.md:183-187`) [П].
- Задержка экспорта: logs ≈ ≤1 с от события до POST (E1: `tool_decision` в 5.9 с, POST в 6.3 с). Traces уходят батчем раз в 5 с,
  но **только завершённые спаны** [П].
- Провайдер OTel строится один раз при старте процесса: `S/app-server/src/lib.rs:650`, `S/exec/src/lib.rs:670`,
  `S/tui/src/startup_orchestration.rs:692`. Пересборка — только при смене аккаунта (`S/app-server/src/otel_reloader.rs`).
  Значит, per-thread `config` в `thread/start` не может дать разные OTel-метки разным тредам [П по коду].

### 1.2. События (logs) и их атрибуты

Каждое событие несёт `event.name`, `event.timestamp`, **`conversation.id`** (= thread id), `app.version`, `auth_mode`, `originator`,
`user.account_id`, `user.email`, `terminal.type`, `model`, `slug` (`S/otel/src/events/shared.rs:14-31`) [П]. Кроме того, у записи есть
`traceId/spanId` для склейки со спанами. `timeUnixNano=0`, время лежит в `observedTimeUnixNano` [П E1]. `originator` в app-server
равен `clientInfo.name` клиента: `otel-lab` в E2 и в E4 через ACP. Это метка соединения, а не треда [П].

| Событие | Когда эмитится | Ключевые поля |
|---|---|---|
| `codex.conversation_starts` | старт треда | provider, effort, approval/sandbox policy, mcp_servers |
| `codex.user_prompt` | приём промпта | длина; текст при `log_user_prompt` |
| `codex.api_request` | **после** ответа или ошибки HTTP | `http.response.status_code`, `error.message`, `attempt`, `duration_ms` |
| `codex.sse_event` | на **каждое** SSE-событие, включая delta; на ошибку стрима — с `error.message` | `event.kind`, `duration_ms`; на `response.completed` — токены |
| `codex.tool_decision` | auto-approve — **до** запуска; при approval — **после** решения | `tool_name`, `call_id`, `decision`, `source=Config\|User\|AutomatedReviewer` |
| `codex.tool_result` | после инструмента (у unified exec — после yield ≤30 с) | `duration_ms`, `success`, `output` (2 КБ) |
| `codex.turn_ttft`, `startup_phase`, `turn_cost`, `skill_invocation`, `agent_response`, `guardian_assessment`, `sandbox_outcome`, `websocket_*`, `auth_recovery` | по факту | — |

Места: `S/otel/src/events/session_telemetry.rs:679-1300`, список имён — grep по `event.name` [П]. События
`codex.tool_call_received`, `codex.unified_exec.process_start_requested`, `codex.retry`, `codex.turn_profile` идут в target
`codex_otel.trace_safe` (`S/core/src/tools/call_trace.rs:48`, `S/core/src/unified_exec/process_manager.rs:1386`,
`S/client/src/retry.rs:72`). Значит, они попадают только как span events и уходят **вместе со спаном на его end**.
Пример: `tool_call_received` произошёл в 5.8 с, а экспортирован в 40.4 с (E1) [П].

Метрики (`S/otel/src/metrics/names.rs`): `codex.api_request{status,success}`, `codex.sse_event{kind,success}`,
`codex.tool.call{tool,success}` + `.duration_ms`, `codex.turn.e2e_duration_ms`, `codex.turn.ttft`, `codex.turn.token_usage`,
`codex.approval.requested{tool,approved}`, `codex.turn.unified_exec.running_processes`, `codex.thread.started`, `codex.process.start`,
`codex.hooks.run` и др. Метки — только `auth_mode, originator, session_source, model, app.version`
(`S/otel/src/metrics/tags.rs:38-60`). Полный набор ключей атрибутов в E2 не содержит thread/conversation id [П].

### 1.3. Где работает

| Режим | OTel | Источник |
|---|---|---|
| `codex exec` | да, `service.name=codex_exec`; analytics по умолчанию **вкл.** (`DEFAULT_ANALYTICS_ENABLED=true`) | `S/exec/src/lib.rs:183,670` [П], E1, E5 |
| `codex app-server` | да, `codex-app-server`; analytics по умолчанию **выкл.**, включается флагом `--analytics-default-enabled` | `S/cli/src/main.rs:627-628,1316-1321`, `S/app-server/src/lib.rs:168,650` [П], E2, E3 |
| TUI | да, analytics по умолчанию вкл.; при работе через общий демон ходы исполняет демон-app-server со своим env | `S/tui/src/startup_orchestration.rs:692-696` [П]; про демон — [В] |
| `codex-acp` 2.6.0 | да: адаптер спавнит `codex app-server` с `process.env`, `[otel]` из `CODEX_HOME` подхватывается; `conversation.id` = ACP `sessionId` | `src/codex-acp-new/src/CodexJsonRpcConnection.ts:37-50` [П]; E4: logs+metrics+traces пришли без единого промпта [П] |

Демон владельца запущен с `--analytics-default-enabled` (`ps`), но в его `~/.codex/config.toml` стоит `[analytics] enabled=false` [П].

### 1.4. Что уходит в OpenAI по умолчанию

1. **Statsig-метрики.** `metrics_exporter=statsig` разворачивается в OTLP/HTTP JSON на `https://ab.chatgpt.com/otlp/v1/metrics`
   с зашитым ключом `client-…` (`S/otel/src/config.rs:9-36`). В debug-сборках выключено, в релизных — нет [П]. Логика выбора
   такая (`S/core/src/otel_init.rs:70-79`): statsig отключается при `analytics.enabled=false`. Если задан свой OTLP
   `metrics_exporter`, statsig тоже не используется: экспортёр метрик один. Документация говорит то же
   (`web/config-advanced.txt:1176-1180`) [П].
2. **Analytics events** (`{chatgpt_base}/codex/analytics-events/events`). Включены, если `analytics.enabled != false`
   (`S/analytics/src/client.rs:326-336`). Отправляются только при ChatGPT- или API-key-auth (`:986-995`). С прокси через `env_key`
   auth нет, поэтому не отправляются [П по коду].
3. **Побочный сетевой трафик, не телеметрия.** При старте выполняется `git ls-remote https://github.com/openai/plugins…`
   (plugins startup sync) [П E1/E4 по таблице процессов]. app-server раз в 150 с опрашивает провайдера `POST /v1/analytics/codex/turn-costs`
   (`S/app-server/src/turn_cost_worker.rs`; первый POST виден в логе фейка E3) [П]. Remote-control-цикл app-server ждёт ChatGPT-auth
   (`S/app-server-transport/src/transport/remote_control/auth.rs:110`).

Выключение: `[analytics] enabled=false` + `[feedback] enabled=false`. Явный `otel.metrics_exporter` на свой коллектор [П].

## 2. Опыты с таймлайном

### E1: `codex exec --json` — `sleep 45`, падающий `ls`, фоновый `sleep 30`

Скрипт `scripts/e1_exec_otel.py`, `OTEL_METRIC_EXPORT_INTERVAL=5000`. Данные: `out/e1_otlp.jsonl`, `out/e1_timeline.txt`,
`out/e1_exec.jsonl`, `out/e1_samples.jsonl` (раз в 1 с: наши процессы и mtime rollout). Модель работает через code-mode: JS-инструмент
`exec` вызывает `tools.exec_command`. Yield не больше 30 с (`S/core/src/unified_exec/mod.rs:70`), поэтому `sleep 45` = `exec_command`
(вернулся через 30 с с «Process running with session ID») + `write_stdin` (7.5 с).

| t, с | OTel | `exec --json` | /proc, rollout |
|---|---|---|---|
| 0.6 | `conversation_starts`, `user_prompt` | `thread.started`, `turn.started` | — |
| 3.3 | `api_request 200` (дошли заголовки) | — | — |
| 5.9 | `tool_decision exec_command approved/Config` (POST 6.3) | 6.0 `item.started command_execution in_progress` | 6.0: `sleep 45` с `CODEX_THREAD_ID=<thread>`, `CODEX_TOOL_CALL_ID=exec-0eee…` |
| **10.3 → 36.4** | **0 POST за 26 с** (logs, traces, metrics), хотя интервал 5 с | тишина | rollout не меняется с 6.0 до 36.0 |
| 36.0 | `tool_result exec_command 30172 ms success=true "Process running…"`; спан `exec_command` 5.8–36.0 пришёл в 40.4 | — | — |
| 41.7–43.4 | `api_request`, SSE-события (модель решает поллить) | — | rollout +1 запись |
| 50.9 | `tool_result write_stdin "Process exited with code 0"` | `item.completed … completed exit 0` | `sleep` исчез |
| 54.8 | `tool_result exec_command success=true`, вывод «**exit code 2**»; метрика `codex.tool.call{success=true}` | `item.completed status=failed exit_code=2` | — |
| 61.0–62.2 | `tool_decision`, `tool_result "Process running… 34350"` | `item.started` (фон), `item.completed` **не пришёл** | `sleep 30` с тем же `CODEX_THREAD_ID` |
| 65.1 | спаны `run_turn`/`session_task.turn` 0.6–65.1 только теперь; `codex.turn.unified_exec.running_processes=1` | `turn.completed`, выход rc=0 | 65.6: фоновый `sleep` **убит** вместе с exec |

Выводы E1 [П]:
- «Команда в процессе» в OTel видна только косвенно: `tool_decision` без парного `tool_result` по тому же `call_id`. Это работает
  лишь для инструментов, проходящих через approval-оркестратор (`S/core/src/tools/orchestrator.rs:192`; у `write_stdin` такого маркера нет).
  Единственный «пульс» — поллинг самой моделью раз в ≤30 с.
- Ошибка команды в OTel не отмечена: `success` означает «инструмент отработал», а exit code есть только в тексте `output`.

### E2: `codex app-server` — фон, approval, простой

`scripts/e2_appserver_otel.py`, интервал метрик по умолчанию. Данные: `out/e2_rpc.jsonl`, `out/e2_timeline.txt`, `out/e2_run.txt`.

| t, с | app-server | OTel |
|---|---|---|
| 3.4 | `item/started commandExecution inProgress` (`sleep 40`, фон) | `tool_decision` |
| 7.1 | `turn/completed`, **`thread/status/changed idle`**, хотя процесс жив | — |
| 7–46 | `thread/backgroundTerminals/list` → `[{itemId, processId:"58551", command, osPid:null, cpuPercent:null, rssKb:null}]`; `server/diagnostics` → `core.turns.active:0` | только наши RPC-спаны и фоновые `auth`-спаны |
| 43.3 | `item/completed commandExecution completed exit 0 durationMs 39876` — **после хода** | — |
| 49.2 | `thread/status/changed active[waitingOnApproval]` + server request `item/commandExecution/requestApproval` | последние SSE-события |
| **49.2 → 74.3** | `thread/read` → `active[waitingOnApproval]`; `server/diagnostics` → `app.server_requests.pending:1`, `core.turns.active:1` | **ни одного лога** |
| 74.3 | `serverRequest/resolved`, `active[]`, `item/completed` | `tool_decision source=User`; `tool_result duration 25092` (включает ожидание); `codex.approval.requested` — только в POST метрик на 120.1 |
| 76.9–147 | `idle`; 70 с простоя без нотификаций | POST traces раз в 5 с: пары спанов `auth` каждую секунду; metrics раз в 60 с |

`osPid/cpuPercent/rssKb` в 0.166 всегда `None` (`S/app-server/src/request_processors/thread_processor.rs:2625-2627`), документация
показывает то же (`web/app-server.txt:1573-1588`) [П]. Пульс `auth` раз в 1 с — это побочный эффект. Remote-control-цикл ждёт
ChatGPT-auth и перечитывает auth (`S/login/src/auth/manager.rs:2498`, в `logs_2.sqlite` строки
«waiting to resolve remote control preference until authentication is available») [П]. Это не сигнал жизни сессии, и с другим
auth он может пропасть [В].

### E3: ошибки API на фейковом провайдере, без LLM

`scripts/fake_provider2.py` + `scripts/e3_failures_otel.py`, у провайдера `stream_idle_timeout_ms=15000`, `stream_max_retries=1`,
`request_max_retries=2`. Данные: `out/e3_*`.

| Сбой | app-server | OTel | HTTP-попыток |
|---|---|---|---|
| Висит до заголовков (fake 40 с молчит и закрывает) | **ничего** 45 с, статус `active[]`; после обрыва — молчаливый повтор; остановлено нашим `turn/interrupt` → `interrupted` | 40 с тишина, затем `api_request status=none error="error sending request" 40008 ms`; второй попытки в логах нет | 2 |
| Стрим замолчал после `response.created` | через 15 с `error{willRetry:true, responseStreamDisconnected, "idle timeout waiting for SSE"}`; 77.5 `systemError`, `turn failed` | `sse_event error="idle timeout waiting for SSE" 15006 ms` ×2 | 2 |
| 503 | один финальный `error{serverOverloaded, willRetry:false}`; промежуточных попыток нет | 3 × `api_request status=503 attempt=0/1/2 "http 503"` | 3 |
| 429 usage limit + `x-codex-*` | `account/rateLimits/updated{primary{usedPercent:100,resetsAt}, secondary{42%}}`, `error{usageLimitExceeded}`, `systemError` | `api_request status=429 "http 429"` — **без типа и времени сброса** | 1 |
| 429 + `retry-after: 20` | `error{willRetry:true, responseStreamDisconnected{429}}` → 20 с тишины → `responseTooManyFailedAttempts{429}` | 2 × `api_request 429` с интервалом 20 с | 2 |

[П] для всех строк. Две поправки к отчёту 15:
- 429 с `Retry-After` **повторяется** после указанной задержки: `RateLimitExceeded{retry_after}` в `S/codex-api/src/api_bridge.rs:23-26`.
  Без заголовка не повторяется, как в отчёте 15.
- Ретраи на уровне HTTP-запроса видны **только в OTel** (`attempt=N`), не в app-server.

Спаны `run_turn` у упавших ходов имеют `status.code=0` (Unset): ошибка в статус спана не попадает [П].

### E4: `codex-acp` и `[otel]`

`scripts/e4_acp_otel.py`: только `initialize` + `session/new`, без промпта. Пришли logs (`conversation_starts`,
`conversation.id` = sessionId `01a1c83a-…`), metrics и traces с `service.name=codex-app-server` и `lab.agent=agent-acp` из env
адаптера [П].

### E5: hooks + `notify` + локальный лог-БД

`scripts/e5_exec_hooks.py`: 14 hook-событий в `home/hooks.json` → `hooks/hook.py`, `-c notify=[…]`,
`--dangerously-bypass-hook-trust`, команда `sleep 12`.

```
+0.1 SessionStart, UserPromptSubmit   +0.2 PreModelRequest   +5.8 PostModelRequest
+5.8 PreToolUse  tool=Bash tool_use_id=exec-0fa0b3cf…  input={'command': 'sleep 12; echo HOOK-OK'}
+17.9 PostToolUse (тот же tool_use_id)   +17.9 PreModelRequest   +20.6 PostModelRequest, Stop, notify(agent-turn-complete), SessionEnd
```

У всех событий `session_id` = thread id [П]. `tool_use_id` = `CODEX_TOOL_CALL_ID` в env процесса команды, сверено с E1 по формату
`exec-<uuid>` [П/В]. `notify` (legacy) умеет только `agent-turn-complete` (`S/hooks/src/legacy_notify.rs:16-27`) [П].
`logs_2.sqlite` за прогон `exec` не получил ни одной строки: `max_id` 1307 без изменений (`out/e5_logsdb.jsonl`) [П].
Живым источником для exec он не является. В app-server строки с колонкой `thread_id` пишутся, но живость для них не проверялась [НП].

## 3. Источники живого состояния и итоговая таблица

- **`exec --json`**: события `thread.started`, `turn.started|completed|failed`, `item.started|updated|completed`, `error`
  (`S/exec/src/exec_events.rs:11-36`). Периодических событий нет, дельт текста нет. Команда — пара `item.started`/`item.completed`
  со `status` и `exit_code` [П E1].
- **app-server**: push-нотификации — `thread/status/changed`, `turn/*`, `item/*` (включая `outputDelta`, `agentMessage/delta`,
  `reasoning/*Delta`), `error{willRetry,codexErrorInfo}`, `account/rateLimits/updated`, `serverRequest/resolved`. Pull-запросы —
  `thread/read` (status), `thread/loaded/list`, `thread/backgroundTerminals/list|terminate|clean` и `server/diagnostics`;
  последние два экспериментальные (`S/app-server-protocol/src/protocol/common.rs:509,771-787`). `server/diagnostics` возвращает
  гейджи процесса: `core.turns.active`, `core.threads.live`, `app.server_requests.pending`, `app.requests.in_flight`,
  `core.mailbox.pending`, `mcp.connections.live` и RSS. Heartbeat'а нет [П].
- **hooks**: 14 событий (`S/hooks/src/lib.rs:29-44`). Для живости полезны `PreModelRequest/PostModelRequest` (окно вызова модели),
  `PreToolUse/PostToolUse` (окно инструмента), `PermissionRequest`, `Stop`, `Interrupt`. Hooks синхронны: каждый — запуск процесса
  на пути хода и требует доверия (trust) [П].
- **Rollout** (`$CODEX_HOME/sessions/**/rollout-*.jsonl`): пишется по item'ам, во время команды mtime стоит (E1: 6.0 → 36.0) [П].
  Подходит для пост-анализа, не для живости.
- **Таблица процессов**: `CODEX_THREAD_ID`/`CODEX_TOOL_CALL_ID` в env дочерних процессов (`S/core/src/exec_env.rs:30`,
  `S/exec-server/README.md:25`). Ограничения: виден только тот же uid, команда может переопределить эти переменные (README: «correlation,
  not authorization»), фоновые процессы под app-server видны до их конца [П E1].

| Состояние | Чем ловится штатно (лучшее → запасное) | Задержка | Надёжность |
|---|---|---|---|
| Жива и думает | app-server: `active[]`, нет tool-item в работе, идут `*/delta`; hooks: после `PreModelRequest` без `PostModelRequest`; OTel: `sse_event` на каждое SSE | push ≈0; OTel ≤1 с | Средняя: reasoning без стриминга даёт **тишину**, неотличимую от зависания [П E3/В] |
| Выполняет команду | `item/started inProgress` (app-server, `exec --json`); `PreToolUse`; `/proc` с `CODEX_TOOL_CALL_ID`; OTel `tool_decision` без `tool_result` | ≈0; /proc — период опроса | Высокая для app-server, exec и /proc; в OTel только для auto-approved, «конец» — после yield, а не после команды [П] |
| Ждёт approval | `thread/status active[waitingOnApproval]` + server request; `server/diagnostics app.server_requests.pending`; hook `PermissionRequest` [НП вживую] | ≈0 | Высокая в app-server; **в OTel невидимо** до решения [П E2] |
| Ждёт ввода | app-server: `turn/completed` + `idle`; mid-turn вопрос — `waitingOnUserInput` + `item/tool/requestUserInput`; hook `Stop`, `notify` | ≈0 | Высокая; `waitingOnUserInput` вживую не проверялся [НП]; в OTel есть только end спана хода |
| Упала на лимите | `error{usageLimitExceeded}` + `rateLimits{usedPercent,resetsAt}` + `systemError` | ≈0 | Высокая в app-server; в OTel — просто `429`, без типа и reset [П E3]; у владельца через прокси `rateLimits` пусты (отчёт 15) |
| Сыплет ошибками | OTel: `codex.api_request{success=false,status}` с `attempt` — лучший счётчик частоты; app-server: `error{willRetry}` | OTel ≤1 с (лог), метрика — интервал | Хорошая для API-ошибок; ошибки команд в OTel **не видны** (`success=true`) [П] |
| Фоновый процесс идёт | app-server: `item/started` без `item/completed` после `turn/completed`; `thread/backgroundTerminals/list`; `/proc` | ≈0 / опрос | Хорошая; статус треда при этом `idle`, `osPid/cpu/rss` = null; в exec фон умирает с процессом [П] |
| Зависла (стрим молчит) | Посреди стрима: свой idle timeout (умолч. 300 с) → `error{willRetry:true}` / `sse_event error`. До заголовков: **ничего** | 300 с × (1+5 повторов) ≈ до 30 мин [В]; до заголовков — без ограничения [П ≥40 с] | Низкая: штатно не ловится, нужен внешний таймер тишины |
| Процесс мёртв | EOF stdio/сокета app-server (мгновенно, отчёт 15); `waitpid`/код выхода exec; в OTel — только отсутствие данных | ≈0 для супервизора | Высокая у владельца процесса; OTel теряет несброшенный батч и незакрытые спаны [В] |

## 4. Предложение: «жива или зависла» на сессию с минимумом своего кода

**Штатно уже есть** (codex через app-server, своим супервизором):
1. Машина состояний треда приходит push'ем: `thread/status/changed` + `item/started|completed` + `error` + `turn/completed`.
2. Снимки по запросу: `thread/read`, `thread/backgroundTerminals/list`, `server/diagnostics`.
3. Типизированные сбои и лимиты: `codexErrorInfo`, `willRetry`, `account/rateLimits/updated`.
4. OTel `[otel]` — для дашбордов «задним числом»: частота ошибок API по статусу и `attempt`, длительность инструментов,
   TTFT, токены. Метки агента/rig навешиваются через `OTEL_RESOURCE_ATTRIBUTES` (если процесс на агента) или по `conversation.id`.

**Придётся написать:**
1. **Редьюсер нотификаций** → состояние треда (`model | tool_running | waiting_approval | waiting_input | retrying | error | idle |
   idle+background`), плюс `last_event_ts` и время начала каждого inflight-инструмента. Прототип —
   `scripts/state_reducer.py`, **89 непустых строк**. Прогон на записях E2 и E3 дал корректные переходы
   (`out/e2_states.txt`, `out/e3_states.txt`): `model → waiting_approval → tool_running`, `idle+background → idle` в момент
   завершения фона, `retrying → error` [П].
2. **Экспорт в OTel**: observable gauges `agent.session.state{session,state}`, `agent.session.silence_seconds` (now − последнее
   событие), `agent.session.tool_inflight_seconds`, `agent.session.background_processes`, counter `agent.session.errors{codex_error,will_retry}`,
   `agent.ratelimit.used_percent/resets_at{window}`. С OTel SDK это ≈40–60 строк Python или ≈100–150 строк Go [В].
   Это и есть недостающий heartbeat: gauge с callback'ом экспортируется на каждом интервале, даже когда codex молчит.
3. **Детектор зависания** — внешний таймер: `state ∈ {model, retrying}` и `silence_seconds > N` → алерт или `turn/interrupt`
   + повтор. N должен превышать максимальную законную паузу reasoning [В]. В конфиге провайдера стоит снизить
   `stream_idle_timeout_ms` (умолч. 300 с) и `stream_max_retries` (умолч. 5), иначе codex сам ждёт до получаса [В]. Для
   зависания до заголовков у codex таймаута нет, поэтому только этот таймер [П].
4. **Скан `/proc`** раз в 1–5 с: процессы с `CODEX_THREAD_ID` дают живые команды и фон с PID, CPU и RSS — то, что app-server
   отдаёт как `null`. ≈40 строк [В]. Работает и для TUI/exec без app-server.
5. **Без app-server** (TUI, exec) тот же редьюсер кормится hooks (`Pre/PostModelRequest`, `Pre/PostToolUse`, `PermissionRequest`,
   `Stop`): hook-скрипт пишет строку в unix-сокет или файл супервизора. Цена — запуск процесса на каждое событие синхронно
   с ходом и управление trust [В].

Итого ≈200–300 строк своего кода на «жива / думает / в команде / ждёт / зависла / мертва» по каждой сессии codex.
OTel codex остаётся источником ретроспективных метрик, а не живости [В].

## Вердикт по гипотезе

- «События уходят по завершении» — **в основном да**. Транспорт логов почти real-time (≤1 с), но события в основном итоговые.
  Маркеры начала есть только у `conversation_starts`, `user_prompt` и `tool_decision` (auto-approve) [П].
- «Спаны на end» — **да**, включая span events «tool_call_received» [П].
- «Счётчики только при активности» — **да**: delta-временность, пустой интервал не экспортируется (E1, 26 с без POST) [П].
  Исключение — побочный 1 Гц auth-цикл app-server, а не задуманный сигнал.
- «Долгая команда, зависший стрим и approval в OTel одинаково молчат» — **да** (E1, E3, E2) [П].
- «Нужен отдельный in-progress/heartbeat» — **да для OTel**. Для codex в app-server есть **in-progress-состояние**, но не
  heartbeat. Heartbeat придётся синтезировать своим экспортёром (§4) [В].

Отношение к H1/H3 (CONTEXT): мониторинг codex без tmux полноценен только через app-server (или hooks + /proc). В ACP
(`codex-acp`) статус треда доступен лишь через `_meta.codex.threadStatus` (отчёт 15). OTel под ACP работает так же,
поскольку внутри тот же app-server [П E4].

## Открытые вопросы и непроверенное

- `waitingOnUserInput` (`request_user_input`) и hook `PermissionRequest` вживую не воспроизводились [НП].
- Есть ли SSE-события во время долгого reasoning без summary, то есть можно ли отличить «думает» от «молчит», — не проверено [НП].
- Что происходит с фоновыми терминалами при выгрузке треда (`thread_unload_delay_secs`, 60 с) и при `thread/unsubscribe` [НП].
- TUI в режиме общего демона: какой процесс эмитит OTel и с чьим env — только по коду [В].
- `otlp-grpc` и `protocol=binary` не гонялись, всё шло в JSON [НП].
- Сетевой egress к `ab.chatgpt.com` при включённой analytics не перехватывался — только по коду [В].
- `originator` при нескольких клиентах одного app-server: метка соединения или треда [НП].
- `logs_2.sqlite` в app-server как живой источник per-thread не изучался [НП].
