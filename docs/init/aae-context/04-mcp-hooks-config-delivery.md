# 04. Доставка конфига в сессию: MCP, хуки, skills, наследование хоста (claude `-p`, codex app-server)

Стенд: Linux aarch64 (OrbStack), 2026-11-10 и 2026-11-11; claude 2.1.322 (ACP-адаптер запускал свой 2.1.321), codex 0.166.0.
Метки: **[П]** проверено опытом, **[К]** по коду/документации, **[В]** вывод авторов отчёта, **[НП]** не проверено.
**[П-ACP]** — проверено только через ACP-адаптер `claude-agent-acp` 0.102.0; для нативного `claude -p` не подтверждено.

Источники: `R14`=`reports/14-harness-claude-acp.md`, `R15`=`reports/15-harness-codex-acp.md`, `R18`=`reports/18-mcp-hooks-config-standards.md`, `R22`=`reports/22-factcheck.md`, `R34`=`reports/34-failure-recovery-spec.md`, `R36`=`reports/36-experiments-shared-mcp-vendor-daemons.md`, `R42`=`reports/42-coldstart-floor.md`, `R46a`=`reports/46a-observability-claude.md`, `FR`=`FINAL-REPORT.md`; число после `:` — строка.
Пути `lab/…` — от `tmp/gc-alt-research/`. Имена заголовков `X-City-*` — лабораторные, не контракт.

## 1. Каналы доставки: сводка

| Что | claude (`claude -p`, процесс на сессию) | codex (`codex app-server`, тред) |
|---|---|---|
| HTTP-MCP | `--mcp-config <file>`: `{"mcpServers":{"n":{"type":"http","url":…,"headers":{…}}}}` **[П]** `lab/coldstart/role/mcp.json:2`, `lab/coldstart/scripts/native.mjs:59` | `thread/start.config.mcp_servers.<n> = {url, http_headers:{…}}` **[П]** `lab/wave2-exp/scripts/exp1.mjs:59-64` |
| stdio-MCP | там же: `{"command":…,"args":[…]}` (или с `"type":"stdio"`) **[П]** `role/mcp.json:3`, `lab/observability/claude/scripts/gen_cfg.py:34` | `config.mcp_servers.<n> = {command, args, env}` **[П]** `lab/codex/scripts/e3_config_hooks_kill.py:49-51` |
| Отсечь чужие MCP | `--strict-mcp-config` **[П]** R42:9,23 | свой `CODEX_HOME` (§5) |
| Хуки | `--settings <file>` с ключом `hooks` **[П]** `role/settings.json`, `native.mjs:60` | **сквозной путь без записи в cwd [НП]**, см. §3.4 |
| Skills | `--plugin-dir <dir>` **[П]** `native.mjs:62` | `$CODEX_HOME/skills/` или `.agents/skills` в cwd **[К]**, флага нет (§4) |
| Системный промпт | `--append-system-prompt <text>` (9,5 КБ одним аргументом) **[П]** `native.mjs:61` | `thread/start.developerInstructions` **[П]** `e3…py:48`, R15:171-172; `baseInstructions` есть в схеме, не испытан **[К]** `lab/codex/schema-ts/v2/ThreadStartParams.ts` |
| Модель, effort | `--model`, `--effort` **[П]** `lab/observability/claude/scripts/run.sh:41` | `thread/start.model`, `config.model_reasoning_effort` **[П]** `e3…py:47-49` |
| Env команд | env процесса при spawn | `config.shell_environment_policy.set` **[П]** `lab/codex/scripts/e10_env_lock.py:32`, `lab/codex/out/e10_run.txt:1-2` |
| Наследование хоста | `--setting-sources ""` + `--strict-mcp-config` (§5) | отдельный `CODEX_HOME` (§5) |
| Разрешения | `--permission-mode`, `--permission-prompt-tool` **[П]** `run.sh:43` | `thread/start.sandbox`, `approvalPolicy` **[П]** `exp1.mjs:61-62` |

Всё в колонке claude относится к одной сессии, потому что процесс один на сессию. У codex `config{}` запроса — слой SessionFlags, конфиг собирается заново на каждый тред с его `cwd` **[К]** R15:164-167 (`app-server/src/config_manager.rs:656-700`). Клиент codex в опытах объявлял `capabilities.experimentalApi: true` **[П]** `lab/wave2-exp/scripts/lib.mjs:179`.

## 2. MCP

### 2.1 Заголовки идентичности

| Харнесс | Как | Метка |
|---|---|---|
| claude нативно | буквальные значения в `headers` файла `--mcp-config`; дошли до сервера (`agent=coldstart rig=lab`, `ua=claude-code/2.1.322 (sdk-cli)`) | **[П]** `lab/coldstart/logs/mcp-server.jsonl:2-6` |
| claude, подстановка | `headers` с `${VAR}` из env сессии; `headersHelper` | **[К]** R18:77 (документация), не испытано |
| codex app-server | `http_headers` треда; у двух тредов одного процесса разные заголовки и разные `Mcp-Session-Id` | **[П]** R36:37-40, `lab/wave2-exp/logs/exp1-run.out:20-21,34` |
| codex, прочее | `env_http_headers`, `bearer_token_env_var`, `http_headers_helper` | **[К]** R18:77 (`codex-rs/config/src/mcp_types.rs:658-690`), не испытано |

Один процесс сервера обслужил 2 сессии claude + 2 треда codex (+2 pi), 6 вызовов, MCP-детей у харнессов 0 (опыт 2026-11-10; claude 2.1.321 через ACP, codex 0.166.0) **[П]** R36:9-10, `exp1-run.out:34`.
<!-- MEASURE id=shared-http-mcp date=2026-11-10 ver=claude-2.1.321,codex-0.166.0 env=linux-arm64-orbstack src=reports/36:9,lab/wave2-exp/logs/exp1-run.out:34 note="6 вызовов whoami, по 2 на харнесс; claude шёл через ACP-адаптер; под нагрузкой не проверялось" -->

### 2.2 Версии протокола и что обязан уметь сервер

| Клиент | Первое действие | Откат | Источник |
|---|---|---|---|
| claude 2.1.322 нативно и 2.1.321 через ACP | `POST server/discover`, заголовок `MCP-Protocol-Version: 2026-07-28` | при 400 или `-32601` шлёт `initialize` с 2025-11-25, затем `notifications/initialized`, `GET` (SSE), `tools/list` | **[П]** `lab/coldstart/logs/mcp-server.jsonl:2-7` (нативно), `lab/wave2-exp/logs/mcp-server-summary.txt:11-15` (ACP), R36:51 |
| codex 0.166.0 (`codex-mcp-client/0.166.0`) | `initialize` с 2025-06-18 | нет; затем `GET` с `Accept: text/event-stream, application/json` | **[П]** `mcp-server-summary.txt:23-26`, R36:52 |

Матрица (опыт 2026-11-10, сервер самописный `lab/wave2-exp/scripts/city-mcp.mjs`) **[П]** R36:57-62, `lab/wave2-exp/logs/exp1b-C.out:2-24`:

| Режим сервера | claude | codex |
|---|---|---|
| stateful (`Mcp-Session-Id`), `GET`→405 | работает | работает |
| stateless без session id, `GET`→405 | работает (через откат на `initialize`) | работает, `mcp_session=none` |
| dual: 2026-07-28 stateless + прежний | работает, идёт по 2026-07-28 без `initialize` | работает, идёт по прежнему |
| только 2026-07-28 | работает | **отказ** `-32022 … requested 2025-06-18` |

Следствия для сервера `aae` и общих сервисов:
- Обе эпохи обязательны. Эпоху выбирать по запросу: заголовок `MCP-Protocol-Version: 2026-07-28` ⇒ stateless-ветка с `server/discover`; `initialize` ⇒ прежняя ветка **[П]** как сделано в `city-mcp.mjs:151-152`.
- `Mcp-Session-Id` и постоянный `GET`-поток не обязательны: на `GET` можно отвечать 405 **[П]** R36:12.
- В прежней ветке лучше не выдавать session id (stateless): рестарт не порождает 404 **[В]** R36:88. Прозрачность рестарта такого сервера для codex — **[НП]** R36:94.
- В 2026-07-28 удалены протокольные сессии, `initialize`, `ping`, возобновляемость SSE и GET-эндпоинт; Roots deprecated **[К]** R18:64-67, R22:51.
- Сервер в опыте — лабораторное приближение спецификации; поведение SDK-серверов на `server/discover` от claude — **[НП]** R36:94.

### 2.3 Сервер упал или перезапущен

| Ситуация | claude | codex |
|---|---|---|
| вызов, пока сервер лежит | ошибка инструмента `MCP server "city" is not connected`, ход завершается нормально **[П-ACP]** `exp1-run.out:37` | `Transport send error …`, ход `completed` **[П]** `exp1-run.out:38` |
| stateful-сервер поднят на том же порту | фоновое переподключение: 6,8 / 15,7 / 20,6 с после подъёма; в 4-м прогоне не успел за 25,7 с и ход ответил `MCP server "city" failed to start` **[П-ACP]** R36:73-78, `exp1-run.out:42`, `lab/wave2-exp/logs/exp1b-D.out:4,10-14` | первый вызов получает `404 unknown session`, сразу `initialize` и повтор, ≈5 мс **[П]** R36:70, `exp1-run.out:43` |
| сервер dual-era (claude идёт stateless) | следующий `tools/call` прошёл на новом pid без рукопожатия **[П-ACP]** R36:81, `exp1b-D.out:22-23` | **[НП]** |

Замер 2026-11-10, claude 2.1.321 (через ACP), codex 0.166.0. По логу D1 выходит ≈16 с (`+36.2s after kill` при простое 20 с), в отчёте 15,7 с.
<!-- MEASURE id=claude-mcp-reconnect date=2026-11-10 ver=claude-2.1.321 env=linux-arm64-orbstack src=reports/36:73-78,lab/wave2-exp/logs/exp1b-D.out note="4 прогона через ACP-адаптер, stateful HTTP-MCP, рестарт на том же порту; перемерить на нативном claude -p с долгоживущим stream-json" -->
<!-- MEASURE id=codex-mcp-reinit date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/36:70 note="один прогон: 404 → initialize → повтор за 5–6 мс; stateful-сервер" -->

- Принудительно: промпт `/mcp reconnect <имя>` → `- Reconnected city` **[П-ACP]** R36:80, `exp1b-D.out:17`. В нативном `-p` — **[НП]**.
- Настраивается ли задержка переподключения claude — **[НП]** R36:179.
- Сбой MCP не роняет ход ни у одного харнесса: это ошибка инструмента **[П]** R36:69.

### 2.4 Прочее про MCP

- claude в режиме `default` требует разрешения на MCP-инструмент (`mcp__<server>__<tool>`) и сначала ищет его через `ToolSearch` (инструменты отложенные) **[П-ACP]** R36:65, `exp1-run.out:8-10`. В `-p` без ответчика на разрешения нужен режим разрешений или `--permission-prompt-tool` **[В]**.
- codex `mcpServerStatus/list` открывает временную MCP-сессию и делает `GET` с `MCP-Protocol-Version: 2024-11-05`; если сервер отвечает на него потоком SSE, вызов занимает ≈5 с (замер 2026-11-10, codex 0.166.0) **[П]** R36:66, `mcp-server-summary.txt:31-40`.
  <!-- MEASURE id=codex-mcpstatus-sse-delay date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/36:66 note="GET без session id висит 5 с до таймаута клиента; при GET→405 задержки нет — перемерить при смене версии" -->
- stdio-MCP стартует на каждую сессию claude (родитель — процесс `claude`) **[П-ACP]** R14:98 и на каждую сессию codex **[П-ACP]** R15:188 (2 ACP-сессии ⇒ 2 процесса; `session/close` за 2 с их не убрал). Пул `McpConnectionPool` есть только в HEAD codex и только для серверов 2026-07-28; в бинарнике 0.166.0 его lease-части нет **[К]** R22:147-150 — считать, что codex stdio не делит.
- Память (замер 2026-11-10): общий HTTP-сервер на Node 74,5 МБ RSS / 38,6 МБ PSS; один stdio-экземпляр на Node 58–62 МБ RSS / 23–26 МБ PSS; на Python 17,5–18,7 МБ RSS / 8,6–9,7 МБ PSS **[П]** R36:83, `lab/wave2-exp/logs/stdio-rss.txt:1-4,13-19`.
  <!-- MEASURE id=mcp-shared-vs-stdio-rss date=2026-11-10 ver=claude-2.1.321,codex-0.166.0 env=linux-arm64-orbstack src=reports/36:83,lab/wave2-exp/logs/stdio-rss.txt note="минимальные серверы с одним инструментом; версии Node и Python не записаны; реальные серверы тяжелее" -->
- Порт на loopback без аутентификации доступен любому локальному процессу **[В]** R18:213.

## 3. Хуки

### 3.1 Диалект Claude (контракт скрипта)

| Часть | Содержание | Источник |
|---|---|---|
| stdin | один JSON: `session_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name`, `agent_id` + поля события (`tool_name`, `tool_input`, `tool_use_id`, `prompt`, `source`, `notification_type`, `message`, `error`…) | **[К]** R18:134; **[П]** поля читались в `lab/observability/claude/scripts/hook.py:15-30` |
| код возврата | 0 — нет возражений; 2 — блокировка; прочее — не блокирует (кроме `onFailure: "block"`) | **[К]** R18:134 |
| stdout | JSON: `hookSpecificOutput.{permissionDecision, updatedInput, additionalContext}`, `decision: "block"`, `continue`, `stopReason`, `systemMessage` | **[К]** R18:142 |
| обработчики | `command`, `http`, `mcp_tool`, `prompt`, `agent` | **[К]** R18:133 |
| форма в settings | `{"hooks":{"<Event>":[{"matcher":"…","hooks":[{"type":"command","command":"…","timeout":10}]}]}}`; `matcher` необязателен | **[П]** `lab/coldstart/role/settings.json:2-6`, `gen_cfg.py:18-29` |

События Claude Code — 33 (по документации на 2026-11-10, claude 2.1.322; список снят с первых 100 из 290 тыс. символов страницы, матрица «обработчик × событие» не извлечена) **[К]** R18:131-132, R18:298: `SessionStart, Setup, UserPromptSubmit, UserPromptExpansion, PreToolUse, PermissionRequest, PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch, Notification, MessageDisplay, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop, StopFailure, TeammateIdle, InstructionsLoaded, ConfigChange, CwdChanged, DirectoryAdded, FileChanged, WorktreeCreate, WorktreeRemove, PreCompact, PostCompact, PreModelSwitch, PostModelSwitch, Elicitation, ElicitationResult, SessionEnd`.
<!-- MEASURE id=claude-hook-event-count date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/18:131 note="подсчёт по неполной выжимке страницы code.claude.com/docs/en/hooks; пересчитать по полной странице" -->

Проверено в нативном `-p` (claude 2.1.322):
- `SessionStart`, `UserPromptSubmit`, `Stop` срабатывают: +0,19 / +0,27 / +1,93 с от spawn (замер 2026-11-10, n=1) **[П]** R42:41, `lab/coldstart/run/hookprobe.log`, `lab/coldstart/run/hookprobe.t0`. Отчёт выводит отсюда «`Stop` срабатывает до `result`», но `result` у него на 1,91 с — раньше метки хука, и часы разные (метка хука — `date`, `result` — таймер скрипта): порядок `Stop` и `result` считать **[НП]**.
  <!-- MEASURE id=claude-hook-timing-p date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42:41,lab/coldstart/run/hookprobe.log note="haiku, n=1, хуки пишут метку времени date, result мерится таймером скрипта (1,91 с); порядок Stop и result этим не доказан — снять с --include-hook-events" -->
- В реальных прогонах 2026-11-11 сработали также `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `PermissionRequest`, `SubagentStart`, `SubagentStop` **[П]** `lab/observability/claude/runs/r1/stream.jsonl` (события `hook_started`).
- В поток stream-json хуки попадают как `system/hook_started` и `system/hook_response{exit_code, outcome, stdout, stderr}`. Без флага `--include-hook-events` в потоке виден только `SessionStart`; с флагом — все **[П]** `lab/coldstart/logs/native-A3H-probe1.ndjson:1-2` против `runs/r1/stream.jsonl` (`run.sh:44`).
- `StopFailure` (matcher `rate_limit|overloaded|authentication_failed|billing_error|server_error|max_output_tokens|…`) решений не принимает **[К]** R18:135.
- Хуки не получают `OTEL_*` из env; свой адрес передавать своей переменной **[К]** R46a:95,306.

### 3.2 Чем отличается codex (0.166.0)

- Событий 14: `PreToolUse, PermissionRequest, PostToolUse, PreCompact, PostCompact, SessionStart, SessionEnd, UserPromptSubmit, SubagentStart, SubagentStop, Stop, Interrupt, PreModelRequest, PostModelRequest` **[К]** R18:138-139, R22:52, `lab/codex/schema-ts/v2/HookEventName.ts`. (В R08:215 записано «11» — устарело.)
  <!-- MEASURE id=codex-hook-event-count date=2026-11-10 ver=codex-0.166.0 env=linux-arm64-orbstack src=reports/22:52,lab/codex/schema-ts/v2/HookEventName.ts note="схема сгенерирована из 0.166.0; пересчитать при обновлении codex" -->
- Нет аналога `StopFailure`: ошибку API и квоту хуком не поймать **[К]** R18:159.
- Формат конфига тот же (`hooks.json` либо `[hooks]` в `config.toml`) **[П]** `lab/codex/ws2/.codex/hooks.json`.
- stdin **[П]** `lab/codex/out/hooks.log:1-3`: `session_id` (= id треда), `turn_id`, `transcript_path` (rollout в `CODEX_HOME`), `cwd`, `hook_event_name`, `model`, `permission_mode` (`bypassPermissions` при `approvalPolicy: never`) + `source` | `prompt` | `stop_hook_active`, `last_assistant_message`.
- Код 2 блокирует **только при непустом stderr**, иначе хук считается упавшим **[К]** R22:52 (`hooks/src/events/pre_tool_use.rs:277-283`).
- `permissionDecision: "ask"` есть в схеме, но код его отвергает и **пропускает вызов** (fail-open) **[К]** R22:52 (`:577-602`).
- `updatedInput` только вместе с `allow`; у `PermissionRequest` его нет **[К]** R18:143,155.
- Обработчики `command` и `mcp_tool` работают; `prompt` и `agent` разбираются и пропускаются **[К]** R18:140.
- Имена инструментов в матчерах: `Bash`, `apply_patch` (алиасы `Edit`/`Write`), `mcp__s__t`; hosted-инструменты не матчатся **[К]** R18:143.
- codex выставляет хукам `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` для совместимости **[К]** R18:165.
- app-server шлёт нотификации `hook/started` и `hook/completed{run.eventName, status, durationMs}`; есть метод `hooks/list{cwds}` **[П]** `lab/codex/out/e3.jsonl:21,45-46`.

### 3.3 Доверие и окружение у codex

- Каждый хук доверяется по хэшу; `HookTrustStatus = managed | untrusted | trusted | modified` **[К]** `lab/codex/schema-ts/v2/HookTrustStatus.ts`. Новый или изменённый хук в TUI останавливает сессию на «Hooks need review» **[К]** R18:38, R08:84.
- Обход: `thread/start.config.bypass_hook_trust = true` (на тред) либо флаг `--dangerously-bypass-hook-trust` **[К]** R18:144 (`app-server/src/config_manager.rs:671`). Опыт E3: 6 проектных хуков со статусом `untrusted` исполнились при `bypass_hook_trust` **[П]** `e3…py:41-43`, `lab/codex/out/e3.jsonl:21,45-47`, R15:169-171. Сработали только `SessionStart`, `UserPromptSubmit`, `Stop` (ход без инструментов); `PreToolUse`/`PostToolUse`/`SessionEnd` в опыте не вызывались.
- Что делает app-server с недоверенным хуком без обхода (TUI нет) — **[НП]**.
- Хуки исполняются с окружением **процесса app-server**, а не треда **[К]** R15:175-176 (issue openai/codex#48500). Меняет ли `shell_environment_policy.set` окружение хуков — **[НП]**; для команд оно работает (§1).
- Доверие пишется в `$CODEX_HOME/config.toml` (`[hooks.state]`) **[П]** FR:875.

### 3.4 Доставка хуков на сессию без записи в репозиторий

| Харнесс | Путь | Статус |
|---|---|---|
| claude | файл вне рига + `--settings <путь>`; пользовательские и проектные хуки отсекает `--setting-sources ""` | **[П]** `native.mjs:56-63`, R42:37,41; через ACP то же: R14:89-90 |
| claude | хуки внутри плагина (`hooks/hooks.json`) через `--plugin-dir` | **[К]** R18:136, не испытано |
| claude | `--settings` с inline-JSON вместо пути | **[НП]** в опытах — только путь к файлу |
| codex | `.codex/hooks.json` в `cwd` треда + `bypass_hook_trust` | **[П]**, но это запись в рабочий каталог |
| codex | `thread/start.config.hooks = {…}` + `bypass_hook_trust` | **[НП]** R18:297, FR:859. R15:31-32 перечисляет «хуки» среди оверрайдов треда, но опыт E3 брал их из файла проекта |
| codex | `-c 'hooks.<Event>=[{matcher="…",hooks=[{type="command",command="…",timeout=10}]}]'` при запуске процесса | **[К]** рабочий конфиг владельца под TUI (codex 0.163–0.165): `gascity-packs/packs/metropolis/pack.toml:28-37`; под app-server не испытано. Второй `-c hooks.<Event>` заменяет первый, а не дополняет (комментарий в `packs/servitors/pack.toml:39`) |
| codex | `hooks.json` или `[hooks]` в сгенерированном `CODEX_HOME` (источник `user`) | **[К]** R18:141; не испытано |

Если путь через `thread/start.config.hooks` не подтвердится, набор хуков становится «процессной» частью (флаги `-c` либо `CODEX_HOME`).

## 4. Skills и плагины

| | claude | codex |
|---|---|---|
| Без записи в cwd | `--plugin-dir <dir>`: `<dir>/.claude-plugin/plugin.json` (`name`, `version`, `description`) + `<dir>/skills/<имя>/SKILL.md` (frontmatter `name`, `description`) **[П]** `lab/coldstart/role/plugin/`, R42:37 | флага нет **[В]** R18:239. Варианты: `$CODEX_HOME/skills/` (на процесс) **[К]** R15:175; симлинк в cwd **[В]** |
| Имена | skill из плагина виден как `<plugin>:<skill>` (`lab-role:lab-skill-1`), плагин — `lab-role@inline` **[П]** `lab/coldstart/logs/native-A3H-probe1.ndjson:3` | — |
| Пути поиска | `~/.claude/skills`, `.claude/skills` (и вложенные), плагин, `.claude/skills` внутри `--add-dir` **[К]** R18:171 | `.agents/skills` в cwd, родителе и корне репо; `~/.agents/skills`; `/etc/codex/skills`; следует симлинкам **[К]** R18:172 |
| Проверка | `system/init.skills`, `.plugins` | методы `skills/list`, `plugin/*` **[К]** R15:152 |

- `--add-dir` для skills подготовлен в лаборатории (`lab/coldstart/role/adddir/.claude/skills/`), но в прогонах не использовался (`native.mjs:56-63`) — **[НП]**.
- Проектные `.claude/skills` при `--setting-sources ""` **не загрузились**: в `repo-role/.claude/skills/` лежали 5 skills, а в `init.skills` есть только пять с префиксом плагина **[П]** по логу `native-A3H-probe1.ndjson:3`; в отчёте 42 это не отмечено.
- Плагин Claude может нести skills, agents, hooks, MCP; плагин codex — skills, mcp_servers, apps, hooks **[К]** R08:218.
- Наследование skills через каталоги-предки для git-worktree не работает: codex останавливается на корне репо **[В]** R18:240.
- `~/.agents/skills` у codex читается из `$HOME`, а не из `CODEX_HOME`; отсекается ли он — **[НП]**.
- Свои skills в сгенерированном `CODEX_HOME/skills/` не испытаны. codex сам создаёт там `.system/` (imagegen, openai-docs, …) **[П]** `lab/coldstart/codex-home/skills/.system/`.
- `AGENTS.md`: codex читает по `cwd` (`instructionSources`) **[П]** R15:150. Claude Code ≥ 2.1.277 читает `AGENTS.md`, если нет `CLAUDE.md` **[К]** R18:169.

## 5. Наследование настроек хоста

### 5.1 claude

| Запуск | Что видно в `system/init` (claude 2.1.322) | Источник |
|---|---|---|
| без флагов (наследование включено) | MCP хоста (плагинный stdio, `excalidraw`, коннектор claude.ai), плагин оператора, 24 skills, `permissionMode: auto`, хук `SessionStart` плагина | **[П]** R42:38,66; `lab/coldstart/out/native-main.jsonl` (A2) |
| `--strict-mcp-config` без `--setting-sources` | MCP нет; плагин оператора и его хук остались | **[П]** R42:23,66; `lab/coldstart/out/pilot.jsonl` (A2 nomcp) |
| `--setting-sources "" --strict-mcp-config` | MCP нет, плагина оператора нет, `permissionMode: default`; остаются 3 плагина `cc-plugin-*` (19 skills) | **[П]** R42:39; `native-main.jsonl` (A1) |

Снято 2026-11-10, claude 2.1.322, по 6 прогонов на строку (вторая строка — 3).
<!-- MEASURE id=claude-setting-sources-residue date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42:36-39,lab/coldstart/out/native-main.jsonl note="числа skills, плагинов и MCP зависят от конфига оператора и версии claude; пересчитать по system/init при обновлении" -->

- Три оставшихся плагина имеют `source: …@builtin` (`native-A3H-probe1.ndjson:3`). R42:39 называет их «пользовательскими» — по логу они встроенные; убрать их флагами нельзя, `--bare` не проверен и не читает OAuth **[НП]** R42:114.
- `--setting-sources` принимает список из `user`, `project`, `local` **[К]** R14:30 (в cmdline адаптера `--setting-sources=user,project,local`). Пустой список отсекает **и хост, и риг**: проектные хуки — 0 срабатываний, `CLAUDE.md` в контекст не попал, проектные skills и команды исчезли **[П-ACP]** R14:85-92; для skills подтверждено нативно (§4). Вариант «без хоста, но с ригом» (`project,local`) — **[НП]**.
- `.mcp.json` рига и MCP хоста отсекает только `--strict-mcp-config` **[П]** R42:23.
- При отсечённом хосте режим разрешений становится `default`: его надо задать явно **[П]** `native-main.jsonl` (A1 против A2).
- Цена наследования (замер 2026-11-10, claude 2.1.322, haiku, n=6): init 2,21 с против 0,25 с; из них ≈1,8 с — подключение MCP хоста (с `--strict-mcp-config` 0,40 с), 0,15 с — хук `SessionStart` плагина **[П]** R42:21-23,60,66.
  <!-- MEASURE id=claude-host-config-start date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42:21-23,60 note="haiku, n=6 (без MCP n=3), медианы от spawn; зависит от набора MCP оператора — перемерить на его текущем конфиге" -->
- Личный stdio-MCP стартует в каждой сессии: 42–62 МБ PSS на сессию (замер 2026-11-10, сервер deja; один экземпляр разросся до 1,09 ГБ) **[П]** R18:10,48-49.
  <!-- MEASURE id=stdio-mcp-per-session date=2026-11-10 ver=gc-a86d05bd7 env=linux-arm64-orbstack src=reports/18:48-49 note="ps + smaps_rollup, 4 сессии claude; версия deja не записана; зависит от набора MCP оператора" -->
- Запись на хост остаётся и при отсечённых настройках: транскрипты `~/.claude/projects/<slug cwd>/`, пустые каталоги `~/.claude/session-env/<id>/`, каталог `memory/`, mtime `~/.claude.json` **[П]** R42:104-105, R36:193.
- Полная изоляция — свой `CLAUDE_CONFIG_DIR` (settings, история, плагины) **[К]** R18:181; тогда нужна доставка учётных данных (§6).
- Запуск из-под сессии Claude Code: лаборатория вычищала из env переменные `CLAUDE*`, `AI_AGENT`, `ANTHROPIC*` **[П]** `native.mjs:9-16`, `lab/wave2-exp/scripts/cenv.sh`. Что будет без чистки — **[НП]**.
- При `--resume` лаборатория передавала весь набор флагов роли заново **[П]** `native.mjs:68,80`. Сохраняются ли флаги сами — **[НП]**.

### 5.2 codex

- Слои, от слабого к сильному: packaged → MDM → system → enterprise → user (`$CODEX_HOME/config.toml`, `--profile`) → project (`.codex/` от `cwd`) → SessionFlags (`-c`, `thread/start.config`) → legacy managed **[К]** R15:164-166.
- Наследование хоста включено, когда app-server запущен с `CODEX_HOME=~/.codex` (умолчание). Подхватывается: `config.toml`, логин, `skills/`, `plugins/`, хуки пользователя, sqlite-состояние **[К]** R15:175, R18:141.
- Отсекается только отдельным `CODEX_HOME`: так шли все опыты **[П]** R15:6, R42:106, `lab/coldstart/scripts/codex.mjs:9`, `exp1.mjs:49`. Это «процессная» часть: один app-server = один `CODEX_HOME`.
- `-c project_root_markers=[]` и `--disable plugins`, названные в R18:238 средством «отсечения личных настроек», пользовательский `config.toml` **не** отсекают. Первое делает рабочий каталог корнем проекта (не грузится `AGENTS.md` каталогов-предков), второе выключает плагины оператора **[К]** комментарий в `gascity-packs/packs/servitors/pack.toml:19-23`. Под app-server не испытано.
- Побочный эффект: codex сам переписывает `config.toml` своего `CODEX_HOME` — хэши доверия хуков (`[hooks.state]`), `[tui] screen_reader_detection_done`, `[tui.model_availability_nux]` **[П]** FR:875, `lab/codex/home/config.toml` (хвост файла). На хосте `~/.codex/config.toml` за время исследования переписан дважды (2026-11-10, 10:13 и 11:01 +07), оба раза не лабораторией с изолированным `CODEX_HOME` **[П]** FR:875, R42:107. Причина второго раза в R36:194 записана как не установленная, в R42:107 — сессия codex-tui живого города.
- В изолированных `CODEX_HOME` лаборатории для каждого `cwd` записано `[projects."<cwd>"] trust_level = "trusted"` **[П]** `lab/codex/home/config.toml`, `lab/wave2-exp/codex-home/config.toml`. Обязательно ли это для загрузки проектного `.codex/` — **[НП]**.
- Независимо от `CODEX_HOME` codex на первом ходе запускает login-shell пользователя для снимка окружения (`zsh -lc`), и команды идут через него **[П]** R15:135-137, `lab/codex/out/e10_run.txt:1`. То есть `~/.zshrc` хоста влияет на команды всегда.
- Провайдер модели — на процесс: тред с другим `modelProvider` отклонён `modelProviderMismatch` (codex 0.166.0) **[П]** R15:176-177.

## 6. Учётные данные при отсечённом хосте

| Случай | Что известно | Метка |
|---|---|---|
| claude, `--setting-sources ""`, `CLAUDE_CONFIG_DIR` не менялся | логин подписки хоста работает без доставки: `apiKeySource: "none"`, ходы прошли | **[П]** `native-A3H-probe1.ndjson:3`, R42:21 |
| claude, свой `CLAUDE_CONFIG_DIR`, ничего не доставлено | отказ `-32000 Authentication required` за 100 мс (замер 2026-11-10, claude 2.1.321 через ACP) | **[П-ACP]** R14:95 |
| claude, свой `CLAUDE_CONFIG_DIR` + `CLAUDE_CODE_OAUTH_TOKEN` в env (значение `claudeAiOauth.accessToken` из `~/.claude/.credentials.json`), `env -i` | нативный `-p` отработал, `apiKeySource: "none"` (опыт 2026-11-11, claude 2.1.322, прогоны R1, R2) | **[П]** R46a:19, `lab/observability/claude/scripts/run.sh:24,31-45`, `runs/r1/stream.jsonl` |
| claude, API-ключ | `ANTHROPIC_API_KEY` (+ `ANTHROPIC_BASE_URL` для поддельного API) в env | **[П]** `run.sh:22` |
| claude, долгоживущий токен | `claude setup-token` — годовой OAuth-токен подписки; `--bare` OAuth не читает | **[К]** R14:155, R34:240 |
| claude, проверка до старта | `claude auth status --json` | **[К]** R34:317 |
| codex, свой `CODEX_HOME`, кастомный `model_providers.<n>` с `env_key` | работает; `auth.json` в каталоге нет | **[П]** R15:8-9, `lab/coldstart/codex-home/config.toml:5-11` |
| codex, логин ChatGPT в своём `CODEX_HOME` | не проверялось: на стенде `codex login status` → `Not logged in` | **[НП]** R15:8-9,238 |
| codex, проверка до старта | `codex login status`; методы `account/read`, `account/login/start` | **[К]** R34:317, R15:74 |

<!-- MEASURE id=claude-noauth-refusal date=2026-11-10 ver=claude-2.1.321 env=linux-arm64-orbstack src=reports/14:95 note="один прогон через ACP-адаптер, options.env.CLAUDE_CONFIG_DIR на пустой каталог; на нативном -p не снималось" -->

Не проверено: срок жизни и обновление access-токена claude, взятого из `.credentials.json` (в опыте — два коротких прогона `-p`); истечение OAuth посреди сессии **[НП]** R14:239, R34:317; перенос логина при смене `CLAUDE_CONFIG_DIR`/`CODEX_HOME` копированием файлов **[НП]** R08:296; macOS keychain **[НП]**.

## 7. Рабочий пример: роль из опыта холодного старта (R42, конфигурация A3)

Только claude, нативный `-p`, 2026-11-10, claude 2.1.322, n=6 + пробник хуков. Сборка — `lab/coldstart/scripts/native.mjs:56-82`:

```
cwd = <репозиторий роли>            # env: окружение родителя без CLAUDE*, AI_AGENT, ANTHROPIC*
claude -p "<промпт>" --output-format stream-json --verbose --include-partial-messages --model haiku \
  --setting-sources "" --strict-mcp-config \
  --mcp-config <role>/mcp.json --settings <role>/settings.json \
  --append-system-prompt "<9,5 КБ текста>" --plugin-dir <role>/plugin  [--resume <session_id>]
```

```json
// <role>/mcp.json — HTTP-MCP с заголовками идентичности и stdio-MCP
{"mcpServers": {
  "city": {"type": "http", "url": "http://127.0.0.1:38659/mcp", "headers": {"X-City-Agent": "coldstart", "X-City-Rig": "lab"}},
  "mini": {"command": "python3", "args": ["<abs>/mini_mcp.py"]}}}
// <role>/settings.json — три хука
{"hooks": {"SessionStart":     [{"hooks": [{"type": "command", "command": "true"}]}],
           "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "true"}]}],
           "Stop":             [{"hooks": [{"type": "command", "command": "true"}]}]}}
// <role>/plugin/.claude-plugin/plugin.json + <role>/plugin/skills/lab-skill-{1..5}/SKILL.md
{"name": "lab-role", "version": "0.0.1", "description": "…"}
```

Проверка сборки — по `system/init` **[П]** R42:37, `native-A3H-probe1.ndjson:3`: `mcp_servers=[city:connected, mini:connected]` (`source: "dynamic"`), `plugins` содержит `lab-role` c путём каталога, 5 skills `lab-role:lab-skill-N`, `tools` содержит `mcp__city__whoami`, `mcp__mini__whoami`; до `init` приходит `hook_started SessionStart`.

Цена роли (замер 2026-11-10, claude 2.1.322, haiku, n=6): init 0,28 с [0,26–0,34] против 0,25 с у голого запуска; разность +0,03 с [0,00–0,11] — в шуме. RSS процесса 231 МБ, дерева с stdio-MCP 249 МБ **[П]** R42:21,24,61.
<!-- MEASURE id=claude-role-config-start date=2026-11-10 ver=claude-2.1.322 env=linux-arm64-orbstack src=reports/42:21,24,61 note="haiku, n=6, медианы от spawn; хуки-пустышки (true), локальный HTTP-MCP, 5 skills-заглушек; перемерить на настоящих хуках и моделях ролей" -->

Эквивалент для codex в опыте холодного старта **не собирался**: codex мерился с пустым `CODEX_HOME` без роли (`codex.mjs:9-10`). Ниже — склейка из трёх отдельных опытов, целиком не запускалась **[В]**:

```
CODEX_HOME=<сгенерированный каталог: config.toml (+ skills/)>  codex app-server          # процессная часть
→ initialize {clientInfo, capabilities:{experimentalApi:true}}; notify initialized
→ thread/start {cwd, model, sandbox, approvalPolicy, developerInstructions,                # e3:46-51
     config: { mcp_servers: { city: {url, http_headers:{…}},                               # exp1.mjs:63
                              mini: {command, args, env} },                                # e3:50-51
               shell_environment_policy: {set:{K:"V"}},                                    # e10:32
               bypass_hook_trust: true } }                                                 # e3:43; хуки — см. §3.4
```

## 8. Расхождения между источниками

| Тема | Значение 1 | Значение 2 |
|---|---|---|
| Учётные данные claude при своём каталоге конфигурации | `AAE-SPEC.md` §6.4: способ доставки **[НП]** | R46a:19 + `run.sh:24`: `CLAUDE_CODE_OAUTH_TOKEN` в env сработал нативно **[П]** |
| Переподключение claude к MCP | `AAE-SPEC.md` §11.4: 7–26 с; R36:73-78: 6,8 / 15,7 / 20,6 / 25,7 (4-й — провал хода) | FR:493, FR:784-785: 7–21 с |
| Число событий хуков codex | R08:215: 11 | R18:138, R15:181, R22:52: 14 |
| Хуки codex оверрайдом треда | R15:31-32: «`config`-оверрайды (хуки, …)» | E3 брал хуки из `ws2/.codex/hooks.json`; сквозной путь **[НП]** R18:297, FR:859 |
| Отсечение хоста у codex | R18:238,250: `project_root_markers=[]` + `--disable plugins` | комментарий пака владельца: это про `AGENTS.md` предков и плагины; конфиг пользователя отсекает только свой `CODEX_HOME` |
| Делит ли codex stdio-MCP | R18:13: да (`McpConnectionPool`) | R15:36, R22:147-150: в 0.166.0 — нет |
| Плагины `cc-plugin-*` при `--setting-sources ""` | R42:39: «пользовательские» | лог `init`: `source: …@builtin` |
| Причина перезаписи `~/.codex/config.toml` в 11:01 | R36:194: не установлена | R42:107, FR:875: сессия codex-tui живого города |

## 9. Чего здесь нет

Выяснять опытом (опыты 5 и 6 из `AAE-SPEC.md` §16); читать отчёты целиком для этого не нужно.

- codex: хуки через `thread/start.config.hooks`; через `-c hooks.*` и через `hooks.json` в `CODEX_HOME` под app-server; поведение недоверенного хука без `bypass_hook_trust`; окружение хуков при нескольких тредах; `PreToolUse`/`PostToolUse` с блокировкой.
- codex: свои skills в `CODEX_HOME/skills/`; влияние `~/.agents/skills`; логин ChatGPT в отдельном `CODEX_HOME`; транспорт на unix-сокете (все опыты этого файла, кроме `lab/wave2-exp/scripts/exp3.py`, шли по stdio).
- claude: `--setting-sources project,local`; `--add-dir` для skills; хуки в плагине; `--settings` inline; `/mcp reconnect` и задержка переподключения MCP в нативном `-p`; обновление OAuth-токена в изолированном каталоге; `headers` с `${VAR}`.
- Блокирующие хуки (код 2, `decision: "block"`, `updatedInput`) ни в одном опыте не исполнялись: формат stdout взят из документации и схем. Точные схемы codex — `src/codex/codex-rs/hooks/schema/generated/*.json` (клон в `tmp/gc-alt-research/src/`).
- Поведение обоих харнессов с реальным SDK-сервером MCP эпохи 2026-07-28; нагрузка на общий сервер; рестарт stateless-сервера прежней эпохи для codex.
- Всё на macOS.
- Шлюзы и мосты stdio→HTTP (supergateway и др.) не запускались. Если понадобится мост — таблица кандидатов в R18:92-114 (один раздел, только по README).
- Протокол самих харнессов (stream-json, JSON-RPC app-server), сбои и resume — в других файлах `aae-context/`.
