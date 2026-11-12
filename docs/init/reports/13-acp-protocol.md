# 13. Протокол ACP (Agent Client Protocol) как единственный транспорт оркестратора — состояние на 2026-11-10

Сокращения путей: `ACP/` = `/home/ubuntu/proj/work/gascity/tmp/gc-alt-research/src/agent-client-protocol` (HEAD `ab82539`, 2026-11-09); `CAA/` = `.../src/claude-agent-acp` (HEAD `2f7e386`, 2026-11-09); `CODEX/` = `.../src/codex` (HEAD `aa39ded`); `PI/` = `.../src/pi-mono`; `GC/` = `/home/ubuntu/proj/work/gascity`.
Метки: **[П]** = ПРОВЕРЕНО (сам видел в коде/документе/выводе), **[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО.
Статусы фич: **stable** (в `schema/v1/schema.json`), **unstable** (только в `schema.unstable.json`), **v2-draft**, **RFD** (Draft/Active или открытый PR), **нет**.

## 1. Выводы за 60 секунд

- Протокол не «хромоногий», а **узкий**: v1 stable — зрелый, быстро растущий стандарт «редактор ↔ один агент» (45 агентов в реестре, 100+ клиентов), но ровно те вещи, которые нужны оркестратору, в нём либо отсутствуют, либо лежат в draft/RFD: steering/очередь, типизированные ошибки лимитов, remote transport, multi-client, liveness, конфиг хуков/скиллов/env.
- **Существуют две версии.** v1 — стабильная (protocolVersion=1, schema v1.30.0 от 2026-11-09). **v2 — Draft с 2026-07-20**, ломает именно «ход»: `session/prompt` отвечает при вставке сообщения, состояние идёт через `state_update` (`running`/`idle`/`requires_action`), появляется `stopReason: "error"`, prompt во время работы нельзя отклонять. v2 явно создан под фоновые/длинные агенты, но в проде его включать запрещено самим анонсом.
- **Транспорт — только stdio.** Streamable HTTP/WebSocket — RFD в статусе Active, в спеку не внесён; durability/keepalive отложены ещё дальше.
- **Steering/очередь:** в v1 поведение второго `session/prompt` во время хода не определено; стандартно только `session/cancel` + новый prompt. Официальные адаптеры claude и codex решают это нестандартным расширением `_session/steering`.
- **Лимиты/квоты/429:** стандартных кодов нет. Ошибка хода в v1 — обычно `-32603 Internal error` с текстом; RFD «error classification» и «quotas & limits» открыты как PR 3–5 недель назад. Сейчас это всё через `_meta`/`data` конкретного адаптера.
- **Мультисессионность разрешена спекой** («one Agent can run several independent sessions»), `cwd` задаётся на сессию; `env` на сессию нет. Но экономия памяти (H4) зависит от адаптера: `claude-agent-acp` поднимает отдельный процесс Claude Code на каждую сессию.
- **Ни один из трёх критичных харнессов не говорит на ACP нативно.** claude и codex — адаптеры организации ACP (Zed+JetBrains) поверх Agent SDK и Codex app-server; pi — community-адаптер `svkozak/pi-acp` 0.0.34, 98 открытых issues, с известным багом убийства параллельных сессий; автор pi от нативного ACP отказался.
- **Протокол 1:1 по соединению.** RFD «Multi-Client Session Attach» (PR #533) открыт с февраля 2026 и не принят; реализуется сторонними прокси (hydra-acp, acp-multiplex). H10 выполнима только если оркестратор сам станет ACP-прокси/мультиплексором.
- `session/request_permission` — рабочая универсальная точка «PreToolUse-политики»: спека прямо разрешает клиенту отвечать автоматически. Но это только allow/deny до вызова; PostToolUse, Stop, SessionStart и др. хуков в протоколе нет.
- Gas City использует из ACP шесть методов уровня 2025 года (`initialize`, `session/new`, `session/prompt`, `session/cancel`, `session/update`, `session/request_permission`) — без `load/resume/close/list`, config options, `usage_update`, `systemPrompt`. Значительная часть «ACP не работает в GC» — недоиспользование протокола, а не его дефект.
- Рекомендация: **внутренний интерфейс драйвера с семантикой ACP v2**, реализации: ACP (claude, codex) + нативный `pi --mode rpc`. «ACP как единственный транспорт» сегодня упирается в pi и в зависимость от `_meta`-расширений.

## 2. Факты

### 2.1 Модель, версии, управление

| Вопрос | Ответ | Источник |
|---|---|---|
| Роли | Client (обычно IDE) запускает Agent как подпроцесс; JSON-RPC 2.0 в обе стороны | [П] `ACP/docs/protocol/v1/overview.mdx:45-47,116-118` |
| Транспорт | **stable: только stdio**, newline-delimited. Remote «is being specified in the transport RFD». Custom transports разрешены | [П] `ACP/docs/protocol/v1/transports.mdx:8,14-24,43-47` |
| Remote transport | **RFD Active** (Streamable HTTP + WebSocket на `/acp`, требуется HTTP/2 и cookies). В v1: «In-flight messages are not replayed», reconnect и liveness «up to the implementer». Durable-вариант — отдельный PR #2208 (открыт) | [П] `ACP/docs/rfds/streamable-http-websocket-transport.mdx:11-18,70-80`; `gh pr view 2208` |
| Версия | Целое число; v1 stable; **v2 Draft с 2026-07-20**: «Don't ship it by default in production»; «Adding v2 support should not mean dropping v1» | [П] `ACP/docs/announcements/acp-v2-draft.mdx:9,59-63` |
| Capabilities | `initialize` обменивается `clientCapabilities`/`agentCapabilities`; опциональные методы запрещено звать без capability | [П] `ACP/docs/protocol/v1/session-setup.mdx:91-106` |
| Расширяемость | `_meta` на большинстве типов; кастомные методы с префиксом `_`; кастомные capabilities в `_meta`; корневые поля добавлять нельзя | [П] `ACP/docs/protocol/v1/extensibility.mdx:6,43,47,117` |
| Управление | «Jointly governed by Zed and JetBrains», цель — независимый фонд. Lead: Ben Brandt (Zed), Sergey Ignatov (JetBrains). Изменения через RFD: Draft → Active → Preview → Completed | [П] `ACP/docs/community/governance.mdx:6-16`; `ACP/MAINTAINERS.md`; `ACP/docs/docs.json` (группы RFD) |
| Темп | schema v1.13.7 (2026-06-16) → v1.30.0 (2026-11-09): 17 minor-релизов за 5 месяцев; ~20 фич стабилизировано за 2026 год. Wire-версия v1 при этом не ломалась | [П] `ACP/schema/v1/CHANGELOG.md:10,208`; `ACP/docs/updates.mdx` |
| SDK | TS v2.5.0, Rust v5.1.0, Python 1.0.0 (все 2026-11-09), Kotlin v0.34.0, Java v0.80.0. **Официального Go SDK нет** (7 community-библиотек) | [П] `gh api repos/agentclientprotocol/*/releases`; `ACP/docs/libraries/community.mdx` |
| Conformance | `acp-tck` — «experimental and does not cover the full protocol… not an official statement of ACP conformance» | [П] `ACP/docs/libraries/testing.mdx` |

### 2.2 Методы и нотификации со статусом

Источник списков: [П] `ACP/schema/v1/meta.json`, `meta.unstable.json`, `ACP/schema/v2/meta*.json`; статусы RFD — `ACP/docs/docs.json`.

| Метод / update | v1 stable | v1 unstable | v2-draft | Примечание |
|---|---|---|---|---|
| `initialize`, `session/new`, `session/prompt`, `session/cancel`, `session/update` | да | да | да | базовые |
| `authenticate`, `logout` | да | да | `auth/login`, `auth/logout` | |
| `session/load` (replay истории) | да (capability `loadSession`) | да | **удалён** → `session/resume` + `replayFrom` | |
| `session/resume` | да (стаб. 2026-04-22) | да | обязателен | |
| `session/close` | да (2026-04-23), идемпотентен | да | обязателен | |
| `session/list` / `session/delete` | да (2026-03-09 / 06-05) | да | list обязателен | |
| `session/set_mode` | да, но помечен к удалению | да | **удалён** | замена — config options |
| `session/set_config_option` | да (2026-02-04) | да | да | категории `mode`, `model`, `model_config`, `thought_level` |
| `session/set_model` | нет | нет | нет | модель — через config option `category: model` |
| `session/fork`, `session/archive`, `session/status`, `session/set_title` | нет | да (RFD Draft) | unstable | |
| `providers/*`, `auth/status` | нет | да | unstable | |
| `session/request_permission` | да | да | да (с `title`/`subject`) | |
| `fs/read_text_file`, `fs/write_text_file`, `terminal/*` | да (capability клиента) | да | **удалены** | в v2 терминал только display-only |
| `elicitation/create`, `elicitation/complete` | да (2026-07-22) | да | да | |
| `$/cancel_request` | да (2026-06-29) | да | да | |
| `mcp/message` (MCP-over-ACP) | нет | да (RFD Active) | unstable | |
| `proxy/initialize`, `proxy/successor` | нет | да (RFD Draft) | unstable | |
| `tool_call/cancel`, `terminal/cancel` | нет | нет | да | |
| Steering/queue, multi-client attach, quotas, typed errors, skills, hooks | **нет** | **нет** | нет | только открытые PR (см. 2.5) |

`session/update` в v1 stable: `user_message_chunk`, `agent_message_chunk`, `agent_thought_chunk`, `tool_call`, `tool_call_update`, `plan`, `available_commands_update`, `current_mode_update`, `config_option_update`, `session_info_update`, `usage_update`, `notice`, `compaction_update`, `compaction_summary_chunk`. Unstable добавляет `plan_update`, `plan_removed`, `subagent_update`, `session_message(_chunk)`. v2 добавляет `state_update`, `user_message`, `agent_message`, `agent_thought`, `tool_call_content_chunk`, `terminal_update`, `terminal_output_chunk`; убирает `tool_call`, `plan`, `current_mode_update`. [П] извлечено скриптом из `$defs.SessionUpdate` четырёх схем; описание — `ACP/docs/protocol/v1/prompt-turn.mdx:14-29`.

### 2.3 Вопросы, критичные для оркестратора

**a. Много сессий в одном процессе.** Разрешено: «Each session has its own context, conversation history, and state, so one Agent can run several independent sessions» [П] `ACP/docs/protocol/v1/session-setup.mdx:6`. `cwd` — на сессию и «MUST be used for the session regardless of where the Agent subprocess was spawned» [П] там же `:441`; плюс `additionalDirectories` `:331-362`. Значит один процесс формально может обслуживать разные rig/worktree. Параллельные `session/prompt` в разных сессиях нормативно не описаны (ни MUST, ни запрета) — допустимы по JSON-RPC [В]. **`env` на сессию нет**: поля `NewSessionRequest` = `cwd`, `additionalDirectories`, `mcpServers`, `systemPrompt`, `_meta` [П] `ACP/schema/v1/schema.json` (`$defs.NewSessionRequest`). Env есть только у stdio-MCP-серверов.

**b. Инъекция сообщения во время хода.** v1: `session/prompt` длится до конца хода и возвращает `stopReason`; про второй prompt во время хода спека молчит [П] `ACP/docs/protocol/v1/prompt-turn.mdx:452,584`; grep по `queue|steer|concurren` в `docs/protocol/v1` пуст. Стандартный путь — `session/cancel` и новый prompt. v2-draft: «Clients MAY send `session/prompt` in any state… Agents MUST NOT reject a prompt only because foreground work is active», но «what inserting during active work means, such as steering the current turn or starting the next one, is up to the Agent… Future capabilities may let Agents advertise steering or queueing» [П] `ACP/docs/protocol/v2/prompt-lifecycle.mdx:301-303`. RFD: PR #1261 «steer and queue prompts» (открыт с 2026-05-19, поле `delivery: steer|queue`), PR #2255, PR #2043. В PR #1261 пользователь Raft пишет: «the absence of steering is currently an adoption blocker for ACP» [П] `gh pr view 1261`. На практике: `claude-agent-acp` объявляет `_meta.claudeCode.promptQueueing: true` и метод `_session/steering` [П] `CAA/src/acp-agent.ts:710,3085,3120-3122`; `codex-acp` — «Mid-turn input through the _session/steering extension» [П] README `agentclientprotocol/codex-acp`. OpenClaw issue #154349: `/acp steer` через чистый ACP превращается в follow-up после хода [П] https://github.com/openclaw/openclaw/issues/154349.

**c. Восстановление.** `session/load` — агент «MUST replay the entire conversation» через `session/update`, затем ответить [П] `session-setup.mdx:136,191`. `session/resume` — «MUST NOT replay» [П] `:258`. Оба опциональны в v1 (capabilities `loadSession`, `sessionCapabilities.resume`). Смерть процесса посреди хода в v1 не описана вовсе; v2: «Clients detect a lost connection or an exited Agent process themselves. No update can report them» [П] `ACP/docs/protocol/v2/prompt-lifecycle.mdx:892`; после resume агент обязан прислать снимок состояния, а при обрыве соединения «MAY continue or cancel their foreground work» [П] `ACP/docs/protocol/v2/session-setup.mdx:54-55`. Идемпотентности prompt нет: «The ID is an acknowledgment, not a client-supplied idempotency key… does not promise automatic deduplication or exactly-once processing» [П] `ACP/docs/rfds/v2/prompt.mdx:391-393`. Идемпотентны только `session/close` и `session/delete`.

**d. Ошибки и лимиты.** Коды v1: `-32700/-32600/-32601/-32602/-32603`, `-32800` (cancelled), `-32000` (auth required), `-32002` (not found) — и всё [П] `ACP/docs/protocol/v1/error.mdx:26-37`. `StopReason` v1: `end_turn`, `max_tokens`, `max_turn_requests`, `refusal`, `cancelled` [П] `prompt-turn.mdx:529-545`; v2 добавляет `error` с JSON-RPC-объектом [П] `v2/prompt-lifecycle.mdx:819-892`. Rate limit / quota / overloaded / context overflow / retry-after — **нет**: «More specific categories, such as rate limits, an exhausted context window, or an overloaded provider, can be added later» [П] `ACP/docs/rfds/v2/prompt.mdx:320`; «Rate limits and quotas are separate concerns that could be addressed in a future RFD» [П] `ACP/docs/rfds/session-usage.mdx:207-209`. Открытые PR: #2755/#2773 «error classification» (`kind`, `retryable`, `retryAfterMs`; создан 2026-11-03, в тексте: «most failures use `-32603`… It cannot tell a rate limit from an exhausted usage plan»), #2316 «quotas & limits» (2026-10-06) [П] `gh pr view`. Usage: `usage_update` (`used`, `size`, `cost`) — stable с 2026-06-05; per-turn токены (`PromptResponse.usage`) — unstable. Адаптер claude передаёт лимиты своим способом: `data.errorKind` и `_meta["_claude/rateLimit"]` [П] `CAA/src/acp-agent.ts:4682-4690,8739-8752,13430-13444`.

**e. MCP.** `mcpServers` в `session/new|load|resume`. stdio — «All Agents MUST support»; HTTP — capability `mcpCapabilities.http`; SSE deprecated; «Agents SHOULD connect to all MCP servers specified by the Client»; «Clients MAY use this ability to provide tools directly… by including their own MCP server» [П] `session-setup.mdx:459,512,563,629-631`. MCP-over-ACP (`type: "acp"`, `mcp/message`) — unstable, RFD Active [П] `ACP/docs/rfds/mcp-over-acp.mdx:1-60`. **«Общий MCP на город» протокол позволяет**: оркестратор передаёт каждой сессии один и тот же `{type:"http", url, headers}` [В из `:510-557`]. Пробел: клиент не видит и не может отключить MCP-серверы из собственного конфига агента — PR #2446 (открыт) [П].

**f. Разрешения.** «Clients MAY automatically allow or reject permission requests according to the user settings» [П] `ACP/docs/protocol/v1/tool-calls.mdx:194`. В запросе — `toolCall` (kind, title, locations, rawInput, с 09.2026 — программное `name`) и варианты `allow_once/allow_always/reject_once/reject_always`. Это годная точка PreToolUse-политики, с оговорками: агент запрашивает разрешение по своему усмотрению («MAY request»), изменить аргументы нельзя, свободный текст отказа — PR #1983 (открыт). Режимы не стандартизованы: идентификаторы свои у каждого агента (в спеке пример `ask/architect/code`; у codex-acp — `read-only`, `workspace-write`, `agent`, `agent-full-access`), а отдельный API modes помечен к удалению [П] `ACP/docs/protocol/v1/session-modes.mdx:6-11,24-44`; README codex-acp.

**g. Наблюдаемость.** В v1 «агент работает» = между запросом `session/prompt` и ответом; «ждёт человека» = висящий `request_permission`/`elicitation`; инструмент = `tool_call`/`tool_call_update` со статусом `pending/in_progress/completed/failed`. Экран и логи читать не нужно [В из `prompt-turn.mdx:39-78`]. Отдельного состояния сессии в v1 нет; в v2 — `state_update` плюс список фоновых работ `background` [П] `v2/prompt-lifecycle.mdx:98-135,894-960`. Heartbeat/ping в протоколе нет; keepalive отложен в transport RFD [П] `ACP/docs/rfds/streamable-http-websocket-transport.mdx:77-78,88`. Субагенты в v1 stable не видны как сущности («Agents therefore have to flatten subagent activity into the parent session, hide it, or encode it in custom tool calls») [П] `ACP/docs/rfds/subagents.mdx:41-43`; `subagent_update` — unstable (RFD Active), реализован обоими официальными адаптерами за capability.

**h. Конфиг агента.** В протоколе есть: `systemPrompt` (`append`; `override` за capability) — stable с 2026-11-06 [П] `session-setup.mdx:364-403`; модель/effort/mode — через `configOptions`, значения у каждого агента свои. **Нет**: skills (PR #370 открыт с 2026-01-10; 2026-10-26 Ben Brandt: «needs a substantial rework»), hooks, settings, env, tool allow-list. Всё это — аргументы/env запуска адаптера или `_meta` (например `_meta.claudeCode.options` [П] `CAA/src/acp-agent.ts:3159`).

**i. Несколько клиентов на одну сессию.** v1: сессия живёт в рамках соединения; `session/load` даёт «sharing sessions between different Client instances» только последовательно [П] `session-setup.mdx:87`. v2: «Whether a session can be active on more than one connection at once is up to the Agent» [П] `v2/session-setup.mdx:54`. RFD «Multi-Client Session Attach» — PR #533, открыт с 2026-02-18, вынесен в прокси-слой; рабочие реализации — сторонние `hydra-acp`, `acp-multiplex`; у каждого свой способ обнаружения сокета [П] `gh pr view 533`. Proxy chains (`proxy/successor`, conductor) — RFD Draft [П] `ACP/docs/rfds/proxy-chains.mdx:11-70`.

**j. Прочее.** Slash-команды: `available_commands_update`, вызов — обычным текстом prompt [П] `ACP/docs/protocol/v1/slash-commands.mdx:8,75`. Контент prompt: baseline `text` и `resource_link`; `image`, `audio`, embedded `resource` — по `promptCapabilities` [П] `ACP/docs/protocol/v1/content.mdx:20-166`. Structured output — нет (discussion #2147).

### 2.4 Экосистема

- Реестр: 45 агентов [П] `lab/acp-protocol/registry/registry.json` (скачан с `cdn.agentclientprotocol.com`, 2026-11-10). Нативно (бинарь самого вендора): gemini, github-copilot-cli, cursor, goose, kiro, opencode, devin. **Через адаптер**: `claude-acp` 0.102.0 (`@agentclientprotocol/claude-agent-acp`), `codex-acp` 2.6.0 (`@agentclientprotocol/codex-acp`), `pi-acp` 0.0.34 (`svkozak/pi-acp`).
- claude: адаптер держит по одному `query()` Agent SDK на сессию, каждый запускает Claude Code CLI [П] `CAA/src/acp-agent.ts:12330,12487`; «A session runs one turn at a time»; адаптер обслуживает v1 и v2 [П] `CAA/CLAUDE.md`.
- codex: «It starts the Codex App Server, translates ACP requests into Codex operations» [П] README. **Локальный клон `src/codex-acp` — это архивный `zed-industries/codex-acp` (Rust, последний push 2026-07-22, `archived=true`); актуальный адаптер живёт в `agentclientprotocol/codex-acp`** [П] `git remote -v`, `gh api repos/zed-industries/codex-acp`.
- pi: нативного ACP нет (grep по `PI/packages/coding-agent` пуст). Mario Zechner, issue #175, 2025-12-13: «I currently have no need for ACP support, so that's a no for me… It should however be trivial to build an ACP adapter on top of RPC mode» [П] `gh api repos/earendil-works/pi/issues/175/comments` (репозиторий переехал в `earendil-works/pi`). Адаптер: 98 открытых issues, последний commit 2026-10-06; PR #131 «preserve concurrent sessions and settle terminated turns» открыт с 2026-09-15 [П] `gh api repos/svkozak/pi-acp`. Сценарий бага: `session/load B` → `closeAllExcept(B)` → воркер сессии A убит, её prompt не завершается никогда [П] discussion earendil-works/pi#4444, комментарий 2026-09-20. Там же, 2026-11-02: «pi-acp is not actively maintained».
- Клиенты: Zed, JetBrains, neovim, Emacs (`acp.el`), Toad, acpx, десятки desktop/web/mobile/мессенджер-мостов и оркестраторов (Orca, Raven, SpurCode, Jockey, CompozyOS) [П] `ACP/docs/get-started/clients.mdx`.

### 2.5 Слабости по мнению сообщества

| Тема | Свидетельство |
|---|---|
| Нет steering | PR #1261 (9 комментариев, «adoption blocker»), discussion #1220, openclaw#154349 |
| Нет multi-client | PR #533 (11 комментариев, с 02.2026) |
| Ошибки неразличимы | PR #2755, #2773, #2316; discussion #2235 «Rate-limit and plan-quota windows» |
| Нет remote/durable transport | RFD Active + PR #2208; discussion #46 (с 08.2025) |
| Resume не идемпотентен, нет курсора | discussions #1824, #1783; PR #2407 (`replay_complete`), #2394 |
| Удаление fs/terminal в v2 | discussions #2299, #1481 |
| Адаптерный ACP ненадёжнее нативного | zeron#466 (2026-09-19): «adapter-mediated ACP kept manufacturing done-status bugs the native wires don't have»; Claude, Codex и Cursor там уже переведены на нативные драйверы [П через WebFetch] https://github.com/zeronsh/zeron/issues/466 |
| Адаптер = граница совместимости | casys.ai (2026-03-23): адаптер claude — «a real compatibility boundary» [П через WebFetch] https://casys.ai/blog/acpx-multi-agent-orchestration |

Открытых issues в репозитории спеки всего 5, обсуждение идёт в 158 discussions и 27 открытых PR [П] `gh api graphql`. Прямой критики ACP от Will McGugan (Toad) и авторов OpenCode поиском не найдено [НП].

### 2.6 ACP против нативных headless-протоколов

| Возможность | ACP v1 stable | Claude `stream-json` / Agent SDK | Codex `app-server` | pi `--mode rpc` |
|---|---|---|---|---|
| Формат | JSON-RPC 2.0, stdio | JSONL + control-запросы, stdio | JSON-RPC; `stdio://`, `unix://`, `ws://IP:PORT`; есть `daemon` и `proxy` | JSONL (не JSON-RPC), stdio |
| Много сессий в процессе | да по спеке | нет, процесс = сессия | да (threads) | нет (`new_session` заменяет текущую) [В] |
| Steering | нет (расширение `_session/steering`) | да (стрим входных сообщений) [НП в деталях] | `turn/steer`, `thread/queue/*` | `steer`, `follow_up`, `clear_queue` |
| Типизированные ошибки | нет | категориальный `error` + `rate_limit_event` | `CodexErrorInfo`: `UsageLimitExceeded`, `RateLimitExceeded`, `ContextWindowExceeded`, `ServerOverloaded`… ; `account/rateLimits/read` | `set_auto_retry`, `abort_retry` |
| Multi-client | нет | Remote Control (отдельный механизм) [НП] | да, через daemon/control socket [В] | нет |
| Hooks / skills / settings | нет | да (`--settings`, `--agents`, хуки) | `skills/list`, `config/*` | extensions |
| Один клиентский код на все харнессы | **да** | нет | нет | нет |

Источники: [П] `CODEX/codex-rs/app-server-protocol/src/protocol/common.rs` (список методов), `.../v2/shared.rs:76-104`, `codex app-server --help`; [П] `PI/packages/coding-agent/docs/rpc-commands.md`; [П] `claude --help` (2.1.322).

ACP **теряет**: steering, типизированные лимиты, конфиг хуков/скиллов, multi-client, часть событий — и возвращает их только через `_meta`, то есть уже не единым кодом. ACP **даёт**: общую модель сессии/хода/tool call/permission, готовые SDK (кроме Go), возможность подключить любой из 45 агентов и любой готовый клиент как UI.

## 3. Вердикты

**H2 (ACP плох в Gas City из-за Gas City, а не протокола) — частично подтверждена, обе причины реальны.** Со стороны Gas City: используется шесть методов, без resume/load/close/list/config/usage [П] grep по `GC/internal/runtime/acp/*.go`. Со стороны протокола — объективные дыры именно для оркестрации: steering, классификация ошибок, liveness, multi-client, remote transport, env/hooks. Формулировка для владельца: протокол не хромой, он ходит по другому маршруту — «человек в редакторе рядом с одним агентом». v2 сдвигает его к оркестрации, но это Draft.

**H3 (claude, codex, pi нормально работают через ACP; хватит как единственного транспорта) — опровергнута в части pi, частично по claude/codex.** Нативно не поддерживает никто из трёх. claude и codex — активно развиваемые адаптеры (релизы 2026-11-09), но оркестраторные функции в них — нестандартные расширения (`_session/steering`, `errorKind`, AIR-extensions под JetBrains). pi — полузаброшенный community-адаптер с багом параллельных сессий.

**H4 (один ACP-сервер на много сессий ⇒ экономия памяти) — спекой разрешено, на практике зависит от адаптера.** Для claude экономии нет: процесс Claude Code на сессию плюс процесс адаптера сверху. Для codex вероятна (один app-server, много threads) [НП — не измерял]. Для pi — процесс на сессию. Кроме того, `env` на сессию отсутствует, поэтому разные наборы credentials/identity всё равно требуют разных процессов.

**H10 (человек подключается к ACP как к control pane) — частично.** Напрямую к той же живой сессии второй клиент подключиться не может: stdio 1:1, attach не стандартизован. Реализуемо двумя путями: (1) оркестратор — ACP-прокси, раздающий сессии наружу (так делают hydra-acp, acp-multiplex; придётся самим определить broadcast prompt'ов, гонку за permission, обнаружение сокета); (2) последовательная передача: оркестратор закрывает сессию, человек делает `session/load` из своего клиента. Плюс: любой готовый клиент (Zed, Toad, acpx, мобильные) становится UI бесплатно.

**H1 (со стороны протокола).** Обнаружение краша без tmux тривиально: EOF на stdio + PID. Но «outOfQuota/429» ACP стандартно не различает — нужен разбор `_meta`/`data` на адаптер либо нативный протокол.

## 4. Следствия для дизайна и варианты

| Вариант | Плюсы | Минусы | Оценка |
|---|---|---|---|
| **A. ACP — единственный транспорт** | Один клиентский код; чужие клиенты как UI | pi через сломанный адаптер; лимиты и steering всё равно через per-harness `_meta`; нет Go SDK; v1→v2 миграция впереди | Не рекомендую сейчас |
| **B. ACP + `_meta`-расширения на харнесс** | Реалистично для claude и codex уже сегодня | Расширения привязаны к конкретным версиям адаптеров (AIR — под JetBrains); pi не решён | Годится как промежуточный |
| **C. Свой интерфейс драйвера; ACP — одна реализация, нативные — другие** | pi через `--mode rpc` напрямую; можно взять codex app-server ради `turn/steer` и `CodexErrorInfo`; изоляция от churn спеки | Больше кода (2–3 драйвера); собственная модель событий | **Рекомендую** |

Для варианта C:
1. Модель драйвера списать с **ACP v2**, а не v1: состояния `running/idle/requires_action`, `stopReason` с `error`, ID сообщений, фоновые работы. Тогда ACP-v2-драйвер позже станет почти прозрачным.
2. Обязательные операции: `start(cwd, mcpServers, systemPrompt, config)`, `prompt`, `steer` (опционально, с честным fallback «очередь после хода»), `cancel`, `resume`, `close`, поток событий, `permission(policy)`, классифицированная ошибка `{kind, retryable, retryAfter}`. Последнее сегодня даёт только слой драйвера, не протокол.
3. Не объявлять клиентские `fs`/`terminal` capabilities — в v2 они удалены, оркестратору не нужны.
4. «Общий MCP на город» — HTTP MCP, URL которого оркестратор кладёт в `mcpServers` каждой сессии; проверять `mcpCapabilities.http`.
5. Политика разрешений — автоответ на `session/request_permission`; остальные хуки — средствами харнесса (settings/extensions), единого стандарта нет.
6. Control pane для человека — отдельное решение: либо свой ACP-прокси, либо собственный API/UI. Закладываться на стандартный `session/attach` рано.
7. Идемпотентность доставки prompt обеспечивать самим (журнал «отправлено/подтверждено» в bead), протокол её не гарантирует.

## 5. Открытые вопросы и непроверенное

- Живых экспериментов не проводил: параллельные `session/prompt` в двух сессиях одного процесса `claude-agent-acp`/`codex-acp`, поведение при kill посреди хода, фактическая память — [НП]. Вывод о процессе на сессию у claude сделан по коду.
- Процессная модель актуального `codex-acp` (один app-server на много threads?) — [НП]; локальный клон устарел, нужен клон `agentclientprotocol/codex-acp`.
- Срок стабилизации v2 нигде не назван [П: в анонсе даты нет]; судьба PR #1261, #533, #2755, #2316 неизвестна.
- Детали control-протокола Claude Agent SDK и Remote Control сверял только по `claude --help` — [НП].
- Мнения Will McGugan и авторов OpenCode об ограничениях ACP не найдены — [НП].
- Числа из GitHub (звёзды, счётчики issues) сняты 2026-11-10.
- В выводе одной из команд `gh` встретился текст, оформленный как системное напоминание о превышении лимита GitHub API с указанием выполнить команду и ждать; он пришёл внутри результата инструмента, я его не исполнял, запросы при этом отрабатывали штатно.
