# 05. Что из лаборатории переиспользовать для контрактных тестов (§7.8 постановки)

Все пути — от `tmp/gc-alt-research/lab/`. Существование и размеры проверены 2026-11-12 (`stat`); скрипты не запускались.
Версии, на которых сняты логи: claude 2.1.322, codex 0.166.0, 2026-11-10 и 2026-11-11, Linux aarch64.
Столбец «Скоп.» = файл лежит в `aae-context/lab/` по тому же относительному пути. Метки сценариев — номера из §7.8 (1–15, в порядке перечисления).

## 1. Поддельные API провайдеров (без затрат на LLM)

| Путь | Что делает | Как запускался | Сценарии | Байт | Скоп. |
|---|---|---|---|---|---|
| `claude/fake-api.mjs` | Anthropic Messages API: режим на весь процесс — `429` (retry-after 1), `529`, `401`, `400ctx`, `quota` (429 + retry-after 3600 + unified-заголовки), `529x2ok`, `ok` (SSE «FAKE-OK») | `node fake-api.mjs <port> <mode> [logfile]`; claude: `ANTHROPIC_BASE_URL=http://127.0.0.1:<port> ANTHROPIC_API_KEY=sk-ant-fake-lab-key CLAUDE_CONFIG_DIR=<пустой> CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_TELEMETRY=1` | 2, 3, 5, 6, 8; 1 (через `529x2ok`) | 3 878 | да |
| `observability/claude/scripts/fake_api.mjs` | то же, но **план по номеру запроса**: `ok,529,500,429,quota,hang,slow,tooltouch` через запятую, последний повторяется. `hang` — заголовки + `message_start`, дальше тишина; `slow` — дельта раз в 2 с; `tooltouch` — ответ с `tool_use` Bash (вызывает запрос разрешения); non-stream запрос получает JSON-ответ | `node fake_api.mjs <port> <log.jsonl> <plan>` | 1 (`429,429,ok`), 2, 3, 5, 12 (`hang`), 13 (`tooltouch,ok`) | 6 419 | да |
| `codex/scripts/fake_provider.py` | Responses API: режим = первый сегмент пути (`/usage/v1`, `/r429/v1`, `/r401/v1`, `/r503/v1`, `/ctx/v1`); `usage` — 429 `usage_limit_reached` + заголовки `x-codex-*`; `ctx` — SSE `response.failed` `context_length_exceeded` | `python3 fake_provider.py <port> <logfile>`; провайдеры описаны в `codex/home-fake/config.toml`, выбор `-c 'model_provider="<mode>"'` | 2, 3, 5, 6, 8 | 2 937 | да |
| `codex/home-fake/config.toml` | конфиг `CODEX_HOME` с пятью `[model_providers.*]` на `127.0.0.1:18765` | — | к строке выше | 736 | да |
| `observability/codex/scripts/fake_provider2.py` | режим читается из файла на каждый POST: `hang_headers` (молчит 40 с и рвёт), `stall` (SSE `response.created`, потом тишина), `r503`, `usage`, `r429` с `retry-after: 20` | `python3 -I fake_provider2.py PORT MODE_FILE LOG` | 12, 2, 3, 5 | 3 699 | да |

**Чего в подделках нет — дописать:**
- codex: **режима успешного ответа** (любой неизвестный режим даёт 500) → сценарии 1, 10, 13, 14 на подделке сейчас невозможны; нужен SSE Responses API c `response.completed` и вариант с вызовом команды;
- обе: квота незнакомой формулировкой (4), `insufficient_quota`/billing (7), несуществующая модель (9);
- claude: ответ с `tool_use` долгой команды (для 10, 11) и с фоновым запуском (14) — образец кода есть в ветке `tooltouch`.

## 2. Скрипты-драйверы (образцы точных вызовов)

| Путь | Что делает | Как запускался | Сценарии | Байт | Скоп. |
|---|---|---|---|---|---|
| `claude/native.mjs` | минимальный клиент `claude -p` stream-json: spawn, построчный разбор, `user()`, `waitFor()` | библиотека | основа всех | 2 317 | да |
| `claude/exp7-fail.mjs` | поднимает `fake-api.mjs`, гонит один и тот же промпт дважды в один процесс, печатает `api_retry`/`assistant.error`/`result` | `node exp7-fail.mjs <native\|acp> <mode> [maxRetries] [timeoutSec]` (импортирует `acp.mjs`) | 2, 3, 5, 6, 8 | 5 776 | нет |
| `observability/claude/scripts/stdio_driver.py` | клиент с `--permission-prompt-tool=stdio`: отвечает `control_response{allow}` на `can_use_tool` через N секунд, закрывает stdin после первого `result` | `python3 -I stdio_driver.py OUT.jsonl DELAY_S PROMPT -- claude …` | 13, 11 (EOF после `result`) | 1 966 | да |
| `observability/claude/scripts/run_stdio.sh` | полный запуск: чистое окружение `env -i`, изолированный `CLAUDE_CONFIG_DIR`, все флаги, поддельный API | `run_stdio.sh TAG PROMPT_FILE fake:<plan>` | 13 | 3 749 | да |
| `claude/exp8c-orphan.mjs`, `claude/exp4-crash.mjs` | `kill -9` адаптера / дочернего claude посреди хода, проверка сирот и resume — **через ACP-адаптер** | `node exp8c-orphan.mjs [mid]` | 10, 11 (как идея; переписать на нативный путь) | 1 851 / 3 797 | нет |
| `claude/exp8-cross.mjs` | второй писатель: `claude -p --resume <id>` при живой сессии → ветка в JSONL | `node exp8-cross.mjs` (реальная модель) | 15 | 6 417 | нет |
| `cc-native/scripts/run.sh` + `cc-native/logs/e3.ndjson`, `e3r.ndjson` | `kill -9` нативного `claude -p` посреди Bash и `--resume`; здесь зафиксирован осиротевший Bash | реальная модель | 10 | 822 / 21 302 / 38 502 | нет |
| `codex/scripts/asc.py` | клиент app-server: JSONL по stdio (`Rpc`) и **WebSocket поверх unix-сокета без зависимостей** (`WsRpc`: рукопожатие, маска, ping/pong, фрагменты); обработка серверных запросов | библиотека; `app_server(name, home, extra_args)` | основа всех | 10 246 | да |
| `codex/scripts/e5_failures.py` | пять сбоев подряд на `fake_provider.py`, считает HTTP-попытки | `python3 e5_failures.py [usage r429 r401 r503 ctx]` | 2, 3, 5, 6, 8 | 2 443 | да |
| `codex/scripts/e3_config_hooks_kill.py` | один процесс, 3 треда с разным конфигом (хуки, модель, MCP); затем `sleep 77`, `kill -9`, проверка сирот, новый процесс и `thread/resume` | `python3 e3_config_hooks_kill.py` (реальная модель через прокси) | 10; критерий 13 постановки | 6 482 | да |
| `codex/scripts/e2_multiclient_kill.py` | сервер на unix-сокете, два клиента: подключение к идущему ходу, `turn/steer`, `turn/interrupt`, `kill -9`, рестарт | реальная модель | 10, 15 | 7 543 | нет |
| `codex/scripts/e10_env_lock.py` | env на тред; второй **процесс** на тот же тред → `already has an active writer` | реальная модель | 15 | 2 454 | нет |
| `wave2-exp/scripts/exp3.py` | два клиента: второй `turn/start` посреди хода, гонка двух `turn/start`, подтверждение при двух подписчиках, `codex queue` | реальная модель; клиент `wave2-exp/scripts/asc2.py` | 13, 15 | 10 810 | да |
| `observability/codex/scripts/state_reducer.py` | редьюсер нотификаций app-server → состояние треда (`model/tool_running/waiting_approval/retrying/error/idle/idle+background`), 89 строк | прогонялся на `e2_rpc.jsonl`, `e3_rpc.jsonl` | 12, 13, 14 (эталон автомата) | 3 929 | да |
| `wave2-exp/scripts/city-mcp.mjs` | общий HTTP-MCP с идентичностью по заголовкам, режимы stateful/stateless/dual | `MCP_MODE=… node city-mcp.mjs` | не §7.8; критерий 14 постановки | 10 276 | нет |

## 3. Снятые логи протоколов (образцы для сценарного драйвера)

| Путь | Что внутри | Сценарии | Байт | Скоп. |
|---|---|---|---|---|
| `claude/logs/exp7-native-429.ndjson` | 429 ×3 с `CLAUDE_CODE_MAX_RETRIES=2`: `api_retry`×2 → `assistant{error}` → `result{subtype:"success",is_error:true}`; второй ход в том же процессе | 2 | 11 884 | да |
| `claude/logs/exp7-native-529x2ok.ndjson` | 529 ×2, затем успех (умолчание `max_retries:10`) | 1 | 11 769 | да |
| `claude/logs/exp7-native-quota.ndjson` | «квота» при API-ключе: без `api_retry`, без `rate_limit_event` | 3 (ожидание — `rate_limited` без времени) | 10 832 | да |
| `claude/logs/exp7-native-529.ndjson` | 529 до исчерпания; финал `server_error` | 5 | 12 066 | да |
| `claude/logs/exp7-native-401.ndjson` | 401: 7 `api_retry` с задержками 567→32 848 мс; **`result` нет** (опыт оборван) | 6 | 4 858 | да |
| `claude/logs/exp7-native-400ctx.ndjson` | переполнение контекста + неудачная компакция на втором ходе | 8 | 11 469 | да |
| `observability/claude/runs/f4/stream.jsonl` | полный ход с `session_state_changed`, `control_request can_use_tool`, ответом драйвера, `result`, `idle`, выходом после EOF; события хуков | 13, 11 | 14 461 | да |
| `observability/claude/runs/r2/stream.jsonl` | реальная подписка: Bash на 65 с с `tool_progress` на 30 и 60 с, `task_started`/`task_notification`, `rate_limit_event{allowed}`, дельты | сторож `stall` | 35 438 | да |
| `observability/claude/runs/r1/stream.jsonl` | реальная подписка: асинхронный субагент, фоновый Bash, `background_tasks_changed`, ход с `origin.kind:"task-notification"` | 14 | 128 029 | нет |
| `observability/claude/runs/f1/stream.jsonl`, `f6/stream.jsonl` | зависший стрим: 529→500→`hang`→fallback; `kill -9` во время зависания | 12, 10 | 10 082 / 5 395 | нет |
| `codex/out/e5_usage.jsonl` | **эталон полного обмена**: `initialize` → `thread/start` → `turn/start` → `rateLimits/updated` → `systemError` → `error` → `turn/completed failed` → `rateLimits/read` | 3 | 8 705 | да |
| `codex/out/e5_r429.jsonl`, `e5_r401.jsonl`, `e5_ctx.jsonl` | 429 без ретрая; 401 с пятью `willRetry:true`; переполнение контекста | 2, 6, 8 | 8 150 / 10 784 / 9 458 | да |
| `codex/out/e5_r503.jsonl` | 503, один финальный `serverOverloaded` | 5 | 8 083 | нет |
| `codex/out/e5_run.txt` | сводка пяти сбоев: времена, `codexErrorInfo`, число HTTP-попыток | 2, 3, 5, 6, 8 | 6 247 | да |
| `codex/out/e3b.jsonl` | новый процесс после `kill -9`: `thread/resume` (с `deprecationNotice`), ход `interrupted`, следующий ход | 10 | 11 346 | да |
| `codex/out/e10b.jsonl`, `e10_run.txt` | отказ второму процессу: `already has an active writer` | 15 | 1 384 / 626 | да |
| `coldstart/logs/codex-appserver-cold1.ndjson` | успешный обмен: два хода в одном треде, порядок `idle` → `turn/completed` | основа | 16 453 | да |
| `wave2-exp/logs/exp3-orchestrator.jsonl`, `exp3-run34.out` | unix-сокет: `requestApproval` (id 0 и 1), ответы, `serverRequest/resolved`, `waitingOnApproval`, `thread/queue/changed` | 13, 15 | 50 574 / 9 916 | да |
| `observability/codex/out/e3_run.txt` | зависание до заголовков, тишина стрима, 503, квота, 429 с `retry-after` — сводка | 12, 2 | 2 185 | да |
| `observability/codex/out/e3_rpc.jsonl`, `e2_rpc.jsonl` | сырой поток тех же сбоев; фоновая команда, подтверждение, `backgroundTerminals/list`, `server/diagnostics` | 12, 13, 14 | 37 745 / 61 142 | нет |
| `codex/schema-ts/` | TypeScript-типы протокола 0.166.0 (`generate-ts`), 97 записей | справка | 3,1 МБ | нет |

Форматы строк логов разные: `claude/logs/*.ndjson` — `<ISO-время> << <json>` (`>>` — отправлено, `STDERR`); `observability/claude/runs/*/stream.jsonl` — `{"t_ms":…,"m":{…}}`, отправленное лежит в `m._driver_sent`; `codex/out/*.jsonl` и `wave2-exp/logs/*.jsonl` — `{"t":<сек>,"dir":">"|"<"|"eof","msg":{…}}`; `coldstart/logs/*.ndjson` — `{"t":<мс>,"dir":…,"m":{…}}`; `observability/codex/out/*_rpc.jsonl` — `{"t_ms":…,"dir":…,"msg":…}`.

## 4. Покрытие 15 сценариев

| № | Сценарий | claude | codex |
|---|---|---|---|
| 1 | 429 ×2, затем успех | подделка: план `429,429,ok`; лог-аналог `529x2ok` | **нет** (нет режима успеха) |
| 2 | 429 бесконечно | есть | есть (`r429`; с `retry-after` — `fake_provider2.py`) |
| 3 | квота с временем сброса | есть (ожидать `rate_limited` без времени) | есть (`usage`) |
| 4 | квота незнакомой формулировкой | нет | нет |
| 5 | 529 / 503 | есть | есть |
| 6 | 401 | есть, без финального `result` | есть |
| 7 | `insufficient_quota` | нет | нет |
| 8 | переполнение контекста | есть | есть |
| 9 | несуществующая модель | нет | нет (есть только `modelProviderMismatch`) |
| 10 | `kill -9` посреди команды | только с реальной моделью или через ACP | только с реальной моделью |
| 11 | EOF на stdin посреди хода | EOF после `result` — есть (f4); посреди хода — только через ACP | нет |
| 12 | провайдер молчит | есть (`hang`) | есть (`hang_headers`, `stall`) |
| 13 | запрос разрешения | есть на подделке (`tooltouch`) | только с реальной моделью |
| 14 | фоновая задача после конца хода | только с реальной моделью (r1) | только с реальной моделью |
| 15 | второй писатель | только с реальной моделью | отказ второму процессу — есть лог; на подделке не воспроизводилось |

## 5. Скопировано в `aae-context/lab/`

33 файла, 308 216 байт (подсчёт 2026-11-12, `du -sb`), побайтно совпадают с оригиналами (`cmp`). <!-- MEASURE id=aae-context-lab-copy-size date=2026-11-12 ver=claude-2.1.322,codex-0.166.0 env=linux-arm64-orbstack src=du-sb-aae-context/lab note="33 файла; версии — харнессов, на которых сняты логи; пересчитать при изменении набора" -->
Состав: все строки таблиц выше с «да» в столбце «Скоп.».

Перед использованием:
- во всех скриптах зашиты абсолютные пути стенда (`/home/ubuntu/proj/work/gascity/tmp/gc-alt-research/lab/…`) и порты (`18765`) — править;
- `run_stdio.sh` в режиме `real` читает OAuth-токен владельца из `~/.claude/.credentials.json`; в тестах использовать только `fake:<план>`;
- логи содержат абсолютные пути машины владельца, а логи реальных прогонов (`r2`, `exp3-orchestrator`, `e3b`, `cold1`) — ответы модели и имя его провайдера. В публичный репозиторий класть только после чистки; предпочитать логи с поддельным API;
- `claude/logs/exp3.ndjson` (два успешных хода под подпиской, 22 КБ) **не скопирован намеренно**: в нём вывод личного хука владельца;
- `codex/home/config.toml` не копировать: там адрес прокси владельца.

## 6. Чего здесь нет

- Ни одного сценарного (поддельного) **драйвера** харнесса в смысле §14 постановки в лаборатории нет — есть только поддельные провайдеры и записи.
- Нет подделок для сценариев 4, 7, 9 и успешного режима codex (§1); нет ни одного артефакта под macOS.
- Логи сняты на claude 2.1.322 и codex 0.166.0: на закреплённой версии переснять, сравнивать по именам полей, а не побайтно.
- Читать отчёты для работы с артефактами не нужно: условия каждого опыта записаны в шапке соответствующего скрипта.
