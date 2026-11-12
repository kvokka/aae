# 02. Драйвер codex: собственный `codex app-server` — шпаргалка

Стенд всех фактов: codex-cli **0.166.0**, Linux aarch64 (OrbStack), опыты 2026-11-10 и 2026-11-11; схема протокола перегенерирована и сверена 2026-11-12.
**Оговорка стенда:** codex ходил в модель через кастомный `model_provider` (прокси, `env_key`), `codex login status` → `Not logged in`.
Логин ChatGPT-подпиской и её лимиты **вживую не проверялись ни разу**; все сбои сняты на поддельном провайдере **[П]** R15:8-10.

**Метки:** **[П]** проверено опытом (указан лог), **[К]** по коду/схеме/документации вендора, **[НП]** не проверено.
**Источники:** `R15:68` = `reports/15-harness-codex-acp.md:68`; `R34` = `34-failure-recovery-spec.md`, `R36` = `36-experiments-…md`,
`R42` = `42-coldstart-floor.md`, `R46b` = `46b-observability-codex.md`. Пути без `R` — от `lab/`. `X/` = `lab/codex/out/`, `W/` = `lab/wave2-exp/logs/`,
`OC/` = `lab/observability/codex/out/`. `SCHEMA` = вывод `codex app-server generate-json-schema` 0.166.0 от 2026-11-12 (не сохранён; перегенерировать).

## 1. Запуск и транспорт

| Что | Факт | Метка, источник |
|---|---|---|
| команда | `codex app-server [--listen URL] [-c key=value …]`; подкоманда помечена `[experimental]` | [К] `codex app-server --help` |
| транспорты | `--listen stdio://` (умолчание) \| `unix://` \| `unix://PATH` \| `ws://IP:PORT` \| `off` | [К] там же |
| stdio | JSONL, по одному JSON-сообщению в строке | [П] `codex/scripts/asc.py:50-112` |
| unix-сокет | говорит **WebSocket** (HTTP Upgrade), не сырой JSONL. Рукопожатие `GET / HTTP/1.1` + `Upgrade: websocket` + `Sec-WebSocket-Key` → `101`; текстовые фреймы с маской, по одному JSON на сообщение; клиент лаборатории отвечал pong на ping (opcode 9) и собирал фрагменты | [П] `codex/scripts/asc.py:174-262` (рабочий клиент), R15:69 |
| каталог сокета | обязан быть `0700` и принадлежать пользователю, иначе `socket parent must be owned by the user…` | [П] R15:70-71 (E2) |
| статус WebSocket | «WebSocket transport is experimental and unsupported» | [К] R15:82 |
| `codex app-server proxy` | байтовый мост stdio ↔ управляющий сокет общего демона — драйверу не нужен | [К] R15:70 |
| старт | `initialize` отвечает за 77 мс [72–116] от spawn, n=6 (замер 2026-11-10, codex 0.166.0) | [П] R42:43 |
| побочное при старте | порождает `git ls-remote https://github.com/openai/plugins…` (≈30 МБ, кратко); на первый ход — login-shell пользователя для снимка окружения (`zsh -lc`, ≈145 МБ на пару секунд; замер 2026-11-10, codex 0.166.0) | [П] R15:135-137, R46b:121 |
| аналитика | у app-server по умолчанию выключена; метрики по умолчанию уходят в statsig OpenAI → ставить `[analytics] enabled=false` в конфиге `CODEX_HOME` | [К] R46b:37-39,105 |
<!-- MEASURE id=codex-startup-transients date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/15,lab/codex/out/e4_run2.txt note="RSS транзиентных дочерних процессов; зависит от shell и rc-файлов пользователя" -->
<!-- MEASURE id=codex-appserver-initialize date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/42,lab/coldstart/out/floors.jsonl note="stdio, от spawn до ответа initialize, n=6; на unix-сокете не мерили" -->

**Общий демон codex пользователя не использовать:** сам обновляется и перезапускается, сокет в `/tmp`, на стенде был сломан **[П]** R15:104-112.

**Схема протокола.** `codex app-server generate-json-schema --out DIR [--experimental]` (и `generate-ts`); вывод привязан к версии бинарника **[К]** R15:79-81.
Сверка 2026-11-12, codex 0.166.0 **[П]**: стабильный набор — 106 клиентских запросов, 85 нотификаций, 10 серверных запросов; с `--experimental` — 179 клиентских запросов.
<!-- MEASURE id=codex-schema-size date=2026-11-12 ver=codex-0.166.0 env=linux-arm64-orbstack src=вывод-generate-json-schema note="подсчёт enum method в ClientRequest.json/ServerNotification.json/ServerRequest.json; бандл 4,5 МБ (5,8 МБ с --experimental); пересчитать при обновлении codex" -->
Все методы из §3 есть в стабильном наборе. Только с `--experimental`: `thread/queue/*`, `server/diagnostics`, `thread/backgroundTerminals/*`, `thread/settings/update`.

## 2. Каркас JSON-RPC

- Поля `jsonrpc` **нет** ни в одну сторону **[П]** `X/e5_usage.jsonl:1-2`.
- Запрос клиента: `{"id":N,"method":…,"params":…}`; ответ: `{"id":N,"result":…}` или `{"id":N,"error":{"code":-32600,"message":…,"data":…}}`.
- Нотификация сервера: `{"method":…,"params":…,"emittedAtMs":1794280933953}`.
- **Запрос сервера к клиенту** имеет и `method`, и `id`; нумерация своя, **начинается с 0** — проверять наличие ключа `id`, а не его истинность **[П]** `W/exp3-orchestrator.jsonl:48`.
- После `initialize` клиент шлёт нотификацию `{"method":"initialized"}` **[П]** `X/e5_usage.jsonl:3`.
- Сразу после `initialize` приходят `configWarning` и `remoteControl/status/changed` — не ошибка **[П]** `X/e5_usage.jsonl:5-6`.
- Незнакомые нотификации (`thread/readState/changed`, `turn/moderationMetadata`, `thread/goal/cleared`, `deprecationNotice`, …) пропускать.

## 3. Минимальное подмножество (примеры — из логов)

### 3.1 `initialize`
```json
> {"id":1,"method":"initialize","params":{"clientInfo":{"name":"gc-alt-lab","title":"gc-alt-lab","version":"0.0.1"},"capabilities":{"experimentalApi":true,"requestAttestation":false}}}
< {"id":1,"result":{"userAgent":"gc-alt-lab/0.166.0 (Ubuntu 26.4.0; aarch64) xterm (gc-alt-lab; 0.0.1)","codexHome":"/…/home-fake","platformFamily":"unix","platformOs":"linux"}}
> {"method":"initialized"}
```
**[П]** `X/e5_usage.jsonl:1-3`. Версию codex брать из `userAgent`, `codexHome` — сверять с ожидаемым. Все опыты шли с `experimentalApi:true`; работа стабильного подмножества без него **[НП]**. `clientInfo.name` попадает в `originator` треда и rollout. В схеме есть `capabilities.optOutNotificationMethods` (отказ от лишних нотификаций) **[К]** SCHEMA, **[НП]**.

### 3.2 `thread/start` (конфиг на тред)
```json
> {"id":2,"method":"thread/start","params":{"cwd":"/…/ws3","sandbox":"read-only","approvalPolicy":"never","model":"gpt-5.6-luna",
   "developerInstructions":"Always end your reply with the token DEV-T3.",
   "config":{"model_reasoning_effort":"medium","mcp_servers":{"city":{"url":"http://127.0.0.1:46507/mcp","http_headers":{"X-City-Agent":"codex-1","X-City-Rig":"rig-a"}}}}}}
< {"id":2,"result":{"thread":{"id":"01a1c37a-4a15-…","sessionId":"01a1c37a-4a15-…","status":{"type":"idle"},"path":"/…/sessions/2026/11/10/rollout-2026-11-10T10-22-13-01a1c37a-….jsonl",
   "modelProvider":"usage","model":"gpt-6-luna","cliVersion":"0.166.0","historyMode":"paginated","turns":[],…},
   "model":"gpt-6-luna","cwd":"…","instructionSources":[],"approvalPolicy":"never","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"low",…}}
```
**[П]** запрос собран из двух опытов (`codex/scripts/e3_config_hooks_kill.py:43-52`, `W/exp1-run-codex.jsonl:7`); ответ показан от запроса **без оверрайдов** (`X/e5_usage.jsonl:4,7`) — при оверрайде модель и effort в ответе меняются (R15:171). Следом приходит нотификация `thread/started`.
Стабильные параметры **[К]** SCHEMA: `cwd, model, modelProvider, sandbox, approvalPolicy, approvalsReviewer, baseInstructions, developerInstructions, config, ephemeral, personality, serviceTier`.
Значения: `sandbox` = `read-only|workspace-write|danger-full-access`; `approvalPolicy` = `untrusted|on-request|never|{granular}`.

### 3.3 `turn/start`
```json
> {"id":3,"method":"turn/start","params":{"threadId":"01a1c37a-4a15-…","clientUserMessageId":"orch-approval-Afirst","input":[{"type":"text","text":"Reply with exactly: OK","text_elements":[]}]}}
< {"id":3,"result":{"turn":{"id":"01a1c37a-4a29-…","rootTurnId":"01a1c37a-4a29-…","items":[],"itemsView":"notLoaded","status":"inProgress","error":null,"startedAt":null,…}}}
< {"method":"thread/status/changed","params":{"threadId":"…","status":{"type":"active","activeFlags":[]}},…}
< {"method":"turn/started","params":{"threadId":"…","turn":{"id":"01a1c37a-4a29-…","status":"inProgress",…}},…}
< … item/started, item/completed, item/agentMessage/delta, thread/tokenUsage/updated, account/rateLimits/updated …
< {"method":"thread/status/changed","params":{"threadId":"…","status":{"type":"idle"}},…}
< {"method":"turn/completed","params":{"threadId":"…","turn":{"id":"…","items":[{"type":"agentMessage","id":"msg_…","text":"STEERED","phase":"final_answer",…}],"itemsView":"summary","status":"completed","error":null,"startedAt":1794280385,"completedAt":1794280391,"durationMs":6191}},…}
```
**[П]** `X/e5_usage.jsonl:8-12`, `X/e2_A.jsonl:32-33`, `W/exp3-orchestrator.jsonl:28`.

- **Подтверждение приёма** — `turn.id` в ответе на `turn/start` **[П]** R34:303.
- **`thread/status/changed{idle}` приходит раньше `turn/completed`** (та же миллисекунда) — границу хода брать только из `turn/completed` **[П]** `X/e2_A.jsonl:32-33`, `coldstart/logs/codex-appserver-cold1.ndjson:22-23`.
- `turn.status`: `completed | interrupted | failed | inProgress` **[К]** SCHEMA; `completed`, `interrupted`, `failed` — **[П]**.
- `clientUserMessageId` возвращается как `item.clientId` в `userMessage` и пишется в rollout (`client_id`); сервер его не проверяет — годится как маркер ключа идемпотентности **[П]** R36:147.
- На ход можно задать `model`, `effort`, `sandboxPolicy`, `approvalPolicy`, `cwd` — действуют на этот и следующие ходы **[К]** SCHEMA; `model`/`effort` **[П]** R15:146.
- Элементы: `item/started` → `item/completed` с `item.type` = `userMessage | agentMessage | commandExecution | …`; у `commandExecution` есть `command`, `status` (`inProgress|completed|failed|declined`), `aggregatedOutput`, код выхода **[П]** `codex/scripts/e10_env_lock.py:22-23`, R46b:25.
- Команды исполняются через login-shell пользователя: `/usr/bin/zsh -lc '<cmd>'` **[П]** `X/e10_run.txt`.

### 3.4 `turn/interrupt`
```json
> {"id":4,"method":"turn/interrupt","params":{"threadId":"01a1c371-…","turnId":"01a1c372-04d8-…"}}
< {"id":4,"result":{}}
< {"method":"thread/status/changed","params":{"threadId":"…","status":{"type":"idle"}},…}
< {"method":"turn/completed","params":{"threadId":"…","turn":{"id":"01a1c372-04d8-…","items":[],"status":"interrupted","error":null,"durationMs":3285,…}},…}
```
**[П]** `X/e2_A.jsonl:49-54` (35 мс от запроса до `turn/completed`, замер 2026-11-10, codex 0.166.0). <!-- MEASURE id=codex-interrupt-latency date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=lab/codex/out/e2_A.jsonl note="n=1, unix-сокет, ход в sleep 40; t 9.517→9.552" -->

### 3.5 `thread/resume`
```json
> {"id":2,"method":"thread/resume","params":{"threadId":"01a1c373-b231-…","excludeTurns":true}}
< {"method":"thread/status/changed","params":{"threadId":"…","status":{"type":"idle"}},…}
< {"id":2,"result":{"thread":{"id":"01a1c373-b231-…","status":{"type":"idle"},"path":"/…/rollout-….jsonl","preview":"Run the shell command `sleep 77` …","turns":[…]},…}}
```
**[П]** `X/e3b.jsonl:6-9` (в логе без `excludeTurns`; тогда приходит `deprecationNotice`: «use `excludeTurns: true`, then page with `thread/turns/list`» — `X/e3b.jsonl:7`).
- После `kill -9` прерванный ход в истории имеет `status:"interrupted"` с одним `userMessage`; ход сам не продолжается; модель о команде отвечает `UNKNOWN` **[П]** R15:216-218.
- На треде без единого хода: `-32600 no rollout found for thread id …` — rollout пишется с первым сообщением, тред без сообщений при рестарте теряется **[П]** R36:142; **[К]** R15:221-222.
- `thread/resume` принимает те же `config`, `cwd`, `model`, `sandbox`, `approvalPolicy`, `developerInstructions` **[К]** SCHEMA. Применяется ли конфиг треда заново после рестарта процесса, если его не передать, — **[НП]**: передавать всегда.
- Прочее: `thread/read{threadId,includeTurns}` (статус), `thread/loaded/list`, `thread/turns/list`, `thread/unsubscribe`, `thread/archive` **[К]** SCHEMA; `thread/read`, `thread/loaded/list` — **[П]** R46b:226, R15:89.

### 3.6 Запрос подтверждения (серверный запрос) и ответ
```json
< {"method":"thread/status/changed","params":{"threadId":"…","status":{"type":"active","activeFlags":["waitingOnApproval"]}},…}
< {"method":"item/commandExecution/requestApproval","id":0,"params":{"kind":"command","threadId":"…","turnId":"…","itemId":"exec-15ebe205-…","environmentId":"local",
   "command":"/usr/bin/zsh -lc 'touch approval-Afirst.txt'","cwd":"/…/work/x1","commandActions":[{"type":"unknown","command":"touch approval-Afirst.txt"}],
   "proposedExecpolicyAmendment":["touch","approval-Afirst.txt"],"availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["touch","approval-Afirst.txt"]}},"cancel"]}}
> {"id":0,"result":{"decision":"accept"}}
< {"method":"serverRequest/resolved","params":{"threadId":"…","requestId":0},…}
< {"method":"thread/status/changed","params":{"threadId":"…","status":{"type":"active","activeFlags":[]}},…}
```
**[П]** `W/exp3-orchestrator.jsonl:46-51` (тред с `approvalPolicy:"untrusted"`, `sandbox:"danger-full-access"`).
- `decision`: `accept | acceptForSession | {acceptWithExecpolicyAmendment} | {applyNetworkPolicyAmendment} | decline | cancel` **[К]** SCHEMA; `accept` и `decline` — **[П]** R36:145.
- Другие серверные запросы **[К]** SCHEMA: `item/fileChange/requestApproval`, `item/permissions/requestApproval`, `item/tool/requestUserInput` (ответ `{"answers":{…}}`), `mcpServer/elicitation/request`, `account/chatgptAuthTokens/refresh`, `item/tool/call`. Вживую снят только `commandExecution` — остальные **[НП]** R46b:300.
- Ответ, отправленный через 25 с (задержка задана опытом 2026-11-11, codex 0.166.0), принят; в это время статус `active[waitingOnApproval]`, других событий нет **[П]** R46b:166-168. <!-- MEASURE id=codex-approval-wait date=2026-11-11 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/46b,lab/observability/codex/out/e2_rpc.jsonl note="n=1; предел ожидания не искали" -->
- При `approvalPolicy:"never"` запросов нет **[П]** (все опыты отчёта 15).

### 3.7 Статус треда
`thread/status/changed.status`: `{"type":"notLoaded"}` | `{"type":"idle"}` | `{"type":"systemError"}` | `{"type":"active","activeFlags":[…]}`; флаги `waitingOnApproval`, `waitingOnUserInput` **[К]** SCHEMA; `idle`, `active[]`, `active[waitingOnApproval]`, `systemError` — **[П]**. Heartbeat'а нет нигде **[П]** R46b:18.
Маппинг на §7.1 постановки: `active[]` → running; `active[waitingOn…]` → requires_action; `idle` после `turn/completed` → idle; `systemError` → сбой хода (§5).

### 3.8 Лимиты аккаунта
```json
< {"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":100,"windowDurationMins":300,"resetsAt":1794284533},
   "secondary":{"usedPercent":42,"windowDurationMins":10080,"resetsAt":1794680933},"credits":null,"planType":null,"rateLimitReachedType":null,…}},…}
> {"id":4,"method":"account/rateLimits/read"}
< {"error":{"code":-32600,"message":"codex account authentication required to read rate limits"},"id":4}
```
**[П]** `X/e5_usage.jsonl:15,20-21` (поддельный провайдер с заголовками `x-codex-*`). `resetsAt` — unix-секунды; обновление «разрежённое» — сливать с последним снимком **[К]** SCHEMA.
Без ChatGPT-логина `account/rateLimits/read` недоступен, а в реальных ходах через прокси `primary/secondary = null` **[П]** R15:211-212, `X/e5_run.txt` (блок `ctx`). Форма при настоящей подписке **[НП]**.

## 4. Конфиг: что на тред, что на процесс

| Ключ | Как задаётся | Метка, источник |
|---|---|---|
| cwd, AGENTS.md | `thread/start.cwd`; AGENTS.md подхватывается по cwd (`instructionSources`) | [П] R15:150,169-172 |
| модель, effort | `thread/start.model`, `config.model_reasoning_effort`; на ход — `turn/start.model/effort` | [П] `codex/scripts/e3_config_hooks_kill.py:47-49` |
| инструкции | `developerInstructions` [П]; `baseInstructions` [К] SCHEMA | [П] там же (`OK DEV-T3 WS3-AGENTS`) |
| MCP (HTTP) | `config.mcp_servers.<имя> = {url, http_headers}`; у двух тредов разные `Mcp-Session-Id`, MCP-процессов codex не порождает | [П] `W/exp1-run-codex.jsonl:7`, R36:40,47 |
| MCP (stdio) | `config.mcp_servers.<имя> = {command, args, env}` принимается, нотификации `mcpServer/startupStatus/updated{threadId,name,status:starting\|ready\|failed\|cancelled}`. **В опыте E3 сервер не стартовал (`failed`)** — рабочий stdio-MCP на тред нативно [НП] | [П] `X/e3.jsonl:18-19`; через ACP-адаптер stdio-MCP работал, процесс на сессию — R15:188 |
| env команд | `config.shell_environment_policy.set = {"VAR":"v"}` — разные значения у тредов одного процесса | [П] `codex/scripts/e10_env_lock.py:32`, `X/e10_run.txt` |
| доверие хукам | `config.bypass_hook_trust = true` на тред | [П] `codex/scripts/e3_config_hooks_kill.py:43` |
| хуки | **сняты только как файл `<cwd>/.codex/hooks.json` + `bypass_hook_trust`**: сработали `sessionStart`, `userPromptSubmit`, `stop`. Передача самих хуков через `thread/start.config` — [НП] | [П] `codex/ws2/.codex/hooks.json`, `X/hooks.log`, R15:169-171 |
| sandbox, approvals | `thread/start.sandbox`, `approvalPolicy` | [П] все опыты |
| `CODEX_HOME` | **процесс**: логин, `skills/`, `plugins/`, пользовательский `config.toml`, sqlite-состояние, rollout'ы, writer-lock'и | [П] R15:175 |
| окружение процесса | **процесс**: с ним исполняются хуки (issue openai/codex#48500) | [К] R15:175-176 |
| провайдер модели | **процесс** (в 0.166): ход на треде с чужим провайдером → `{"code":-32600,"data":{"type":"modelProviderMismatch","modelProvider":"r429","currentModelProvider":"usage"}}`. На HEAD вендора README описывает иное поведение | [П] `X/e5.jsonl:24`; R15:177-179 |
| OTel-метки `[otel]` | **процесс** (читается один раз) | [К] R46b:67-69 |

Слои конфига по возрастанию силы: defaults → MDM → system → user (`$CODEX_HOME/config.toml`) → project (`.codex/` от cwd) → SessionFlags (`-c`, `thread/start.config`) **[К]** R15:164-167.
Хук-формат как у Claude Code: `{"hooks":{"SessionStart":[{"matcher":"","hooks":[{"type":"command","command":"…","timeout":10}]}]}}` **[П]** `codex/ws2/.codex/hooks.json`. 14 событий, 4 типа обработчиков **[К]** R15:181-184.
Вход хука (stdin): `{"session_id":"<thread id>","turn_id":…,"transcript_path":"/…/rollout-….jsonl","cwd":…,"hook_event_name":"Stop","model":…,"permission_mode":"bypassPermissions",…}` **[П]** `X/hooks.log:1-3`. В потоке — `hook/started`/`hook/completed` с `run.eventName`, `run.source:"project"`, `run.status` **[П]** `X/e3.jsonl:45`. `hooks/list{cwds:[…]}` показывает `trustStatus` **[П]** `codex/scripts/e3_config_hooks_kill.py:59-60`.
Дочерние процессы команд несут `CODEX_THREAD_ID` и `CODEX_TOOL_CALL_ID` в env **[П]** R46b:23.

## 5. Сбои

Общая форма **[П]** `X/e5_usage.jsonl:15-19`: [`account/rateLimits/updated`] → `thread/status/changed{systemError}` → `error` → `turn/completed{status:"failed"}`:
```json
< {"method":"error","params":{"error":{"message":"You’ve hit your usage limit. … try again at 11:22 AM.","codexErrorInfo":"usageLimitExceeded","additionalDetails":null,"misalignment":null},"willRetry":false,"threadId":"…","turnId":"…"},…}
< {"method":"turn/completed","params":{"threadId":"…","turn":{"id":"…","status":"failed","error":{"message":"…","codexErrorInfo":"usageLimitExceeded",…},"durationMs":50,…}},…}
```
`error{willRetry:true}` — промежуточная попытка, ход продолжается; финал — `willRetry:false` + `turn/completed failed`.
`codexErrorInfo` бывает строкой и объектом с одним ключом. Полный enum **[К]** SCHEMA: `contextWindowExceeded, sessionBudgetExceeded, usageLimitExceeded, rateLimitExceeded, flexUnavailable, serverOverloaded, modelNotFound, policyCapacityUnavailable, cyberPolicy, misalignmentPolicyViolation, tooManyDenials, internalServerError, unauthorized, badRequest, threadRollbackFailed, sandboxError, other`, объекты `{httpConnectionFailed|responseStreamConnectionFailed|responseStreamDisconnected|responseTooManyFailedAttempts:{httpStatusCode}}`, `{activeTurnNotSteerable}`.

| kind | Сигнал | `retry_at` | HTTP-попыток | Метка, источник |
|---|---|---|---|---|
| `quota_exhausted` | `account/rateLimits/updated{primary{usedPercent:100,resetsAt}}` **перед** `error{codexErrorInfo:"usageLimitExceeded",willRetry:false}`; в самой ошибке время только текстом | `rateLimits.primary.resetsAt` (или `secondary`) | 1 | [П] `X/e5_run.txt` (usage), `X/e5_usage.jsonl:15-19` |
| `rate_limited` (429 без `Retry-After`) | `{"responseTooManyFailedAttempts":{"httpStatusCode":429}}`, текст `exceeded retry limit, last status: 429 Too Many Requests` | нет | 1 — **не ретраится** | [П] `X/e5_run.txt` (r429) |
| `rate_limited` (429 с `Retry-After: 20`) | `error{willRetry:true,{responseStreamDisconnected:{429}}}` → 20 с тишины → `responseTooManyFailedAttempts{429}` | нет (заголовок клиенту не отдаётся) | 2 — **ретраится один раз после паузы** | [П] R46b:188,191 |
| `overloaded` (503) | один финальный `"serverOverloaded"` через 3,3 с; промежуточные попытки в app-server **не видны** | нет | 5 (R15) / 3 при `request_max_retries=2` (R46b) | [П] `X/e5_run.txt` (r503), R46b:186,193 |
| `auth_required` (401) | 5 × `error{willRetry:true,{responseStreamDisconnected:{401}}}` («Reconnecting… n/5») → `{httpConnectionFailed:{401}}` через 6,6 с; вариант `"unauthorized"` [К] | нет | 6 | [П] `X/e5_run.txt` (r401) |
| `billing` | `usage_not_included`, `insufficient_quota` разбираются в коде codex; форма в app-server [НП] | — | — | [К] R34:90 |
| `context_overflow` | `"contextWindowExceeded"` | — | 1 | [П] `X/e5_run.txt` (ctx) |
| `config_error` | `"modelNotFound"`, `"badRequest"` [К]; отказ `modelProviderMismatch` на `turn/start` [П] | — | — | R34:92, `X/e5.jsonl:24` |
| `harness_crash` | EOF канала за 0,00 с; ожидающие запросы не получают ответа | — | — | [П] R15:216 |
| `stall`: стрим замолчал | через `stream_idle_timeout_ms` (умолч. 300 с; в опыте 15 с) `error{willRetry:true,responseStreamDisconnected,"idle timeout waiting for SSE"}`, затем `systemError`/`failed` | — | 2 | [П] R46b:185; умолчание [К] R46b:30 |
| `stall`: провайдер молчит до заголовков | **ничего**: 45 с статус `active[]`, ни одной нотификации; после обрыва — молчаливый повтор. Собственного таймаута у codex нет | — | 2 | [П] R46b:28-29,184 |
| `tool_failure` | `item/completed` с `status:"failed"`, код выхода; MCP — `mcpServer/startupStatus/updated{failed}` | — | — | [П] R46b:25, `X/e3.jsonl:19` |
| `transcript_divergence` | второй **процесс** на тот же тред: `-32600 thread <id> already has an active writer` | — | — | [П] `X/e10_run.txt`, `X/e10b.jsonl` |

Замеры таблицы: 2026-11-10 (R15) и 2026-11-11 (R46b), codex 0.166.0. <!-- MEASURE id=codex-failure-attempts date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=lab/codex/out/e5_run.txt,reports/46b note="поддельный провайдер; число HTTP-попыток по логу провайдера; 401 — 6 попыток за 6,6 с, 503 — 5 за 3,3 с; в 46b у провайдера request_max_retries=2, stream_max_retries=1, stream_idle_timeout_ms=15000" -->
Умолчания повторов **[К]**: `retry_429:false, retry_5xx:true, retry_transport:true`, лимиты попыток 4/5 (R15:203-204); `stream_idle_timeout_ms` 300 с, `stream_max_retries` 5 (R46b:30,272). Худший случай тишины посреди стрима ≈ 300 с × (1+5) ≈ 30 мин — оценка, не замер (R46b:248).
Вывод для драйвера: `retried_by_harness=false` для 429 без `Retry-After` (повтор обеспечивает политика), `true` для 5xx/401/обрыва стрима.

В rollout тот же сбой — snake_case: `event_msg{type:"task_complete",error:{codex_error_info:"usage_limit_exceeded"}}` и `event_msg{type:"token_count",rate_limits:{primary:{used_percent:100.0,window_minutes:300,resets_at:…}}}` **[П]** `codex/home-fake/sessions/2026/11/10/rollout-…4a15….jsonl:14-15`.
Можно ли продолжать тред после `systemError` следующим `turn/start` — **[НП]**.

## 6. Фон и «idle»

- Фоновый процесс команды: `turn/completed` и `thread/status/changed{idle}` приходят, **пока процесс ещё жив**; `item/completed` этой команды приходит позже, вне хода (в опыте через 36 с после конца хода; замер 2026-11-11, codex 0.166.0) **[П]** R46b:40,162-165. <!-- MEASURE id=codex-bg-item-after-turn date=2026-11-11 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/46b,lab/observability/codex/out/e2_rpc.jsonl note="n=1, фоновый sleep 40; turn/completed на 7,1 с, item/completed на 43,3 с" --> «Idle без фона» = `idle` и нет `commandExecution` с `item/started` без `item/completed`.
- `thread/backgroundTerminals/list` (experimental) → `[{itemId,processId,command,osPid:null,cpuPercent:null,rssKb:null}]`; `osPid/cpu/rss` в 0.166 всегда `null` **[П]** R46b:164,171.
- Долгая команда: yield инструмента ≤ 30 с (константа кода), дальше модель сама поллит; между событиями тишина 26 с при `sleep 45` (замер 2026-11-11, codex 0.166.0) **[П]** R46b:134,142 — порог сторожа для идущего `commandExecution` должен быть заметно больше 30 с.
<!-- MEASURE id=codex-long-command-silence date=2026-11-11 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/46b,lab/observability/codex/out/e1_timeline.txt note="codex exec --json, sleep 45, n=1; в app-server отдельно не мерили" -->
- Субагенты — дочерние треды (`parentThreadId`, `agentRole`) **[К]** R15:153; не испытаны **[НП]**.
- Выгрузка простаивающего треда (`thread_unload_delay_secs`, 60 с) и судьба его фоновых терминалов — **[НП]** R46b:302.

## 7. Процесс и несколько клиентов

| Событие | Поведение | Метка, источник |
|---|---|---|
| `kill -9` app-server посреди команды | клиент видит EOF за 0,00 с (замер 2026-11-10, codex 0.166.0); `sleep` и code-mode-хост умирают вместе с сервером (сирот нет) — `PR_SET_PDEATHSIG`, **только Linux** | [П] R15:216; [К] R34:254 |
| после рестарта | `thread/resume` → `idle`, прерванный ход `interrupted`; **все** треды процесса оборваны разом | [П] R15:217, `X/e3b.jsonl` |
| EOF на stdin (stdio, один клиент) | сервер завершается: до 30 с на слив RPC, жёсткий выход через 45 с (константы кода, не замер); судьба идущего хода [НП] | [К] R34:268 |
| unix-сокет, клиент отключился | сервер и ходы живут; новый клиент делает `thread/loaded/list` + `thread/resume` на идущем ходе (`status: active`) и получает поток | [П] R15:88-90 (E2) |
| два клиента на треде | оба получают одинаковые `item/*` и `turn/completed`; `turn/interrupt` и `turn/steer{expectedTurnId}` работают с любого соединения | [П] R15:89-91 |
| второй `turn/start` посреди хода | **не очередь и не отказ: сливается в идущий ход (steer)** — ответ несёт тот же `turn.id`, в ходе оба `userMessage`, модель отвечает только на последнее | [П] R36:143 |
| два одновременных `turn/start` на простаивающем треде | оба получают один `turn.id`; второй добавлен как steer; ответ один | [П] R36:144 |
| запрос подтверждения при двух подписчиках | приходит обоим с одним `id`; выигрывает первый ответ, поздний молча игнорируется; `serverRequest/resolved` — всем | [П] R36:145 |
| второй процесс app-server на тот же тред | отказ `already has an active writer`; lock-файлы `$CODEX_HOME/thread-writer-locks/<threadId>.lock` | [П] `X/e10_run.txt` |
| зависший app-server | держит writer-lock треда (openai/codex#41849) | [К] R15:246 |
| человек | `codex --remote unix://SOCK resume <threadId>` открывает тред оркестратора, поток виден обеим сторонам | [П] R15:93-96 (E8) |

<!-- MEASURE id=codex-kill9-eof date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/15,lab/codex/scripts/e3_config_hooks_kill.py note="stdio, n=1, ход в sleep 77; время от kill до EOF на stdout клиента" -->
Следствие: ввод в тред сериализует демон (проверять, что предыдущий `turn/completed` получен). Настоящая очередь — `thread/queue/*` (experimental; CLI `codex queue … --remote` **[П]** R36:146).

**Rollout.** `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<ts>-<threadId>.jsonl`; путь приходит в `thread.path` и в `transcript_path` хука **[П]** R15:226, `X/e5_usage.jsonl:7`. Строки: `{"timestamp":…,"ordinal":N,"type":"session_meta"|"event_msg"|…,"payload":{…}}` **[П]** rollout выше. Во время команды файл не меняется (30 с в опыте) **[П]** R46b:233. Формат мигрирует на sqlite (`historyMode:"paginated"`, `thread_history_1.sqlite`) — читать историю лучше через `thread/turns/list`, а не файл **[К]** R15:229-230. thread id = id для `codex resume` = имя rollout **[П]** R15:227.

## 8. Логин и песочница

- До старта: `codex login status` (на стенде `Not logged in`) **[П]** R15:9. В протоколе: `account/read{refreshToken:false}` → `{"account":null,"requiresOpenaiAuth":false}` при кастомном провайдере **[П]** `X/e1.jsonl:6-7`; `account` бывает `{type:"apiKey"}`, `{type:"chatgpt",email,planType}` **[К]** SCHEMA.
- Логин лежит в `CODEX_HOME`; другой аккаунт = другой `CODEX_HOME` = другой процесс **[П]** R34:241. Истечение токена: в режиме `chatgpt` codex обновляет сам; в `chatgptAuthTokens` шлёт серверный запрос `account/chatgptAuthTokens/refresh` **[К]**, **[НП]** R15:213-214.
- Кастомный провайдер задаётся в `$CODEX_HOME/config.toml` (`[model_providers.<n>] base_url, wire_api="responses", env_key`) и выбирается `-c 'model_provider="<n>"'` на процесс **[П]** `codex/home-fake/config.toml`, `codex/scripts/e5_failures.py:9`.
- **Песочница на стенде не работала**: `Codex's Linux sandbox uses bubblewrap and needs access to create user namespaces` (`configWarning` при каждом старте); команда под `read-only` упала сразу (`bwrap: No permissions to create a new namespace`); все долгие команды шли в `danger-full-access` **[П]** `X/e5_usage.jsonl:5`, R36:143, R15:315. Поведение рабочей песочницы, её совместимость с финишером и записью вне cwd — **[НП]** целиком.

## 9. Замеры

| Что | Значение | Дата, версия | Источник |
|---|---|---|---|
| RSS app-server: пустой / 1 / 2 / 4 треда (простой → после хода) | 93 / 132→155 / 158→168 / 172→182 МБ | 2026-11-10, codex 0.166.0 | R15:120 |
| то же, другой прогон | 94 МБ пустой → 154…171 МБ после 5 тредов; +4–6 МБ на ход | 2026-11-10, codex 0.166.0 | R42:30,80 |
| `thread/start` в тёплом сервере | 10 мс | 2026-11-10, codex 0.166.0 | R42:30 |
| `thread/start` → первая дельта | 2,57 с [2,35–6,70], n=5 (через прокси) | 2026-11-10, codex 0.166.0 | R42:30 |
| 4 хода параллельно в 4 тредах | 5,7 с, все `completed` | 2026-11-10, codex 0.166.0 | R15:86 |
| релизы codex | стабильный раз в 2–8 дней (0.164 23.10 → 0.167 06.11) | наблюдение 2026-11-10 | R15:242 |
<!-- MEASURE id=codex-appserver-rss date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/15,reports/42 note="RSS из /proc/pid/status, ходы reply OK, до 5 тредов; история треда живёт в памяти процесса — на длинных историях и 10–15 ходах сразу не мерили" -->
<!-- MEASURE id=codex-thread-start date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/42 note="тёплый app-server на stdio; gpt-6-luna low через прокси, хвосты до 6,7 с — задержки прокси" -->
<!-- MEASURE id=codex-parallel-turns date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/15,lab/codex/out/e1.jsonl note="n=1, 4 треда с разными cwd" -->
<!-- MEASURE id=codex-release-cadence date=2026-11-10 ver=codex-0.166.0 env=не-применимо src=reports/15 note="gh api releases; 30 минорных за 151 день; пересчитать перед выбором политики обновлений" -->

## 10. Experimental и прямо названное непроверенным

- **Experimental по вендору:** вся подкоманда `app-server`; WebSocket-транспорт (а значит и unix-сокет); `thread/queue/*`, `server/diagnostics`, `thread/backgroundTerminals/*`; auth-режим `chatgptAuthTokens` (R15:40,82,234; R46b:226-227).
- **[НП] в отчётах:** лимиты и логин ChatGPT-подписки, истечение OAuth (R15:305); память на длинных тредах и 10–15 одновременных ходах (R15:307); суточная работа и утечки MCP-процессов (#25015, #38754; R15:310); `waitingOnUserInput` и хук `PermissionRequest` (R46b:300); перенос треда между `CODEX_HOME` (R34:385); EOF посреди хода (R34:381); поведение вне Linux (нет `PR_SET_PDEATHSIG`); песочница (§8).
- Открытые дефекты вендора, важные для автономной работы **[К]** R15:245-247: #48500 (хуки с env первого клиента), #41849 (зависший сервер держит writer-lock), #18960 (reconnect-loop websocket), #45019 («queued follow-up no longer exists»).

## 11. Расхождения в отчётах

| Тема | Значение А | Значение Б |
|---|---|---|
| ретрай 429 | R15:198,203 и R34:87: «не ретраится», 1 попытка | R46b:188-191: с `Retry-After` повторяется после паузы (2 попытки) |
| попыток на 503 | R15:199: 5 | R46b:186: 3 (там `request_max_retries=2`) |
| MCP на тред | R15:171-172: T3 «со своим `mcp_servers`» — подтверждено | `X/e3.jsonl:18-19`: этот MCP-сервер не стартовал (`failed`); рабочий MCP на тред подтверждён только по HTTP (R36) |
| провайдер на тред | 0.166: `modelProviderMismatch` [П] | README на HEAD: «Resume and input use the thread's saved provider» (R15:178) |
| память пустого сервера | R15:120: 93 МБ | R42:30: 94 МБ; на 5 тредах 171 МБ против 182 МБ на 4 (разные прогоны) |

## 12. Чего здесь нет

Выяснять опытом (отчёты читать **не нужно**):
- Rust-клиент WebSocket поверх unix-сокета против реального сервера (ping/pong, фрагментация, большие сообщения); запасной путь — stdio;
- доставка хуков через `thread/start.config` без файла в cwd; рабочий stdio-MCP на тред; `thread/resume` с повторной передачей конфига после рестарта процесса;
- переподключение клиента к живому серверу на unix-сокете после рестарта демона: какие нотификации потеряны, как дочитать состояние (`thread/read`, `thread/turns/list`);
- работа без `experimentalApi`; `optOutNotificationMethods`;
- успех после 429 на поддельном провайдере (в лабораторных подделках нет режима «ok» — см. `05-lab-artifacts.md`), несуществующая модель, `insufficient_quota`;
- песочница: `workspace-write`, сеть, сокет демона и запись в БД трекера вне cwd; macOS (seatbelt, судьба дочерних команд);
- настоящий логин ChatGPT: `account/read`, `account/rateLimits/read`, форма `rateLimitReachedType`, идентификатор аккаунта для ворот выдачи.

Полные типы полей — только из сгенерированной схемы закреплённой версии; `lab/codex/schema-ts/` (TypeScript, 0.166.0, 3,1 МБ) годится для чтения глазами.
