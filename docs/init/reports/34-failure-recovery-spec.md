# 34. Спецификация обработки сбоев и восстановления без tmux (H1)

Дата: 2026-11-10. Вторая волна: синтез отчётов 03, 13–17 (плюс 02, 19) и сырых логов `lab/{claude,codex,pi}`. Новых запросов к LLM не было; досмотрено по коду клонов в `src/` и по логам.

**Допущение о драйверах.** Оркестратор-демон ведёт claude через нативный stream-json (или ACP-адаптер с `emitRawSDKMessages` — это тот же процесс), codex через `codex app-server`, pi через `--mode rpc`. Остальные пути в матрице нужны, чтобы показать, что теряется.

**Метки.** [Э] = ПРОВЕРЕНО экспериментом первой волны (указан лог); [К] = ПО КОДУ (сигнал выведен из исходников или типов, вживую не снят); [В] = ВЫВОД; [НП] = НЕ ПРОВЕРЕНО.
**Сокращения путей.** `C:` = `lab/claude/logs/*.ndjson`, `X:` = `lab/codex/out/`, `P:` = `lab/pi/out/*.jsonl`, `src/` = `tmp/gc-alt-research/src/`.

## 0. Выводы за 60 секунд

- **Тезис владельца верен по существу: без tmux «раздупляется» всё.** Из 18 классов сбоев ни один не требует терминала. Но решает их не ACP, а связка «нативный протокол харнесса + адаптер ошибок в драйвере + запасные каналы».
- **Логи — рабочий запасной канал, но неравноценный.** codex пишет в rollout и тип ошибки, и структурное время сброса [Э]. claude пишет тип, а время сброса — только текстом без даты [Э]. pi пишет только строку ошибки [Э].
- **Логов недостаточно в пяти местах:** жив ли процесс, ждёт ли агент человека, что стало с прерванной командой, сколько квоты осталось до сбоя, сделана ли работа на самом деле.
- **Пять ловушек «успешного провала»** обязаны быть закрыты драйвером: claude `subtype:"success"` при `is_error:true`; codex-acp отдаёт 429 как `end_turn`; pi выходит с кодом 0 и `aborted:false`; pi-acp превращает ошибку в `end_turn`; pi ретраит незнакомую формулировку лимита как временный сбой.
- **Ретраи у харнессов разные:** claude — до 10 попыток с задержкой 0,5→33 с [Э]; codex не ретраит 429 вовсе [Э, К]; pi — 3 попытки от 2 с [Э]. Оркестратор не должен ретраить поверх claude и обязан ретраить за codex.
- **Квота — свойство аккаунта, а не сессии.** Нужен общий «светофор» на аккаунт и парковка до `resetsAt`, иначе десять агентов упадут по очереди.
- **tmux давал даром две вещи:** процесс переживает оркестратор, человек видит экран. Первое в v0 заменяется «убить и resume» (resume 0,4–0,5 с [Э]) плюс плановый drain; второе — `takeover/release`.
- **Смерть демона — единственный сценарий, где транскрипт обязателен:** claude дорабатывает ход без наблюдателя (17 с [Э]), и узнать исход можно только из лога и состояния репо.
- **«Сделано» определяет проверка состояния, а не слова агента.** Prompt в ACP не идемпотентен [К]; нужен журнал intent → sent → acked → settled и shipped-work gate.
- **Не проверено вживую:** исчерпание подписочной квоты у claude и codex, истечение OAuth посреди сессии, судьба дочерних команд claude после `kill -9`, поведение codex и pi при EOF посреди хода.

## 1. Единая таксономия сбоев

Драйвер приводит любой сигнал к записи `Failure{kind, scope, retry_at?, retried_by_harness, source, raw}`. `scope` определяет, кого ещё касается сбой; `source` — откуда взят класс (протокол / текст / транскрипт / ОС).

| № | kind | Определение | scope | Почему отличать |
|---|---|---|---|---|
| 1 | `quota_exhausted` | исчерпано окно подписки (5h / weekly) | аккаунт | есть время сброса; ретрай до него бессмыслен; касается всех сессий аккаунта |
| 2 | `rate_limited` | кратковременный 429 (RPM/TPM) | аккаунт + модель | секунды–минуты; лечится backoff |
| 3 | `overloaded` | 529/503 у провайдера | провайдер + модель | вина не аккаунта; смена модели помогает, смена аккаунта — нет |
| 4 | `auth_required` | нет логина, токен истёк, 401 | аккаунт | сам не пройдёт; нужен человек или refresh |
| 5 | `billing` | кредит, `insufficient_quota`, блокировка аккаунта | аккаунт | времени сброса нет; нужен человек с деньгами |
| 6 | `context_overflow` | запрос не влезает в окно | сессия | повтор в той же сессии без компакции бесполезен |
| 7 | `config_error` | модель не найдена, плохой запрос, несовместимый провайдер | профиль агента | повтор бесполезен для всех сессий профиля |
| 8 | `harness_crash` | процесс харнесса умер | сессия | контекст цел, исход последней команды неизвестен |
| 9 | `adapter_crash` | умер посредник (ACP-адаптер, общий app-server, SDK-хост) | все его сессии | радиус поражения больше одной сессии |
| 10 | `daemon_crash` | умер, перезапущен или обновлён оркестратор; сюда же перезагрузка машины | город | дети получают EOF; риск осиротевших процессов и двух писателей |
| 11 | `stall` | нет событий N минут при открытом ходе | сессия | процесс жив, ошибки нет; тишина ≠ idle |
| 12 | `runaway_turn` | ход слишком долгий или дорогой | сессия | события идут, ошибки нет; нужен бюджет |
| 13 | `waiting_for_human` | permission, вопрос, elicitation | сессия | не сбой: watchdog молчит, слот можно освободить |
| 14 | `background_wake` | активность вне хода (фоновая задача разбудила агента) | сессия | иначе сессию сочтут простаивающей и закроют |
| 15 | `tool_failure` | упал вызов инструмента (15a) или MCP-сервер (15b) | вызов / сервер | 15a — рядовое событие хода, не сбой сессии; 15b — деградация всех сессий с этим сервером |
| 16 | `transcript_divergence` | два писателя в одну сессию | сессия | молчаливая ветка, контексты расходятся |
| 17 | `workspace_failure` | диск полон, worktree сломан | риг | повтор на любом агенте упадёт так же |
| 18 | `hollow_done` | ход завершён штатно, результата нет | назначение | протокол сообщает успех; видно только по состоянию репо или бида |

Запасной вид — `unknown{text}`: считается `rate_limited` с малым числом повторов и обязательной записью сырого текста для пополнения классификатора.

## 2. Матрица сигналов

Классы 10, 16, 17, 18 наблюдает оркестратор, от харнесса они не зависят (разделы 6–7).

### 2.1 claude

| Класс | stream-json нативно | ACP v1, обычный клиент | ACP + `_meta` / raw SDK |
|---|---|---|---|
| 1 квота | `rate_limit_event{status:"rejected", rateLimitType:"five_hour"\|"seven_day", resetsAt}` + `assistant.error:"rate_limit"` с текстом из `USAGE_LIMIT_ERROR_PREFIXES`. Типизирован, время сброса есть. Вживую снят только `status:"allowed"` (`C:exp3:6`) [Э]; форма `rejected` — [К] `sdk.d.ts:6000-6026,10419` | `-32603` + `data.errorKind:"rate_limit"` — как обычный 429; отличие только по префиксу текста [К] | `usage_update._meta["_claude/rateLimit"]` с `utilization`, `resetsAt` (`C:exp5:23`) [Э для allowed] |
| 2 429 | `system/api_retry{attempt,max_retries,retry_delay_ms,error_status:429,error:"rate_limit"}` ×N → `assistant{error:"rate_limit", is_api_error_message:true}` → `result{is_error:true, api_error_status:429, terminal_reason:"api_error"}` (`C:exp7-native-429:3-6`) [Э] | `-32603 "Internal error: API Error: Request rejected (429)…"`, `data.errorKind:"rate_limit"` (`C:exp7-acp-429:23`) [Э]; ретраи не видны | `_claude/sdkMessage` несёт те же `api_retry` (`C:exp7-acp-429:18-22`) [Э] |
| 3 overloaded | `api_retry.error:"overloaded"`, финал `assistant.error:"server_error"`, `api_error_status:529` (`C:exp7-native-529:3-6`) [Э] | `errorKind:"server_error"` (`C:exp7-acp-529:22`) [Э] — 529 неотличим от 500 | статус 529 в raw |
| 4 auth | `api_retry.error:"authentication_failed"`, затем `result is_error`, 401 (`C:exp7-native-401:3-9`) [Э] | `-32000 Authentication required` (`C:exp7-acp-401:24`) [Э]; без логина — за 100 мс | то же |
| 5 billing | `assistant.error:"billing_error"\|"account_on_hold"` [К] `sdk.d.ts:3832`; [НП] | `-32603` + `errorKind` [К, НП] | то же |
| 6 контекст | `assistant.error:"invalid_request"`, `result.terminal_reason:"prompt_too_long"`, 400 (`C:exp7-native-400ctx:3-4`) [Э] | `-32603`, `errorKind:"invalid_request"`; повторно — вообще без `data` (`C:exp7-acp-400ctx:20,28`) [Э] | `terminal_reason` в raw |
| 7 конфиг | `assistant.error:"model_not_found"`; сбой старта — `result.subtype:"error_during_execution"` [К] `sdk.d.ts:6030,6059` | `-32603` [НП] | — |
| 8 краш | exit + EOF | через 9 мс `-32603 "The Claude Agent process exited unexpectedly…"` без `data`; дальше `-32002` (`C:exp4:39,41`) [Э] | то же |
| 9 адаптер | не применимо | EOF; ход дорабатывает без наблюдателя [Э, exp8c] | то же |
| 11 stall | пульса нет; признак — тишина при `session_state_changed:"running"` [К] | пульса нет; `session/prompt` может не завершиться никогда (#1445, #1284, #1269) | — |
| 12 runaway | `result.subtype:"error_max_turns"\|"error_max_budget_usd"` при заданных `maxTurns`/`maxBudgetUsd` [К] | `stopReason:"max_turn_requests"` [К, НП] | опции едут через `_meta.claudeCode.options` [К] |
| 13 ждёт | control-запрос `can_use_tool` (`--permission-prompt-tool=stdio`); `session_state_changed:"requires_action"` [К] | `session/request_permission`, `elicitation/create`; в bypass-режимах не запрашивается [К] | то же |
| 14 фон | `session_state_changed{idle\|running}` — «authoritative turn-over signal» [К] `sdk.d.ts:6271-6277` | обновления вне хода без границы; единственный маркер `_meta["_claude/origin"].kind:"task-notification"` (`C:exp5:127`) [Э]; в v2-draft — `state_update` (`C:exp10:23,31`) [Э] | маркер тот же |
| 15 tool / MCP | `tool_result.is_error`; `system/init.mcp_servers[].status` [К] | `tool_call_update status:"failed"` (форма видна на синтетическом compact-вызове, `C:exp7-acp-400ctx:25`) [Э] | — |

Ретраи claude: по умолчанию 10 попыток, задержки 0,57 → 1,0 → 2,3 → 4,9 → 9,7 → 19,9 → 32,8 с (`C:exp7-native-401:3-9`) [Э]; настраивается `CLAUDE_CODE_MAX_RETRIES`.

**Ловушки claude.**
- `result.subtype` равен `"success"` при `is_error:true` — во всех четырёх ошибочных прогонах, дошедших до `result` (429, квота, 529, контекст) [Э]. Смотреть на `is_error`, `api_error_status`, `terminal_reason`.
- 401 ретраится как временный: 7 попыток за 45 с [Э]. Истёкший логин съедает до минуты, прежде чем станет виден.
- 429 с `retry-after: 3600` не ретраится совсем: два запроса на два хода (`lab/claude/logs/exp7-fake-native-quota.log`) [Э]. При API-ключе он приходит как обычный `rate_limit` без `rate_limit_event`, а `retry-after` в `result` не передаётся [Э: в ключах `result` его нет].
- `data.errorKind` в ACP бывает пустым (вторая ошибка контекста) [Э].
- После `resume` модель сбрасывается на значение из настроек [Э, отчёт 14].

### 2.2 codex

| Класс | app-server | codex-acp v1 (± AIR) | `exec --json` |
|---|---|---|---|
| 1 квота | `account/rateLimits/updated{primary{usedPercent:100, windowDurationMins:300, resetsAt}, secondary{…10080…}}` → `error{willRetry:false, codexErrorInfo:"usageLimitExceeded"}` → `turn/completed failed`, тред `systemError` (`X:e5_run.txt`) [Э на фейке]. 1 HTTP-попытка | `-32603`, `data.codexErrorInfo:"usageLimitExceeded"`; время сброса текстом «try again at 11:23 AM» (`X:e6_run.txt`) [Э]. С AIR: `end_turn` + `sessionFailure{category:"limit", actions:[]}` (`X:e7_run.txt`) [Э] | `turn.failed{error.message}`, rc=1, без типа [Э, отчёт 15] |
| 2 429 | `codexErrorInfo:{responseTooManyFailedAttempts:{httpStatusCode:429}}`, `failed`; 1 попытка [Э] | **`stopReason:"end_turn"`**, текст ошибки как `agent_message_chunk`; след — `_meta.codex.threadStatus.type:"systemError"`. С AIR: `actions:["retry"]` [Э] | то же, текст |
| 3 overloaded | `"serverOverloaded"` через 3,3 с, 5 попыток [Э] | `end_turn` + текст [Э] | текст |
| 4 auth | 5 × `error{willRetry:true, responseStreamDisconnected:{401}}`, затем `httpConnectionFailed:{401}` через 6,6 с [Э]; вариант `"unauthorized"` [К] | `-32000 Authentication required` [Э] | текст |
| 5 billing | `usage_not_included`, `insufficient_quota` разбираются в `codex-api/src/api_bridge.rs:217-262` [К]; [НП] | [НП] | текст |
| 6 контекст | `"contextWindowExceeded"` [Э] | `end_turn` + текст; `usage_update used == size` [Э] | текст |
| 7 конфиг | `"modelNotFound"`, `"badRequest"` [К]; отказ `modelProviderMismatch` (`X:e5.jsonl`) [Э] | [НП] | текст |
| 8 краш | EOF за 0,00 с [Э, E3] | `error{code:1001, data:{signal:"SIGKILL", restartable:true}}`; адаптер сам поднимает app-server [Э, E4] | код выхода |
| 9 адаптер | общий app-server сам играет эту роль: EOF рвёт все треды | EOF, потомков не остаётся [Э] | — |
| 11 stall | пульса нет; активность — `item/*`, `thread/status/changed`. Зависший сервер держит writer-lock (#41849) | `session/cancel` может потеряться (#641) | — |
| 12 runaway | `"sessionBudgetExceeded"` [К]; иначе watchdog + `turn/interrupt` [Э] | `session/cancel` | SIGINT |
| 13 ждёт | серверные запросы `item/commandExecution/requestApproval`, `item/tool/requestUserInput`, `mcpServer/elicitation/request`; статус `active{activeFlags:["waitingOnApproval"\|"waitingOnUserInput"]}` [К] `lab/codex/schema-ts/v2/ThreadActiveFlag.ts` | `session/request_permission` | нет |
| 14 фон | `thread/status/changed{idle\|active}` [Э] | нет границы [В] | — |
| 15 tool / MCP | `item/completed` с кодом; `mcpServer/startupStatus/updated{starting\|ready\|failed\|cancelled}` [К] | `tool_call_update` | события |

Ретраи codex: `retry_429:false, retry_5xx:true, retry_transport:true` (`src/codex/codex-rs/model-provider-info/src/lib.rs:502-504`) [К]; число попыток 4–5.

**Ловушки codex.** В ACP v1 429, 503 и переполнение контекста неотличимы от успешного хода [Э]. В самой ошибке нет `resetsAt` — он только в снимке `rateLimits`. У владельца через прокси `account/rateLimits/read` отвечает `-32600`, а в реальных ходах `primary/secondary = null` [Э]. `exec --json` типа ошибки не несёт.

### 2.3 pi

| Класс | `--mode rpc` | SDK в процессе | pi-acp 0.0.34 |
|---|---|---|---|
| 1 квота | типа нет: `message_end{stopReason:"error", errorMessage:"429: {…usage_limit_reached…}"}`. **Незнакомая формулировка ретраится как временная** (`P:e6-sublimit:13`) [Э]. `resets_in_seconds` — только внутри строки | те же события [К] | `end_turn` [К] `src/pi-acp-svkozak/src/acp/agent.ts:892` |
| 2 429 | `agent_end{willRetry:true}` → `auto_retry_start{attempt, maxAttempts:3, delayMs, errorMessage}` → `auto_retry_end{success}` → `agent_settled` (`P:e6-429:13`) [Э] | то же | скрыто |
| 3 overloaded | то же (`P:e6-529:13`) [Э] | то же | скрыто |
| 4 auth | `stopReason:"error"`, `errorMessage:"401: …"`, без ретрая, `agent_settled{aborted:false}` (`P:e6-auth:9-13`) [Э] | то же | `end_turn` |
| 5 billing | `insufficient_quota` — без ретрая (`P:e6-quota:9-13`) [Э] | то же | `end_turn` |
| 6 контекст | `stopReason:"error"`, 400 (`P:e6-overflow`) [Э]; штатно `compaction_start{reason:"overflow"}` [К] | то же | `end_turn` |
| 8 краш | выход процесса, EOF [Э, e7] | падает хост — все сессии сразу [В] | `session/prompt` не завершается никогда (#160) [Э, e5] |
| 11 stall | пульса нет; активность — `message_update` | то же | — |
| 12 runaway | watchdog + `abort` (7 мс) [Э] | `abort()` | `session/cancel` (10 мс) [Э] |
| 13 ждёт | встроенных подтверждений нет; диалоги расширений — под-протокол extension UI [К] | колбэки | запросов не бывает [Э] |
| 14 фон | `agent_settled` — единственная надёжная граница [Э] | то же | нет |
| 15 tool / MCP | `tool_execution_end`; событие расширений `mcp_servers_change` [К] | то же | — |

Ретраи pi: 3 попытки, база 2 с, потолок 60 с; классификация двумя регулярками по тексту (`src/pi-mono/packages/ai/src/utils/retry.ts:7-52`) [К].

**Ловушки pi.** Код выхода 0 и `agent_settled.aborted:false` при провале хода [Э]. Признак провала — `stopReason:"error"` последнего сообщения ассистента. Истёкший логин как событие не существует.

## 3. Запасные каналы: что дают логи

### 3.1 Транскрипты

| Источник | Что читается достоверно | Чего нет |
|---|---|---|
| claude `~/.claude/projects/<cwd>/<id>.jsonl` | `{"type":"assistant","error":"rate_limit","isApiErrorMessage":true,"apiErrorStatus":429,"message":{"model":"<synthetic>"}}` и записи `type:"system"` на каждую попытку с `error.status` (`lab/claude/cfgFake/projects/…/02491a51-….jsonl:19,25`) [Э]. Квота подписки: «You've hit your session limit · resets 6:20am (Asia/Bangkok)» [Э, отчёт 14] | время сброса — текст без даты; `rate_limit_event` в транскрипте не найден, поле `error.rateLimits` в записях попыток на фейке равно `null`, под подпиской — [НП] |
| codex rollout `$CODEX_HOME/sessions/…/rollout-*.jsonl` | `event_msg{type:"task_complete", error:{codex_error_info:"usage_limit_exceeded"}}` и **`event_msg{type:"token_count", rate_limits:{primary:{used_percent:100.0, window_minutes:300, resets_at:1794284533}}}`** (`lab/codex/home-fake/sessions/2026/11/10/rollout-…4a15….jsonl:14-15`) [Э]. Пять видов `codex_error_info` в lab-rollout'ах | имена в snake_case, не как в app-server; формат хранения мигрирует на sqlite [отчёт 15] |
| pi `<agent-dir>/sessions/…/<ts>_<id>.jsonl` | `message.stopReason:"error"` + `errorMessage` (`lab/pi/agent-dir/sessions/…dead0a56663e.jsonl:17`) [Э на abort]; для квоты — [В] | типа и времени сброса нет |

Находка второй волны: rollout codex, записанный **под ACP-адаптером** (прогон E6, `rollout-…3ca2….jsonl:14`), содержит структурный `resets_at`, который сам адаптер клиенту не отдаёт [Э]. Для codex транскрипт полностью закрывает дыру ACP.

Когда транскрипт нужен: (1) подтвердить класс, если протокол дал текст или `end_turn`; (2) после смерти демона узнать, чем кончился ход без наблюдателя; (3) найти последний незавершённый tool call для reconcile-промпта; (4) проверить, дошёл ли prompt (раздел 7); (5) аудит.

### 3.2 Внешние источники квоты

| Источник | Что даёт | Надёжность |
|---|---|---|
| claude `rate_limit_event` / `_claude/rateLimit` | `status: allowed\|allowed_warning\|rejected`, `utilization`, `resetsAt` по окнам | надёжно, пока идут ходы и вход по подписке; в простое не приходит; мейнтейнер адаптера называет API экспериментальным (#625) |
| codex `account/rateLimits/read` и `…/updated` | `usedPercent`, `windowDurationMins`, `resetsAt` | надёжно при ChatGPT-логине; у владельца через прокси недоступно [Э] |
| недокументированные usage API, как в caam: `api.anthropic.com/api/oauth/usage` (заголовок `anthropic-beta: oauth-2025-04-20`) и `chatgpt.com/backend-api/wham/usage` (`src/coding_agent_account_manager/internal/usage/claude.go:14,300-301`, `codex.go:20-21,144-150`) [К] | окна и время сброса без запуска хода | ненадёжно: схема менялась (комментарии про issue #97), OAuth-токен используется вне официального бинарника — риск по политике вендора. Только как выключенный по умолчанию советчик |
| pi | ничего | — |

### 3.3 Насколько верен тезис «логи есть — tmux не нужен»

Верен в части «tmux не нужен»: всё, что Gas City вычитывает с экрана, есть в протоколе или транскрипте в более точном виде. Неточен в части «логов хватит»:

1. **Живость и зависание.** Молчащий лог не отличает работу от смерти (урок gascity#6748). Нужны `wait(2)`/EOF и watchdog по событиям.
2. **Ожидание человека.** Запрос разрешения живёт в памяти процесса и в транскрипт попадает после ответа [В].
3. **Исход прерванной команды.** В логе — tool call без результата [Э, pi e7; codex E3: модель отвечает UNKNOWN]. Истина — в репо.
4. **Квота до сбоя.** Светофору нужен `utilization` заранее; он идёт событиями, не логом (кроме codex).
5. **Время сброса.** claude — текст без даты, pi — ничего.
6. **Результат работы.** Лог фиксирует слова агента, а не факт.

## 4. Машины состояний

### 4.1 Сессия

```
 spawn ─► starting ──init ok──► idle ◄──────────── turn_end(ok) ───────────────┐
             │                   │  ▲                                           │
             │ init fail         │  └── respond / timeout→ответ по умолчанию ─┐ │
             ▼                   ▼ prompt (intent→sent)                        │ │
        failed{kind} ◄────── running ──request_permission/question──► requires_action
             │    turn_end(err) │ ▲
             │    exit / EOF    │ └── background_wake (из idle, без назначения)
             │    stall/runaway
             ├─ retry ──────► recovering ──resume ok──► idle (+ reconcile-prompt ► running)
             ├─ park ───────► parked{until, reason, scope} ──таймер ∧ светофор зелёный──► recovering
             ├─ quarantine ─► parked{until: manual}
             └─ give up ────► closed
 idle | parked | requires_action ──cmd takeover──► takeover{by} ──cmd release──► recovering
 idle ──idle_ttl / cmd close──► closed ──есть работа──► starting (resume по id харнесса)
```

Правила:
- Граница `running → idle` берётся из авторитетного сигнала: claude `session_state_changed` / `result`, codex `turn/completed` + `thread/status/changed`, pi `agent_settled`. Ответ на `session/prompt` в ACP v1 границей не считается (класс 14).
- `parked` и `takeover` означают, что процесса харнесса у демона нет: память и слот свободны, идентификатор сессии сохранён.
- `recovering` всегда начинается с проверки «нет живого писателя» и чтения хвоста транскрипта.
- Каждое вхождение в `starting`/`recovering` получает новое поколение (`generation`); события старого поколения отбрасываются.

### 4.2 Назначение работы (bead ↔ session, generation)

```
 ready ──claim(gen=g)──► claimed ──журнал: intent──► dispatching ──sent, acked──► in_turn
 in_turn ──turn_end(ok)──► verifying ──gate pass──► settled
                               └─gate fail──► hollow ──► redeliver(«не сделано: …») | escalated
 in_turn ──сессия failed/parked──► suspended{reason, until}        (claim удержан)
 suspended ──сессия восстановлена──► redeliver (та же сессия, reconcile-prompt)
 suspended ──resume не удался N раз──► requeued (claim снят, gen+1, новая сессия + handoff)
 любое ──attempts > max ∨ kind ∈ {config_error, billing}──► escalated (human-гейт на биде)
 escalated ──abort──► failed   (правило формулы: остальные шаги skipped, шаги `always` идут)
```

| Сбой сессии | Назначение | Счётчик attempt |
|---|---|---|
| 1, 2, 3 (лимиты, перегрузка) | `suspended` до `retry_at`, затем та же сессия | не растёт: вина не агента |
| 4, 5 (auth, billing) | `suspended` + немедленная эскалация | не растёт |
| 6 контекст | компакция и повтор; не вышло — `requeued` с handoff | +1 |
| 7 конфиг | `escalated`, профиль в карантин | — |
| 8, 9, 10 (краши) | `redeliver` с reconcile-промптом | +1 |
| 11, 12 | interrupt → `redeliver`; повторно — `requeued` | +1 |
| 13 ждёт | остаётся `in_turn`, флаг blocked; по таймеру эскалация | — |
| 16 расхождение | сессия в карантин, `requeued` | — |
| 17 workspace | `suspended` для всего рига + эскалация | не растёт |
| 18 hollow | `redeliver` с перечнем несделанного | +1 |

## 5. Политики реакции

| Класс | Действие по умолчанию | Параметры | Транспорт или суждение |
|---|---|---|---|
| 1 квота | светофор аккаунта красный до `resetsAt` + джиттер; сессии аккаунта → `parked`, слоты свободны; после сброса — один пробный ход, затем открытие | `reset_margin`, `probe`, порог жёлтого по `utilization` (0,9), `weekly → escalate` | ожидание и парковка — транспорт; failover — суждение |
| 2 429 | claude, pi: не ретраить сверху, дождаться исчерпания их ретраев; codex: ретраит оркестратор. Backoff `min(10 с · 2^(n−1), 5 мин)` (Symphony §8.4, `lab/landscape/symphony_SPEC.md:799-801`) | база, потолок, `max_attempts` | транспорт |
| 3 overloaded | тот же backoff; светофор на пару провайдер + модель, не на аккаунт | `fallback_model` | транспорт; смена модели — конфиг |
| 4 auth | светофор аккаунта красный без таймера; эскалация; предстартовый `auth check` | канал уведомления | транспорт |
| 5 billing | как 4 | — | транспорт |
| 6 контекст | компакция; не помогла — fresh с handoff-сводкой | порог превентивной компакции | сводку пишет агент |
| 7 конфиг | карантин профиля, эскалация | — | транспорт |
| 8–10 краши | resume + reconcile; смерть раньше 30 с = rapid crash; 5 подряд → карантин сессии; 3 неудачных resume → fresh с handoff (пороги Gas City, отчёт 03) | `rapid_window`, `crash_loop_n`, `resume_fail_n` | пороги — конфиг |
| 11 stall | interrupt; нет подтверждения 10 с → kill группы → сценарий (а) | `stall_timeout` (5 мин, Symphony §8.5), отдельный таймаут на идущий tool call | транспорт |
| 12 runaway | жёсткий бюджет: `maxTurns`/`maxBudgetUsd` у claude, wall-clock и токены у остальных; interrupt, эскалация | бюджеты на шаг | величина бюджета — конфиг |
| 13 ждёт | уведомление сразу, напоминания с backoff; по таймауту — ответ по умолчанию (deny) или cancel + human-гейт | `human_timeout`, `default_answer`, `hold_slot` | транспорт |
| 14 фон | `idle → running(background)`; idle-TTL не тикает; бюджет фона | `background_budget` | транспорт |
| 15b MCP | рестарт общего сервера супервизором; сессиям — уведомление | health-интервал | транспорт |
| 16 | запрет: один писатель; обнаружено — карантин сессии | — | транспорт |
| 17 | стоп выдачи на риг, эскалация | порог свободного места | транспорт |
| 18 hollow | shipped-work gate; N провалов → эскалация | проверка на шаг | проверку задаёт формула |

**Светофор (circuit breaker).** Ключ — `(харнесс, аккаунт)` для классов 1, 4, 5 и `(провайдер, модель)` для 2, 3. Первый же сигнал закрывает выдачу всем сессиям ключа; последующие однотипные сбои уже идущих ходов поглощаются без счёта попыток и без уведомлений. Состояния: green → yellow (`allowed_warning` или порог) → red{until} → half-open (один пробный ход) → green.

**Failover как конфиг** — упорядоченный список целей на роль; оркестратор только исполняет.

| Харнесс | Что технически нужно | Ограничения |
|---|---|---|
| claude | `CLAUDE_CONFIG_DIR` на сессию — работает, но учётные данные надо доставить (`-32000` без них) [Э, exp6]; `CLAUDE_CODE_OAUTH_TOKEN` из `claude setup-token`; для API — `ANTHROPIC_API_KEY`; `fallbackModel` | лимиты подписки рассчитаны на «ordinary, individual usage»; ротация нескольких подписок ради обхода лимита — зона риска. Failover на API-ключ чист |
| codex | `CODEX_HOME` на процесс: другой аккаунт = другой app-server; провайдер тоже на процесс (`modelProviderMismatch`) [Э]. Rollout лежит в `CODEX_HOME`, перенос треда между аккаунтами [НП] | app-server-аутентификация подпиской «has never been permitted for commercial or hosted services» |
| pi | провайдер и модель — на сессию и на лету: rpc-команда `set_model{provider, modelId}` [К], [НП вживую] | Claude по подписке через pi биллится отдельно |
| межхарнессный | новая сессия на другом харнессе + handoff-сводка; транскрипты не переносимы [В] | — |

**Что оставить суждению** (конфиг или агент-координатор): переключаться ли на платный API; ждать или эскалировать при недельном лимите; бросить ли шаг после N попыток; что писать в handoff; принять ли частичный результат. **Что транспорт:** классификация, таймеры, светофор, парковка, kill, resume, журнал, запуск проверки. Правило «упёрлись в лимит — ждём до сброса» — управление потоком, а не суждение.

## 6. Восстановление по сценариям

### (а) `kill -9` харнесса посреди tool call

| | claude | codex | pi |
|---|---|---|---|
| Обнаружение | exit + EOF; в ACP `-32603` через 9 мс [Э] | EOF за 0,00 с [Э] | выход процесса [Э] |
| Дочерние команды | [НП]: exp4 судьбу `sleep 40` не проверял | умирают: `PR_SET_PDEATHSIG` (`src/codex/codex-rs/core/src/spawn.rs:117`), только Linux [Э + К] | сирота [Э]; `bash` запускается с `detached:true` (`src/pi-mono/packages/coding-agent/src/core/tools/bash.ts:127`) — своя группа, `kill(-pgid)` харнесса не достанет [К] |
| Resume | `session/resume` 0,4 с, контекст цел [Э]; нативно `--resume <id>` [Э] | `thread/resume` → `idle`, ход `interrupted` [Э] | `--session <id>`, `isStreaming:false` [Э] |
| После resume | модель сброшена — переустановить [Э] | тред без сообщений теряется [К] | ход сам не продолжается [Э] |

Шаги: (1) драйвер выдаёт `harness_crash{exit, signal}`; (2) демон убивает cgroup сессии — группы процессов недостаточно из-за pi; (3) назначение → `suspended`; (4) счётчик crash-loop; (5) resume; (6) reconcile-промпт: «Предыдущий ход прерван сбоем процесса. Результат последней команды `<команда из журнала>` неизвестен: она могла выполниться, выполниться частично или не начаться. Сначала сверь состояние (git status, артефакты, бид), затем продолжай шаг `<step>`»; (7) attempt + 1.

### (б) Смерть адаптера

claude-agent-acp: клиент видит EOF; простаивающий `claude` выходит меньше чем за секунду, а процесс в середине хода дорабатывает ход и выходит (17 с) [Э, exp8c]. Новый адаптер делает `session/load` за 0,5 с [Э]. Опасность: resume раньше, чем старый процесс вышел, даёт двух писателей — сначала дождаться выхода PID или убить. codex-acp: потомков не остаётся, `session/load` реплеит [Э]. Общий app-server: падение рвёт все ходы и убивает их команды [Э]. При нативных драйверах claude и pi класс исчезает.

### (в) Смерть, рестарт, апгрейд демона

Дети на stdio получают EOF:
- **claude** — дорабатывает ход без наблюдателя и выходит; простаивающий выходит сразу [Э, exp8c: для процесса `claude` смерть адаптера и смерть демона — один и тот же EOF]. Побочные эффекты хода происходят.
- **codex app-server** — EOF в одноклиентском режиме завершает сервер: до 30 с на слив RPC, жёсткий выход через 45 с (`src/codex/codex-rs/app-server-transport/src/transport/stdio.rs:124-133,168-181`; `app-server/src/lib.rs:1235-1240`) [К]. Судьба идущего хода — [НП].
- **pi rpc** — `stdin end` → `shutdown()` → `runtimeHost.dispose()` → `exit(0)` (`src/pi-mono/packages/coding-agent/src/modes/rpc/rpc-mode.ts:744-756,826-829`) [К]. Судьба идущей команды — [НП].

После старта демон: (1) читает реестр сессий из state (id харнесса, pid, время старта pid, cgroup, generation); (2) сверяет с таблицей процессов — реестр подсказка, процессы истина: метка сессии в env ребёнка (`/proc/<pid>/environ`), у claude `--session-id=<uuid>` виден в cmdline [Э, exp4]; (3) осиротевшим даёт до T секунд выйти самим, затем убивает cgroup; (4) только после этого делает resume — так исключаются два писателя (у codex страхует writer-lock: `thread … already has an active writer` [Э, E10]; у claude и pi блокировок нет [Э, К]); (5) читает хвост транскрипта: ход завершён → проверка результата, не завершён → reconcile-промпт.

| | Убить всех и resume | Detached-хост на сессию | Внешний `codex app-server` |
|---|---|---|---|
| Идущий ход при рестарте демона | теряется или дорабатывается вслепую | переживает | переживает (только codex) |
| Новые компоненты | нет | хост-процесс, протокол демон↔хост, fence по generation (образец: `lab/landscape/ao_architecture.md:10-13,347,506`) | unit супервизора; WebSocket поверх UDS; `[experimental]` |
| Подключение человека | через takeover | второй клиент к хосту | `codex --remote` [Э, E8] |
| Риск | промах prompt-кэша [НП] | хост должен быть «скучнее» демона | один радиус поражения на все треды |

Рекомендация: v0 — «убить и resume» плюс **плановый drain** при апгрейде (остановить выдачу, дождаться конца ходов до N минут, затем рестарт). У владельца сессии живут десятки секунд, потеря хода дешева. Внешний app-server — когда доля codex вырастет; detached-хост — только если измерения покажут, что незапланированные падения демона дороги.

### (г) Перезагрузка машины

Всё мертво, сирот нет. Порядок старта: state (SQLite WAL восстанавливается сам) → общие MCP-сервисы → предстартовый `auth check` по аккаунтам → resume сессий с назначениями в `in_turn` через reconcile-промпт. Демон — unit системы. tmux перезагрузку тоже не переживал.

### (д) Зависший ход

Watchdog считает время от последнего события хода, отдельно для «модель молчит» и «идёт tool call». Лестница: interrupt (`session/cancel` 16 мс [Э], `turn/interrupt` [Э], `abort` 7 мс [Э]) → нет подтверждения за 10 с → kill cgroup → сценарий (а). Обязательно из-за багов адаптеров (#1445, #1284, #1269, #641). Тишина никогда не трактуется как idle.

### (е) Агент ждёт человека часами

Состояние `requires_action`: watchdog и idle-TTL выключены. Уведомление сразу, затем с backoff. По `human_timeout` — ответ по умолчанию (deny) либо cancel хода и human-гейт на биде. Запрос живёт в памяти процесса, поэтому парковка с убийством процесса теряет его; переспросит ли харнесс после resume — [НП]. В bypass-режимах запросов разрешений нет; `AskUserQuestion` адаптер claude отключает сам (`--disallowedTools=AskUserQuestion` [Э, exp4]).

### (ж) Квота исчерпана у всех агентов аккаунта

(1) Первый `quota_exhausted` → светофор красный до `resetsAt`. (2) Остальные идущие ходы упадут с тем же классом — поглощаются. (3) Новая выдача на аккаунт закрыта; сессии → `parked`, процессы закрыты. (4) Назначения → `suspended`, claim удержан: шторма переочереди нет. (5) Если `resetsAt` дальше порога (недельное окно) — эскалация человеку с ETA и, по конфигу, failover. (6) После сброса — один пробный ход, затем постепенное открытие. Источник `resetsAt`: claude — `rate_limit_event`; codex — `rateLimits` или rollout; pi и codex через прокси — нет, тогда backoff с потолком и пробами.

## 7. Идемпотентность и «exactly-once»

Протокол не помогает: идентификатор сообщения в ACP — «an acknowledgment, not a client-supplied idempotency key» (`src/agent-client-protocol/docs/rfds/v2/prompt.mdx:391-393`) [К]. Гарантию строит оркестратор.

1. **Ключ** `(run, step, attempt)` плюс `generation` claim'а. Маркер ключа вставляется в текст промпта.
2. **Журнал в state:** `intent` (записан до отправки) → `sent` → `acked` → `settled`. Подтверждение приёма: claude — эхо пользовательского сообщения (`--replay-user-messages` [Э, exp4]) или `messageId` в v2; codex — `turn.id` в ответе на `turn/start` (`X:e5_usage.jsonl:10`) [Э]; pi — `{"disposition":"started"}` [Э].
3. **Восстановление по журналу:** `intent` без `sent` — отправить; `sent` без `acked` — искать маркер в транскрипте, нет — отправить снова; `acked` без `settled` — читать хвост транскрипта и идти в проверку или reconcile.
4. **Один получатель:** claim атомарен (в state либо `br update --claim`); результат от устаревшего `generation` отбрасывается.
5. **Shipped-work gate:** шаг закрыт, когда прошла проверка состояния — коммит на ветке, статус бида, артефакт, exec-проверка формулы. Слова агента «done» — только повод запустить проверку. Образцы: NEEDLE, claw-orchestrator (отчёт 17).
6. **Безопасная повторная выдача:** промпт шага формулируется как «добейся состояния X», а не «сделай действие Y», и требует сначала сверить текущее состояние.

Итог — не exactly-once доставка, а at-least-once доставка с идемпотентным эффектом и проверкой по факту.

## 8. Чего мы лишаемся без tmux

| Что давал tmux | Чем закрыто | Остаточный риск |
|---|---|---|
| Процесс переживает оркестратор | resume за 0,4–0,5 с [Э] + плановый drain; позже внешний app-server или detached-хост | незапланированное падение демона обрывает ходы; цена промаха кэша [НП] |
| Человек видит экран | журнал событий (`tail`), `takeover/release` в родной TUI, для codex `--remote` | нет «живого» вмешательства в claude и pi без takeover |
| Диалог login | предстартовый `claude auth status --json`, `codex login status`, `pi auth check`; токен из `claude setup-token` | истечение OAuth посреди сессии [НП]; первичный логин — руками |
| Диалог trust | claude: `settingSources` и права параметрами сессии; codex: `config.bypass_hook_trust` [Э, E3], флаг `--dangerously-bypass-hook-trust` [К]; pi: `--approve` либо ресурсы в своём каталоге агента | новый диалог в новой версии харнесса проявится как stall |
| Диалог смены модели | модель задаётся параметром; у claude переустанавливать после resume [Э] | #1056 |
| Любой CLI | только харнессы с протоколом | владельцу не нужно |
| Отладка глазами | захват протокола + транскрипт | нужен просмотрщик |

## 9. Контрактные тесты драйверов

Прогоняются на каждое обновление харнесса или адаптера; провайдер — фейк (`lab/claude/fake-api.mjs`, `lab/codex/scripts/fake_provider.py`, `lab/pi/scripts/mock-openai.mjs`), LLM не нужен.

| № | Сценарий | Ожидание от драйвера |
|---|---|---|
| 1 | 429 ×2, затем успех | ход успешен; попытки видны как события |
| 2 | 429 бесконечно | `rate_limited`, `retried_by_harness` верен; не «успех» |
| 3 | квота с временем сброса | `quota_exhausted`, `retry_at` = сбросу |
| 4 | квота незнакомой формулировки | не ретраится как временная; хуже `unknown` быть не должно |
| 5 | 529 / 503 | `overloaded`, отличим от 429 |
| 6 | 401 | `auth_required` быстрее заданного порога |
| 7 | `insufficient_quota` | `billing` |
| 8 | переполнение контекста | `context_overflow` |
| 9 | несуществующая модель | `config_error` |
| 10 | `kill -9` харнесса посреди команды | `harness_crash` ≤ 1 с; после kill cgroup сирот нет; resume сохраняет память |
| 11 | EOF на stdin посреди хода | процесс выходит за ограниченное время; исход читается из транскрипта |
| 12 | провайдер молчит 30 с | `stall` по watchdog; interrupt подтверждён |
| 13 | запрос разрешения | `requires_action`; ответ доходит; таймаут даёт ответ по умолчанию |
| 14 | фоновая задача после конца хода | состояние `running(background)`, не idle |
| 15 | второй писатель в ту же сессию | отказ либо обнаружение до записи |

Тесты 3, 4, 7, 9, 11, 13, 14 частично опираются на непроверенное; первый прогон покажет реальные формы.

## 10. Вердикт по H1

Значения: **П** — решено протоколом (типизированный сигнал); **Д** — решено драйвером с адаптером ошибок (текст, сочетание сигналов, watchdog); **З** — требует запасного канала (транскрипт, таблица процессов, состояние репо); **Н** — не решено.

| Класс | claude (stream-json) | codex (app-server) | pi (rpc) |
|---|---|---|---|
| 1 `quota_exhausted` | Д; вживую [НП] | П; через прокси владельца З (rollout) или Н | Д по классу, Н по времени сброса |
| 2 `rate_limited` | П | П (ретраит оркестратор) | Д |
| 3 `overloaded` | П | П | Д |
| 4 `auth_required` | П | Д | Д |
| 5 `billing` | Д [НП] | Д [НП] | Д |
| 6 `context_overflow` | П | П | Д |
| 7 `config_error` | Д | П | Д |
| 8 `harness_crash` | П (ОС) + З (исход команды) | П + З | П + З |
| 9 `adapter_crash` | класса нет | П | класса нет |
| 10 `daemon_crash` | З | З | З |
| 11 `stall` | Д | Д | Д |
| 12 `runaway_turn` | П | Д | Д |
| 13 `waiting_for_human` | П | П | П (подтверждений нет) |
| 14 `background_wake` | П | П | П |
| 15 `tool_failure` / MCP | П | П | Д |
| 16 `transcript_divergence` | З (дисциплина демона) | П (writer-lock) | З |
| 17 `workspace_failure` | Д | Д | Д |
| 18 `hollow_done` | З | З | З |

Для сравнения, **если бы транспортом был только стандартный ACP v1:** классы 1–3 и 6 у codex и все ошибки хода у pi — **Н** (неотличимы от успеха); класс 14 у claude — **Н**.

**H1 — подтверждена в части «tmux не нужен», опровергнута в части «всё решается через ACP».** Без tmux нет ни одного класса с итогом «не решено», кроме времени сброса квоты у pi и у codex за прокси; там остаётся backoff с пробами. Цена — адаптер ошибок на харнесс, реестр сессий со сверкой по таблице процессов и проверка результата по состоянию.

## 11. Открытые вопросы и непроверенное

- Форма исчерпания подписочной квоты вживую: claude (`rate_limit_event{status:"rejected"}`) и codex (ChatGPT-логин) — только по коду и фейку.
- Истечение OAuth посреди долгой сессии у всех трёх.
- Судьба дочерних команд claude после `kill -9`; у codex вне Linux.
- codex и pi при EOF посреди хода: что с идущей командой и записью в транскрипт.
- Переспрашивает ли харнесс разрешение после resume.
- Цена resume в токенах (промах prompt-кэша).
- Падение MCP-сервера посреди сессии — сигналы только по схеме.
- Перенос треда codex между `CODEX_HOME`; `set_model` у pi как failover.
- Долгие ходы и субагенты под адаптером claude (#1445, #1284) не воспроизводились.
- Пороги (5 мин stall, 30 с rapid crash, 3 resume) взяты у Symphony и Gas City, под город владельца не калиброваны.
