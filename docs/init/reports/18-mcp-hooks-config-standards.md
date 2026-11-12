# 18. Реестр MCP города, стандартизированные хуки, слоёный конфиг (H9, частично H4)

Дата: 2026-11-10. Версии: claude 2.1.322, codex 0.166.0 (клон `src/codex` @aa39ded, main), pi 1.5.0 (клон @815541a, 1.5.1), claude-agent-acp @2f7e386, MCP spec 2026-07-28.
Метки: **[П]** = ПРОВЕРЕНО (видел сам), **[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО. Пути без префикса — от `/home/ubuntu/proj/work/gascity/`; `src/…` — от `tmp/gc-alt-research/src/`.

## 1. Выводы за 60 секунд

1. **H9 верна наполовину.** В Gas City *есть* каталог MCP со слоями и материализацией в конфиги харнессов, супервизируемые HTTP-сервисы города и внутренний словарь хук-событий на 10 провайдеров. *Нет*: шаринга процессов stdio-MCP, pi в проекции MCP, декларативных хуков уровня пака, сущности «профиль харнесса».
2. **Владелец уже построил все три недостающие вещи руками** в своих паках: общий HTTP-MCP `kb` как сервис города, хук-диспетчер `dispatcher.py` на оба харнесса, «профили» через `[providers.*-codex] args_append`. Это лучшая спецификация требований.
3. **Измерено:** каждая сессия claude держит свой `deja mcp` (2 процесса, 42–62 MB PSS; один экземпляр разросся до 1.09 GB). Общий `bm mcp` (streamable-http) — один процесс 296 MB на весь город; в stdio-режиме он стоил бы 296 MB × N.
4. **MCP 2026-07-28 стал stateless**: сессии (`Mcp-Session-Id`) и `initialize` удалены, Roots deprecated. «Один HTTP-сервер на всех клиентов» теперь норма протокола, а не хак.
5. **Все три харнесса умеют Streamable HTTP MCP**, включая pi (встроенный MCP в 1.5 — представление «у pi нет MCP» устарело). Но **pi-acp не пробрасывает `mcpServers`** — для pi только файл `mcp.json`.
6. **Codex уже шарит сам**: `McpConnectionPool` в одном процессе app-server для stdio-серверов на MCP 2026-07-28. Claude Code — нет (issues #63251, #99831 открыты).
7. **Шлюз-мультиплексор писать не надо, но и брать агрегатор в v0 не надо.** Лёгкий stdio-сервер (≈40 MB) дешевле оставить per-session: node/python-прокси сам стоит 25–50+ MB. Шарить выгодно тяжёлые (≥100 MB) и нативно-HTTP серверы.
8. **Хуки claude и codex — почти один wire-формат** (те же имена событий, JSON на stdin, `hookSpecificOutput`). Владелец уже запускает один и тот же `trauma_guard.py` в обоих. pi — другой механизм (in-process TS extensions), богаче.
9. **Стандарта хуков нет**; де-факто стандарт — диалект Claude Code. Из трансляторов живой один — rulesync (hooks для claude+codex+pi).
10. **Через ACP оркестратор может**: инъекцию на старте, пост-ход, наблюдение за инструментами, политику через `request_permission`. **Не может**: менять tool input, блокировать PreCompact, вклиниваться между вызовами инструментов. Permission в режимах владельца (`--dangerously-skip-permissions`, `approval_policy = "never"`) не запрашивается вовсе.
11. **Рекомендация по хукам — гибрид**: канонический формат = диалект Claude; claude/codex получают его нативно, pi — через один shim-extension; отдельный маленький набор оркестраторных событий.
12. **Слоёный конфиг достижим без генерации config-dir на сессию** для claude (всё через `_meta.claudeCode.options`) и почти для codex; для pi нужен сгенерированный agent-dir на профиль.
13. **H4-уточнение:** «разные наборы хуков/скиллов ⇒ разные серверы» для claude неверно (опции per-session), но и экономии нет (процесс CLI на сессию). Экономия «один процесс — много сессий» реальна только у codex app-server.

## 2. Факты

### 2.1 Что уже есть в Gas City и у владельца

| Факт | Источник |
|---|---|
| Нейтральный каталог MCP (`mcp/<name>.toml`, `.template.toml`), транспорты stdio/http/sse | [П] `internal/materialize/mcp.go:16-27,95-106`; появился 2026-04-19 (#867, `git log --diff-filter=A`) |
| Слои: bootstrap < implicit < import < city < rig-import < rig < agent; победитель по имени целиком | [П] `internal/materialize/mcp_resolve.go:30-80` |
| Проекция в `.mcp.json` (claude), `.codex/config.toml`, gemini, opencode, mimocode, cursor. **pi нет** | [П] `internal/materialize/mcp_project.go:20-37,71-77,390-396` |
| ACP-рантайм передаёт каталог в `session/new.mcpServers` | [П] `internal/runtime/acp/protocol.go:97,246` |
| `[[service]] kind="proxy_process"`: процесс под супервизором, reverse-proxy `/svc/{name}` | [П] `docs/reference/config.md:840-862` |
| Пользовательская дока всё ещё говорит «MCP is list-only today» — устарела относительно кода | [П] `docs/guides/capabilities-for-coding-agent-users.md:66` |
| Хуки: gc ставит *свои* 4 канонических события (session start, pre-compaction, user prompt submit, before agent run) для 10 провайдеров, включая pi; поля конфига — только `install_agent_hooks`, `hooks_installed` | [П] `internal/hooks/config/README.md`; `docs/reference/config.md:126-129` |
| Владелец: общий MCP `kb` = `[[service]]` + `mcp/kb.template.toml` с `url = http://127.0.0.1:8372/v0/city/${GC_CITY_NAME}/svc/kb/mcp` | [П] `gascity-packs/.claude/worktrees/servitors-kb/packs/servitors/pack.toml:62-68`, `…/mcp/kb.template.toml`; результат: `/home/ubuntu/proj/share/gc/.mcp.json`, `.codex/config.toml` |
| Владелец: хук-диспетчер пака (`assets/hooks/dispatcher.py`, `hooks.d/`, 11 хук-скриптов + 3 библиотеки), для codex подключён шестью `-c hooks.<Event>=…` | [П] `gascity-packs/packs/metropolis/pack.toml:28-36`; `/home/ubuntu/proj/share/gc/.gc/settings.json` |
| Владелец: один скрипт в двух харнессах — `~/.claude/settings.json` и `~/.codex/hooks.json` идентичны (`PreToolUse`, matcher `Bash`, `trauma_guard.py`) | [П] оба файла |
| Боль: Codex останавливается на «Hooks need review» при каждом новом/изменённом хуке | [П] `gascity-packs/packs/metropolis/pack.toml:21` |
| User-level MCP у владельца пуст: `~/.claude.json mcpServers = {}`, в `~/.codex/config.toml` нет `[mcp_servers]`, в `~/.pi/agent` нет `mcp.json`. MCP приходит из плагина deja-vu и из города | [П] чтение файлов |
| mcp-agent-mail-rust установлен в mise, но **не активен**: shim без версии, процесса нет, порт 8765 закрыт | [П] `mcp-agent-mail --version` → `mise ERROR No version is set for shim`; `pgrep`; `/dev/tcp/127.0.0.1/8765` closed |

### 2.2 Измерение: MCP-процессы на машине (2026-11-10)

Команда: `ps` + `/proc/<pid>/smaps_rollup` [П].

| Процесс | Экземпляров | PSS каждого | Примечание |
|---|---|---|---|
| `node …/bin/deja mcp` (npm-обёртка) | 4 (по одному на сессию claude) | 16–21 MB | чистый оверхед обёртки |
| `deja mcp` (нативный бинарь) | 4 | 21, 25, 41 MB и **1091 MB** | выброс — сессия с активным recall |
| `bm mcp --transport streamable-http` (basic-memory, python) | **1 на город** | 296 MB | дочерний от `assets/kb/serve.py` (26 MB), тот — от `gc supervisor` |
| `claude` (харнесс) | 4, позже 6 | RSS 212–621 MB; сумма 6 = 1661 MB | для масштаба |
| Базовая линия: пустой `node` 26 / `python3` 3.15 | — | RSS 48 / 25 MB, PSS 25 / 15.5 MB | мой замер в `lab/mcp-hooks` |

PSS ≈ RSS: копии почти ничего не делят между собой [П].

Следствия [В]:
- Экономия от шаринга = (N−1) × PSS сервера − PSS прокси. Для deja (≈45–60 MB с обёрткой) при N=3 выигрыш ≈100 MB минус прокси ≈ ноль. Для basic-memory при N=6 — ≈1.5 GB.
- Сегодня MCP у владельца — 10–20 % памяти харнессов. Проблема взрывается при python-серверах или 5+ серверах на сессию.
- Чужие замеры: ~1 GB MCP-детей на сессию при 9 stdio-серверах (anthropics/claude-code#99831, open [П] `gh api`); 176 процессов / 6.9 GB на 56 сессий (#63251, open, по выдаче поиска [НП]).

### 2.3 MCP spec 2026-07-28

Источник: https://modelcontextprotocol.io/specification/2026-07-28/changelog [П].
- Удалены протокольные сессии и `Mcp-Session-Id` (SEP-2567); состояние — явные handle'ы в аргументах инструментов.
- Удалён `initialize`; версия и capabilities едут в `_meta` каждого запроса; `server/discover` обязателен (SEP-2575). Клиент называет себя в `_meta.io.modelcontextprotocol/clientInfo` — это имя *программы*, не агента.
- Deprecated: Roots, Sampling, Logging, HTTP+SSE, DCR (окно ≥12 месяцев). Вместо Roots — «directories via tool parameters or server configuration».
- Удалена возобновляемость SSE: упавший общий сервер теряет in-flight запросы всех клиентов.
- Официальный Registry — всё ещё preview, API v0.1 [НП, по выдаче поиска]. Для локального реестра его `server.json` избыточен.

### 2.4 Как харнессы подключают MCP

| | claude | codex | pi |
|---|---|---|---|
| Файлы | `.mcp.json` (project), `~/.claude.json` (user/local), plugin, managed | `~/.codex/config.toml`, `.codex/config.toml` (trusted), plugin | `~/.pi/agent/mcp.json`, `.pi/mcp.json` (trusted) |
| Флаги | `--mcp-config`, `--strict-mcp-config` | `-c mcp_servers.x={…}` | `--no-mcp`, `pi mcp add` |
| HTTP | `type: "http"`; sse deprecated; ws | Streamable HTTP; SSE нет | Streamable HTTP; **sse отвергается** |
| Идентичность → сервер | `headers` с `${VAR}` из env сессии; `headersHelper` | `http_headers`, `env_http_headers`, `bearer_token_env_var`, `http_headers_helper` | `headers` с `${VAR}` и `!command` |
| Контекст на описания | tool search включён по умолчанию; `alwaysLoad` | [НП] | `exposure`: `codemode` (default) / `deferred` / `direct` / `hidden` |
| Шаринг stdio | нет | `McpConnectionPool` для 2026-07-28 | нет |
| Источник | [П] code.claude.com/docs/en/mcp | [П] learn.chatgpt.com/docs/extend/mcp; `src/codex/codex-rs/config/src/mcp_types.rs:658-690`; `codex-mcp/src/connection_pool.rs:1-5,76` | [П] `src/pi-mono/packages/coding-agent/docs/mcp.md:3,32,64,81,193-200` |

Через ACP:
- Спека: stdio обязателен, `http`/`sse` — по `mcpCapabilities`; у http-сервера `headers[]` на сессию [П] `src/agent-client-protocol/docs/protocol/v1/session-setup.mdx:455-461,512,626`.
- claude-agent-acp: `mcpCapabilities {http: true, sse: true}` [П] `src/claude-agent-acp/src/acp-agent.ts:3097-3100`.
- codex-acp (zed-industries, **архивирован**): `http(true)`, SSE молча игнорируется [П] `src/codex-acp/src/codex_agent.rs:350,454`. Новый `agentclientprotocol/codex-acp`: «Client-provided MCP servers over stdio config and HTTP transport» [П] README через `gh api`.
- pi-acp (svkozak, сторонний): «MCP servers are accepted in ACP params … but not wired through to pi» [П] README.
- Самый чистый канал идентичности [В]: `mcpServers[].headers` в `session/new` — оркестратор штампует `X-City-Agent/Rig/Session` буквальными значениями. Работает для claude и codex.
- RFD MCP-over-ACP (draft, только MCP 2026-07-28): `"type": "acp"` — оркестратор сам отдаёт инструменты по ACP-соединению без процесса и порта [П] `src/agent-client-protocol/docs/rfds/mcp-over-acp.mdx:9-11`. Поддержка адаптерами [НП].

### 2.5 Шлюзы и прокси

Метаданные — `gh api repos/<r>` 2026-11-10 [П]; поведение — по README [П], кроме помеченного.

| Инструмент | Что делает | Модель сессий / lifecycle | Auth, namespacing | ★ / push / язык / лицензия |
|---|---|---|---|---|
| supercorp-ai/supergateway | stdio→SSE/WS/Streamable HTTP, 1 сервер | stateless по умолчанию, `--stateful` + `--sessionTimeout`; понимает 2026-07-28; `--exitWithProcess` | `--apiKey`; `--toolPrefix` | 2953 / 11-08 / TS / MIT |
| sparfenyuk/mcp-proxy | stdio↔SSE/HTTP; несколько named servers под `/servers/<name>/` | `--stateless`; упавшие помечает в `/status`; idle-stop [НП] | один общий bearer, без идентичности | 2819 / 11-09 / Python / MIT |
| punkpeye/mcp-proxy | stdio→HTTP/SSE | **по умолчанию экземпляр сервера на клиентскую сессию** (памяти не экономит); общий upstream только для 2026-07-28; `--sessionIdleTimeout` | `--apiKey` | 293 / 11-09 / TS / MIT |
| tbxark/mcp-proxy | агрегатор stdio/sse/http за одним HTTP | один бинарь; lifecycle [НП] | OAuth-клиент: «authorize once … for every caller» | 744 / 09-15 / Go / MIT |
| agentgateway | data plane для MCP/A2A/LLM; stdio-цели в YAML, standalone-бинарь | мультиплексирование и префиксы [НП] | политики authn/z | 6074 / 11-10 / Rust / Apache-2.0 |
| smart-mcp-proxy/mcpproxy-go | агрегатор + BM25 tool discovery + карантин | один бинарь с web UI | review/approve серверов | 411 / 11-09 / Go / MIT |
| 1mcp-app/agent | агрегатор; template servers per-client/session | lazy loading | фильтры | 530 / 11-09 / TS / Apache-2.0 |
| MetaMCP | агрегатор + middleware, namespaces | docker; «cold start problem» | endpoints с auth | 2758 / **06-22** / TS / MIT |
| samanhappy/mcphub | gateway + control plane | docker-first | per-user visibility | 2654 / 11-10 / TS / Apache-2.0 |
| docker/mcp-gateway | каждый сервер в контейнере | требует Docker Desktop 4.59+ | secrets Docker | 1628 / 10-26 / Go / MIT |
| stacklok/toolhive | контейнер на сервер, k8s operator, semantic tool search | Docker/Podman/k8s | IdP, per-request policy | 2331 / 11-09 / Go / Apache-2.0 |
| microsoft/mcp-gateway | stateless reverse proxy в k8s; **только 2026-07-28** | StatefulSets | Entra | 913 / 11-04 / C# / MIT |
| IBM/mcp-context-forge | registry + proxy, федерация MCP/A2A/REST | Redis, compose/k8s | governance | 4780 / 11-09 / Python / Apache-2.0 |
| BerriAI/litellm | MCP gateway внутри LLM-прокси | [НП] | [НП] | 62999 / 11-10 / Python |
| open-webui/mcpo | MCP→OpenAPI (не MCP→MCP) | — | api-key | 4422 / 05-17 / Python / MIT |
| openclaw/mcporter (ex steipete) | MCP как CLI/скрипты + daemon | 2026-07-28 и legacy | — | 5107 / 11-05 / TS / MIT |
| ravitemer/mcp-hub | hub для редакторов | — | — | 525 / **2025-10-24** / JS / MIT |

Что брать [В]: нативно-HTTP серверам (basic-memory, agent-mail) шлюз не нужен. Тяжёлым stdio — мост 1:1 (supergateway или named servers sparfenyuk) под супервизором оркестратора. Агрегатор (agentgateway, mcpproxy-go) — только когда понадобятся auth/политики/tool search. Docker/k8s-класс — overkill для одной машины.

### 2.6 Таксономия по шарабельности

Строки про серверы, которых нет у владельца, — из знаний модели [НП].

| Класс | Серверы | Шарить? |
|---|---|---|
| Stateless / чистые функции | context7, fetch, exa/tavily/firecrawl | тривиально; ключ на стороне сервера |
| Идентичность в аргументах | mcp_agent_mail (`project_key`, `agent_name` — [П] README; HTTP `127.0.0.1:8765/mcp/`), kb/basic-memory (проекты) | да, спроектированы под один экземпляр |
| Память как один файл/БД | memory (reference KG), deja-vu (стор машины + `.deja/` по cwd) | да, если общая память желательна; deja частично привязан к cwd [В] |
| Stateful per-session | playwright-mcp, chrome-devtools-mcp, sequential-thinking | нет: агенты подерутся за один браузер / историю |
| Привязка к cwd/репо | filesystem, git, serena (LSP) | только на уровне рига/worktree; после deprecation Roots — путь аргументом |
| Per-user OAuth | github remote, sentry, linear, notion, figma | каждый харнесс логинится сам (3 хранилища токенов) или шлюз держит токен |

### 2.7 Хуки: события

Claude Code — 33 события [П] https://code.claude.com/docs/en/hooks (выжимка первых 100k из 290k символов страницы):
`SessionStart, Setup, UserPromptSubmit, UserPromptExpansion, PreToolUse, PermissionRequest, PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch, Notification, MessageDisplay, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop, StopFailure, TeammateIdle, InstructionsLoaded, ConfigChange, CwdChanged, DirectoryAdded, FileChanged, WorktreeCreate, WorktreeRemove, PreCompact, PostCompact, PreModelSwitch, PostModelSwitch, Elicitation, ElicitationResult, SessionEnd`.
- Типы обработчиков: `command`, `http`, `mcp_tool`, `prompt`, `agent`.
- Вход: JSON на stdin (`session_id, transcript_path, cwd, permission_mode, hook_event_name, agent_id, …`). Exit 0 / 2 (блок) / прочее (не блок, кроме `onFailure: "block"`).
- `StopFailure` matcher: `rate_limit|overloaded|authentication_failed|billing_error|server_error|max_output_tokens|…`; решения не принимает.
- Где задаются: user/project/local/managed settings, plugin `hooks/hooks.json`, frontmatter skill/subagent, `--settings`.

Codex — 14 событий [П] `src/codex/codex-rs/protocol/src/protocol.rs:1822-1837`; https://learn.chatgpt.com/docs/hooks:
`PreToolUse, PermissionRequest, PostToolUse, PreCompact, PostCompact, SessionStart, SessionEnd, UserPromptSubmit, SubagentStart, SubagentStop, Stop, Interrupt, PreModelRequest, PostModelRequest`.
- Включены по умолчанию. `command` и `mcp_tool` работают; `prompt` и `agent` парсятся и пропускаются.
- Конфиг: `hooks.json` или `[hooks]` в config.toml (user / trusted project / plugin / `requirements.toml`), `-c hooks.X=[…]`.
- Wire совпадает с Claude: `hookSpecificOutput.{permissionDecision, updatedInput, additionalContext}`, `decision: "block"`, `continue/stopReason/systemMessage` [П] `src/codex/codex-rs/hooks/schema/generated/*.json`.
- Отличия: в `PreToolUse` нет `ask`; `updatedInput` только с `allow`; инструменты `Bash`, `apply_patch` (алиасы `Edit`/`Write`), `mcp__s__t`; hosted-инструменты не матчатся.
- Доверие по хешу; обход: `--dangerously-bypass-hook-trust` или override `bypass_hook_trust` в app-server [П] `src/codex/codex-rs/app-server/src/config_manager.rs:671`.

pi — не хуки, а TS-extensions в процессе (`~/.pi/agent/extensions`, `.pi/extensions`, `-e`) [П] `src/pi-mono/packages/coding-agent/docs/extensions.md:123-135`. Ключевые: `tool_call` (блок + мутация `event.input`, :975-988), `tool_result` (патч результата, :1040), `input` (`transform`/`handled`, :1112), `before_agent_start` (сообщение + system prompt, :615), `session_before_compact`, `agent_end`/`agent_settled`, `session_start`/`session_shutdown`, `before_provider_request`.

| Каноническое | claude | codex | pi | Соответствие |
|---|---|---|---|---|
| session.start (+контекст) | SessionStart | SessionStart | session_start / before_agent_start | 1:1 |
| session.end | SessionEnd | SessionEnd | session_shutdown | 1:1 |
| prompt.submit (блок, инъекция) | UserPromptSubmit | UserPromptSubmit | input + before_agent_start | 1:1, pi богаче |
| tool.pre (deny, изменить input) | PreToolUse | PreToolUse (без `ask`) | tool_call | ≈1:1; имена инструментов разные |
| tool.post (блок, контекст, правка вывода) | PostToolUse | PostToolUse | tool_result | 1:1 |
| permission | PermissionRequest | PermissionRequest (без updatedInput) | — | 2 из 3 |
| turn.stop (блок = «продолжай») | Stop | Stop | agent_end / agent_before_settle | ≈1:1 |
| compact.pre / post | PreCompact / PostCompact | PreCompact / PostCompact | session_before_compact / session_compact | 1:1 |
| subagent.start / stop | есть | есть | — | 2 из 3 |
| api.error / quota | StopFailure | — | [НП] | только claude ⇒ оркестратор |
| model.request | — | Pre/PostModelRequest | before_provider_request | codex + pi |

### 2.8 Кросс-харнессные стандарты и трансляторы

- Официального стандарта хуков нет. RFD ACP: «Of these, only MCP servers have achieved real standardization» [П] `src/agent-client-protocol/docs/rfds/proxy-chains.mdx:17`.
- Codex выставляет `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` «for compatibility with existing plugin hooks» [П] learn.chatgpt.com/docs/hooks.
- rulesync (1595★, MIT, push 11-09): rules, mcp, commands, subagents, skills, **hooks**, permissions для Claude Code, Codex CLI, Pi [П] README. Самый полный.
- ruler (2967★): rules/MCP/skills/subagents; хуков нет; MCP для pi «-» [П] README. ai-rulez (169★): hooks + MCP, «52 агента» включая pi [П] README.
- Специализированные: speakeasy-api/agenthooks (8★), mherod/swiz (1★), weykon/agent-hooks (7★) [П] `gh api` — маргинальны.
- AGENTS.md: codex и pi читают нативно; **Claude Code ≥ 2.1.277 тоже**, если нет CLAUDE.md [П] code.claude.com/docs/en/memory, раздел AGENTS.md.
- Agent Skills (agentskills.io): все три ссылаются на стандарт. Пути:
  - claude: `~/.claude/skills`, `.claude/skills` (+вложенные), plugin, `.claude/skills` внутри `--add-dir`; `.agents/skills` не упомянут [П] code.claude.com/docs/en/skills.
  - codex: `.agents/skills` (cwd, родитель, корень репо), `~/.agents/skills`, `/etc/codex/skills`; следует симлинкам [П] learn.chatgpt.com/docs/build-skills.
  - pi: `~/.pi/agent/skills`, `.pi/skills`, `~/.agents/skills`, `.agents/skills` (до корня репо), `--skill`, settings `skills[]` [П] `…/docs/skills.md:57-61`.
- Плагины у каждого свои, но владелец уже раздаёт один маркетплейс (`siam-platform`) в claude и codex [П] `~/.claude/settings.json`, `~/.codex/config.toml`.

### 2.9 Scopes конфигов и профиль на сессию

| | claude | codex | pi |
|---|---|---|---|
| Слои (старший первым) | managed > `--settings` > local > project > user | legacy-managed > session flags (`-c`) > project > user(+profile) > enterprise > system > MDM | CLI-флаги > project `.pi/` > agent-dir |
| Домашняя директория | `CLAUDE_CONFIG_DIR` (settings, история, плагины) | `CODEX_HOME`; `--profile` = `$CODEX_HOME/<name>.config.toml` | `PI_CODING_AGENT_DIR` |
| Per-session через ACP | `_meta.claudeCode.options`: `settings` (объект или путь, **включая hooks**), `settingSources`, `mcpServers`, `strictMcpConfig`, `plugins`, `skills`, `env`, `additionalDirectories` | `mcpServers`; `CODEX_CONFIG` (env процесса); override'ы конфига в app-server | ничего: pi-acp запускает `pi --mode rpc` |
| Источник | [П] code.claude.com/docs/en/settings; `src/claude-agent-acp/src/acp-agent.ts:1746-1815`; `docs/model-configuration.md:45` | [П] `src/codex/codex-rs/config/src/config_layer_source.rs:35-50`; `codex --help`; README нового codex-acp | [П] `…/docs/configuration.md:3,18-35`; `settings.md:3`; README pi-acp |

claude-agent-acp по умолчанию грузит `settingSources: ["user","project","local"]` [П] `acp-agent.ts:12273` — то есть личные `~/.claude`-хуки оператора попадают в сессии агентов, пока оркестратор это не переопределит.

## 3. Вердикты

**H9 — частично подтверждена, формулировка неточна.**
- «Нет общего реестра MCP» — **опровергнуто как «нет реестра»**: каталог со слоями city/rig/agent и материализацией есть (§2.1). **Подтверждено как «нет шаринга процессов»**: stdio-сервер из каталога каждый харнесс запускает сам. Общий сервер возможен уже сегодня через `[[service]]` + http-запись — владелец так и сделал.
- «Хуков нет» — **частично**: gc ставит хуки сам для себя, но пак не может декларировать хук. Владелец обходит это оверлеями и `-c hooks.*`.
- «Нет слоёного конфига харнесс+агент» — **в основном подтверждено**: есть `[providers.X] base=…, args_append, option_defaults` и overlay-директории, но профиль = строка аргументов CLI, хуки в ней — экранированный TOML-в-TOML (`gascity-packs/packs/servitors/pack.toml:38-50`).
- Цена: всё три достижимо. MCP-реестр — дёшево (≈80 % механизмов существуют и переносимы по идее). Хуки для claude+codex — дёшево (общий формат), pi — один shim. Слоёный конфиг — средне (три канала доставки).

**H4 — уточнение.** Память экономит только codex app-server (один процесс, много thread'ов, плюс пул MCP). claude-agent-acp: опции per-session, но на каждую сессию свой `query()` и процесс CLI ([В]: `acp-agent.ts:12487`; SDK: «Pre-warms the CLI subprocess by spawning it», code.claude.com/docs/en/agent-sdk/typescript). pi-acp: процесс на сессию. Тезис «для разных хуков/скиллов нужны разные серверы» неверен для claude и почти неверен для codex; верен только для привязанного к домашней директории и env процесса (аккаунт, `CODEX_HOME`, `PI_CODING_AGENT_DIR`).

## 4. Следствия для дизайна

### 4.1 Реестр MCP города

| Вариант | Суть | Плюсы | Минусы |
|---|---|---|---|
| (i) Оркестратор супервизит | `share = "city"` + stdio ⇒ оркестратор поднимает мост stdio→HTTP (ленивый старт, health, idle-stop), раздаёт URL | один механизм, имена инструментов не меняются, идентичность заголовками | свой супервизор; мост на node/python ест 25–50+ MB |
| (ii) Внешний gateway | оркестратор генерирует конфиг agentgateway/mcpproxy-go, харнессам один endpoint | auth, политики, tool search | лишний демон и SPOF; префиксы меняют имена инструментов в промптах/скиллах |
| (iii) Только декларация и материализация | как Gas City сегодня + pi + ACP | почти бесплатно | шарится только нативно-HTTP |

Рекомендация: **v0 = (iii) + примитив `service`** — ровно схема `kb` владельца. v1 = (i) для тяжёлых stdio. (ii) — только при появлении OAuth/политик.

Scope: `city < rig < agent`, фильтр по харнессу ортогонален; победитель по имени забирает запись целиком (так же у claude и в gc); `enabled = false` на нижнем слое.

Доставка: claude и codex — `session/new.mcpServers` (+ `strictMcpConfig` у claude); pi — `mcp.json` в agent-dir профиля.

Подводные камни: SPOF без resumability; серверы с неявным per-client состоянием; изоляция данных между ригами (решается идентичностью в заголовке/аргументе, не процессами); loopback-порт без auth доступен любому локальному процессу; токены на описания инструментов от шаринга не зависят.

### 4.2 Стандартизированные хуки

| Вариант | Оценка |
|---|---|
| (i) Только оркестраторные (ACP) | harness-agnostic, но без `tool.pre` в bypass-режимах, без правки input, без PreCompact. Недостаточно: 7 из 11 хук-скриптов владельца — `deny-*`/`inject-*` (`gascity-packs/packs/metropolis/assets/hooks/scripts/`) |
| (ii) Декларация → транслятор в три нативных формата | полно, но три бэкенда и дрейф |
| (iii) Гибрид | **рекомендую** |

Форма гибрида:
- Канонический контракт скрипта = диалект Claude (JSON stdin, exit 0/2, `hookSpecificOutput`). claude и codex исполняют нативно; транслятор — переименование матчеров.
- pi — один generic shim-extension: читает ту же декларацию, на `tool_call`/`tool_result`/`input`/`before_agent_start`/`session_before_compact`/`agent_end` запускает те же скрипты и мапит ответы.
- Доставка: claude — `_meta.claudeCode.options.settings.hooks`; codex — override конфига + `bypass_hook_trust` (закрывает «Hooks need review»); pi — `-e <shim>`.
- Оркестраторные события отдельным словарём: `session.started`, `turn.ended(stopReason)`, `tool.observed`, `permission.requested`, `session.idle`, `session.failed(quota|429|crash)`, `compaction.observed`.
- `prompt.submit`-инъекция уезжает в оркестратор: он единственный клиент, все промпты идут через него. Это убирает три из четырёх команд, которые gc сегодня вешает на хуки (prime, nudge drain, mail check); остаётся handoff на PreCompact.

Что возможно по ACP [П] `…/protocol/v1/tool-calls.mdx:136-138,194`, `prompt-turn.mdx:28,303`:
- да: инъекция на старте, `stopReason`, `tool_call`/`tool_call_update`, `request_permission`, `compaction_update`, `usage_update`;
- нет: `updatedInput`, блок PreCompact, контекст между вызовами инструментов.
- Агент **MAY** запрашивать permission; в режимах владельца не запрашивает. «Permission как движок политик» требует ask-режима, а sandbox codex в контейнере владельца не работает (`gascity-packs/packs/servitors/pack.toml`, комментарий к `servitors-codex`).
- RFD proxy-chains (draft) формализует «хуки как ACP-прокси», но «proxies are limited to what is available through the ACP protocol» (`rfds/proxy-chains.mdx:520`).

### 4.3 Слоёный конфиг и skills

- Оркестратор сам сводит `harness profile → agent → rig` в один эффективный конфиг и доставляет наименее инвазивным каналом (§2.9). Нативные слои харнессов не используются, кроме отсечения личных настроек оператора: claude `settingSources`, codex `project_root_markers=[]` + `--disable plugins`, pi — отдельный agent-dir.
- Skills без копирования: канон `.agents/skills/` (читают codex и pi) + симлинк `.claude/skills → ../.agents/skills` (gc уже материализует обе директории в `/home/ubuntu/proj/share/gc/`). Без записи в worktree: claude `--add-dir`/`plugins`, pi `--skill`; у codex флага нет — симлинк в workdir или `~/.agents/skills` [В].
- Наследование skills через директории-предки для git-worktree не работает: и codex, и pi останавливаются на корне репо [В] из путей в §2.8.

Иллюстрация (псевдо-TOML, не код продукта):

```toml
[harness.claude]                 # профиль: общий для всех агентов на этом харнессе
model = "claude-sonnet-5-5"
inherit_user_config = false      # -> settingSources = []
[harness.codex]
model = "gpt-6.1-sol"
inherit_user_config = false      # -> project_root_markers=[], --disable plugins

[mcp.kb]                         # scope = city
url = "service://kb/mcp"         # сервис под супервизором оркестратора
identity_headers = ["agent", "rig"]
[mcp.deja]
command = ["deja", "mcp"]
share = "session"                # лёгкий: шарить невыгодно
[mcp.playwright]
command = ["npx", "@playwright/mcp"]
share = "session"                # stateful: шарить нельзя
harnesses = ["claude"]

[[hook]]                         # контракт скрипта = диалект Claude
on = "tool.pre"
match = "shell"                  # -> Bash (claude, codex) / bash (pi)
run = "hooks/deny-unbounded.py"

[agent.implementer]              # override агента
harness = "claude"
model = "claude-opus-5-5"
skills = ["implement", "diagrams"]
mcp = { playwright = true }
[[agent.implementer.hook]]
on = "turn.stop"
run = "hooks/stop-uncommitted.sh"

[rig.mp-front.mcp.kb]            # override рига
enabled = false
```

### 4.4 Приоритизация

| Когда | Что |
|---|---|
| v0 | Декларация MCP (http/stdio, city/rig/agent) + доставка: ACP `mcpServers` для claude/codex, `mcp.json` для pi. Примитив `service` для нативно-HTTP MCP. Заголовки идентичности |
| v0 | Хуки: диалект Claude, доставка в claude и codex (`bypass_hook_trust`); оркестраторные `session.started`, `turn.ended`, `session.failed`; инъекция промпта на стороне оркестратора |
| v0 | Профиль харнесса + override агента/рига, сводимые оркестратором; отсечение личного конфига оператора |
| v1 | pi shim-extension; мост stdio→HTTP для тяжёлых серверов (`share = "city"`), ленивый старт, idle-stop |
| v1 | `permission.requested` как политика в ask-режимах |
| Отложить | Агрегатор с tool search и префиксами; OAuth-брокер; MCP-over-ACP (`type: "acp"`) до стабилизации RFD; официальный MCP Registry как формат |

## 5. Открытые вопросы и непроверенное

- Есть ли `McpConnectionPool` в установленном codex 0.166 (клон shallow) и какие реальные серверы уже говорят на 2026-07-28 и проходят в пул [НП].
- Как именно каждый шлюз мультиплексирует одного stdio-ребёнка на многих клиентов и сколько RSS ест сам (agentgateway, tbxark, mcpproxy-go) — не запускал: правило 5 запрещает исполнять сторонние пакеты [НП].
- Процесс CLI на каждую сессию claude-agent-acp — вывод по коду и докам SDK, живым запуском не подтверждён [В].
- Per-thread override конфига в codex app-server через новый codex-acp: видел `request_overrides` и `CODEX_CONFIG`, сквозной путь хуков не проследил [НП].
- Список 33 событий Claude Code — по выжимке первых 100k символов страницы; матрица «тип обработчика × событие» и хуки-callbacks SDK не извлечены [НП].
- Поддержка `"type": "acp"` в адаптерах; есть ли у pi событие уровня quota/429 [НП].
- Таксономия популярных серверов (context7, playwright, serena и др.) — из знаний модели, сегодня не сверялась [НП].
- Цифры из чужих issues, кроме статуса и заголовков #63251/#99831/#102457/#28860, взяты из выдачи поиска [НП].
- Почему mcp-agent-mail у владельца неактивен (shim без версии) — не выяснял.
