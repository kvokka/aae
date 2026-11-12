# 19 — Control pane для человека без tmux (H10, частично H1, H4)

Дата: 2026-11-10. Источники: ACP spec @ ab82539 (2026-11-09), Gas City @ f1f75f53c, codex @ aa39ded (CLI 0.166.0),
claude-agent-acp 0.102.0, pi-mono @ 815541a, город владельца `/home/ubuntu/proj/share/gc`, пак `metropolis`.
Метки: **[П]** = ПРОВЕРЕНО (видел в коде/выводе/документе), **[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО.

## 1. Выводы за 60 секунд

- **«Просто подключиться по ACP к живой сессии» из коробки нельзя.** ACP v1 — это 1 клиент ↔ 1 агент по stdio. Multi-client
  attach — открытый PR #533 (с февраля 2026, не слит); удалённый транспорт (HTTP/WebSocket) — RFD в статусе Active, в спеке
  только stdio. H10 в буквальной формулировке — **частично**.
- **Но H10 реализуема одним приёмом: оркестратор = ACP-хаб.** Демон владеет агентами «вниз» и сам выставляет ACP-агента
  «вверх» (`<cli> acp` — stdio-шим к демону). Draft v2 это прямо разрешает: «Whether a session can be active on more than one
  connection at once is up to the Agent». Рабочее доказательство — `hydra-acp` (демон + шим + TUI + web + Slack).
- **Эксперимент:** два процесса `claude-agent-acp` на одной сессии — блокировки нет, зеркалирования нет, контексты
  расходятся, транскрипт превращается в дерево с двумя ветками. Конкурентный доступ «из двух процессов» запрещать должен
  оркестратор (single-writer), харнесс этого не сделает.
- **Нативный мульти-клиент есть только у codex** (app-server daemon, `codex --remote`, `codex agents`, подписка нескольких
  соединений на один thread) — гарантированно только если оркестратор сам говорит с общим app-server-демоном; `codex-acp`
  поднимает собственный app-server, и виден ли он родному TUI — не проверено.
  У claude Remote Control/agent view покрывают интерактивные и `--bg` сессии и требуют подписку claude.ai; для headless/SDK/ACP
  документация молчит. У pi — ничего (RPC 1:1).
- **Что tmux даёт владельцу сегодня**: `gc session attach desk` (главная сессия), attach к любой сессии «как есть», ответ на
  диалоги, которые gc не умеет закрывать, ssh с телефона, переживание рестарта supervisor. **Чем плох**: это сырой TUI без
  обзора города; nudge через send-keys может «ответить» на диалог первой опцией (gascity#8150).
- **Gas City уже наполовину построил не-tmux пульт**: API (`transcript`, `stream` SSE, `pending`, `respond`, `messages`) и
  dashboard («needs you», live peek, respond). Но в SPA нет поля «написать сессии», а `attach` к ACP-сессии — жёсткая ошибка.
- **Главная сессия** в новой системе = обычная долгоживущая сессия профиля `desk`, которой владеет демон; человек входит в неё
  любым ACP-клиентом. «Тот же конфиг» обеспечивает хаб (system prompt, MCP, hooks, cwd навешиваются на стороне демона), а не
  клиент.
- **Готовые клиенты есть**: произвольную команду-агента принимают почти все (Zed, JetBrains, Neovim-плагины, agent-shell,
  Toad, acpx); `session/list` + открытие существующей сессии — Zed, CodeCompanion, agentic.nvim, avante, agent-shell, acpx.
  Удалённого транспорта и наблюдения за чужой живой сессией нет ни у одного редактора.
- **Уведомления ACP не решает**: отсутствующему клиенту пушить некому. Нужен out-of-band канал от журнала событий
  оркестратора (mail как сейчас + ntfy/Telegram/desktop).
- **Рекомендация для v0**: (c-min) `<cli> sessions | tail | send | respond` поверх журнала событий + (a) `<cli> acp`-шим
  для готовых клиентов + (e) `<cli> takeover/release` как аварийный выход в родной TUI через resume. Свой web UI,
  WebSocket-транспорт, мульти-контроллер — отложить.
- **Теряем** против `tmux attach`: родной TUI «как есть» (slash-команды, `/login`, статус-строка), универсальность (любой CLI
  без адаптера), выживание агентов при падении демона. **Приобретаем**: явный сигнал «ждёт человека», очередь ввода без гонок
  с диалогами, один пульт на весь город, работу из IDE/браузера/телефона, replay истории.

## 2. Факты

### 2.1. Что tmux даёт человеку в Gas City сейчас

| Возможность | Как сделано | Доказательство |
|---|---|---|
| Главная сессия оператора | named session `desk` (claude opus), вход `gc session attach desk`; `mayor` — отдельная, «for diagnosis only» | **[П]** `share/gc/pack.toml:31-42`; `gascity-packs/packs/metropolis/README.md:1632, 2057-2064` |
| Attach к любой сессии | `exec tmux attach` на сокете города | **[П]** `internal/runtime/tmux/adapter.go:1464-1490` |
| Снимок вывода | `capture-pane` | **[П]** `internal/runtime/tmux/adapter.go:1197-1202` |
| Ввод агенту | send-keys с ожиданием idle; fallback «шлём всё равно» | **[П]** `internal/runtime/tmux/adapter.go:868-885` |
| Ответ на диалоги, которых gc не знает | «`gc session attach <id>` and answer it» (model switch, external CLAUDE.md imports) | **[П]** `metropolis/README.md:2050-2051` |
| Переживание рестарта supervisor | «the `tmux -L gc` server outlives a supervisor restart, which adopts its sessions» | **[П]** `share/gc/city.toml:101-104` |
| Уведомления оператору | mail в ящик `human`, читается в desk; «nothing interrupts the desk», счётчик непрочитанного в строке перед промптом | **[П]** `share/gc/city.toml:63-65`; `metropolis/README.md:2059-2063` |

Неудобства (то, на что жалуется владелец):

- Desk — это просто Claude Code TUI в отдельном tmux; обзора города в нём нет, всё добывается командами агента
  (`gc metropolis status`, `gc mail inbox human`). **[П]** `metropolis/README.md:1653-1660`
- send-keys — не протокол: «on tmux a gc nudge would answer it by picking the first option, gastownhall/gascity#8150» —
  поэтому unattended-ролям запрещён `AskUserQuestion`. **[П]** `metropolis/README.md:1797`
- Для не-tmux провайдеров attach отсутствует: ACP — `"acp provider does not support attach"`
  (`internal/runtime/acp/acp.go:764-766`), subprocess — то же (`subprocess.go:366-368`), t3bridge возвращает ошибку-подсказку
  «visible in T3 Code UI at http://localhost:5173» (`t3bridge/provider.go:3115-3125`). **[П]** То есть переход на ACP в
  самом Gas City сегодня отнимает у человека attach и ничего не даёт взамен в терминале.

### 2.2. Что уже есть в Gas City без tmux

- HTTP API: `GET /v0/sessions`, `GET /v0/session/{id}/transcript | pending | stream`, `POST /v0/session/{id}/messages |
  respond | stop | kill | suspend | wake`. **[П]** `internal/api/server.go:368-384`
- ACP-провайдер не авто-одобряет разрешения: `session/request_permission` удерживается как pending interaction и ждёт
  `Respond` (`approve`→`allow_once`, `approve_always`→`allow_always`). **[П]** `internal/runtime/acp/permission.go:18-44`,
  `conn.go:52-62`
- Peek для ACP — кольцевой буфер строк из `session/update`. **[П]** `internal/runtime/acp/acp.go:937-945`, `conn.go:335-340`
- Dashboard (SPA встроен в бинарь): страницы `/agents`, `/agents/:slug`, `/beads`, `/runs`, `/mail`; набор «needs you» с
  причинами `awaiting-input | errored | rate-limited | stalled`; live peek по SSE; `respondSession`. **[П]**
  `dashboardspa/web/frontend/src/App.tsx:157-162`, `attention/agentNeedsYou.ts:11-16`, `supervisor/client.ts:159`
- В интерфейсе `SupervisorApi` нет метода отправки сообщения в сессию (есть `sendMail`, `sling`, `respondSession`); «Chat
  thread» на странице агента — это mail-переписка, не диалог с сессией. **[П]** `supervisor/client.ts:95-180`,
  `components/agent/AgentChatThread.tsx:1-35`. Терминального вида (xterm) нет. **[П]** (grep по `src/`)
- Мессенджеры: `internal/extmsg` — фабрика привязок + HTTP-адаптер; Telegram/iMessage — только PoC в
  `contrib/openclaw-bridge` («Proof of concept»). **[П]** `contrib/openclaw-bridge/README.md:1-8`
- `contrib/mayor-chat` — Ink-TUI поверх Claude Agent SDK streaming input: «это contrib reference code», не часть `gc`.
  **[П]** `contrib/mayor-chat/README.md:1-9`. Попытка сделать «главную сессию без tmux» уже была, но в обход города.

**[В]** Dashboard заменяет `peek` и «ответить на разрешение», но не заменяет `attach` (свободный диалог) и не является
главной сессией.

### 2.3. Статус ACP по интересующим возможностям

| Возможность | Статус на 2026-11-09 | Источник |
|---|---|---|
| Транспорт | В спеке только stdio; «A remote transport … is being specified» | **[П]** `docs/protocol/v1/transports.mdx:8` |
| Streamable HTTP + WebSocket | RFD **Active** (не Preview, не Completed); в v1 «in-flight messages are not replayed», reconnect — забота реализации | **[П]** `docs/docs.json:210-214`; `docs/rfds/streamable-http-websocket-transport.mdx:70-78` |
| `session/list`, `session/resume`, `session/close` | Completed (стабильны в v1) | **[П]** `docs/docs.json:242-250` |
| Multi-client attach (`session/attach`) | **Открытый PR #533**, создан 2026-02-18, последний комментарий 2026-11-05; роли controller/observer выброшены, «proxy broadcasts everything» | **[П]** `gh pr view 533 -R agentclientprotocol/agent-client-protocol` |
| Несколько соединений на сессию | v2 draft: «up to the Agent»; агент MAY отклонить второй `session/resume` или вытеснить первое соединение | **[П]** `docs/protocol/v2/session-setup.mdx:54` |
| Ввод во время работы | v2 draft: промпты «inserted or held in order, not rejected»; явные queue/steer — «later RFD» | **[П]** `docs/rfds/v2/prompt.mdx:88` |
| Состояние «ждёт человека» | v2 draft: `state_update`: `running`, `idle`, `requires_action` | **[П]** `docs/rfds/v2/overview.mdx:68` |
| `session/status` (спросить без attach) | RFD **Draft**; прямо назван сценарий «dashboards … surface a session blocked on a permission request» | **[П]** `docs/rfds/session-status.mdx:141`; `docs/docs.json:204` |
| Proxy chains (conductor) | RFD **Draft**; реализован в Rust SDK; цепочка линейная, M:N — «in the future» | **[П]** `docs/rfds/proxy-chains.mdx:594-618`; `docs.json:199` |
| Client system prompt | Completed: `systemPrompt` в `session/new`, режимы append/override | **[П]** `docs/rfds/client-system-prompt.mdx:14-18, 200`; `docs.json:266` |
| MCP-over-ACP | RFD Active: клиент/прокси отдаёт инструменты агенту по тому же каналу | **[П]** `docs/rfds/mcp-over-acp.mdx:8-10` |
| v2 целиком | Draft с 2026-07-20: «Don't ship it by default in production» | **[П]** `docs/announcements/acp-v2-draft.mdx:59-61` |

**[В]** Спека движется ровно в сторону H10 (replay, messageId, state, multi-client упоминается как цель в v2 prompt RFD),
но на сегодня всё, что нужно для «пульта», либо Draft, либо не слито. Строить на этом можно только как на собственном
поведении хаба, совместимом с v1.

### 2.4. Эксперимент: два клиента на одной сессии claude

Скрипт `lab/control-plane/scripts/acp2.py`: два процесса `claude-agent-acp` 0.102.0 (A и B), 3 минимальных LLM-запроса.

```
A session/new -> e33902e2-…            A prompt1 "Remember KIWI" -> 'OK'
B session/list count: 1 | contains A's session: True
B session/load (A still alive) -> OK | replayed: user_message_chunk 1, agent_message_chunk 1
B prompt "second codeword is MANGO" -> 'OK2'
A received any update from B's turn: {}
A prompt2 "List every codeword" -> 'KIWI'
```

Транскрипт `~/.claude/projects/…control-plane-work/e33902e2-….jsonl`: оба хода — B («MANGO», uuid `223b980a`) и следующий
ход A (uuid `e2def3ae`) — имеют один и тот же `parentUuid 893c7b69`. **[П]**

Следствия **[В]**: (1) `session/list` + `session/load` из чужого процесса работают — человек может открыть сессию
оркестратора готовым клиентом без какой-либо поддержки с нашей стороны; (2) это не «та же живая сессия», а второй писатель в
тот же файл: живого стрима нет, владелец не узнаёт о чужом ходе, история ветвится, одна из веток молча выпадает из
контекста. Документация Claude Code говорит то же: «messages from both interleave into one transcript», блокировки не
описаны (https://code.claude.com/docs/en/sessions.md — по отчёту субагента, **[НП]** мной лично).

### 2.5. Нативные мульти-клиентные возможности харнессов

| | codex 0.166 | claude 2.1.322 | pi 1.5.0 |
|---|---|---|---|
| Демон на много сессий | `codex app-server daemon start`, «Shared clients» **[П]** `codex-rs/app-server-daemon/README.md:1-30` | `claude remote-control` (server mode, `--spawn`, `--capacity`) и supervisor для `--bg` **[П]** (`claude remote-control --help`, по отчёту субагента) | нет **[П]** (`pi --help`: режимы text/json/rpc) |
| Второй клиент на живую сессию | да: resume уже работающего thread «atomically subscribing for new updates»; `serverRequest/resolved` **[П]** `app-server/src/thread_state.rs:67`, `app-server-protocol/src/protocol/common.rs:2108` | для сессий под Remote Control: «stay in sync across all connected devices» (https://code.claude.com/docs/en/remote-control.md); точная семантика двух водителей **[НП]** | нет |
| Родной TUI к демону | `codex --remote` (`ws://`, `wss://`, `unix://`), `--remote-auth-token-env` **[П]** (`codex --help`) | `claude attach <id>` к `--bg` сессии **[П]** (help, по отчёту субагента) | нет |
| Список/сетка сессий | `codex agents` — «Browse all agent sessions on the shared local app-server daemon» **[П]** | `claude agents`: Working / Needs input / Idle / Completed / Failed; только background-сессии (https://code.claude.com/docs/en/agent-view.md) | нет |
| Телефон | `codex remote-control start` / `pair` (experimental) **[П]** (help) | Remote Control → claude.ai/code, iOS/Android; push «when actions required» | нет |
| Работает для headless/ACP-сессий | актуальный `@agentclientprotocol/codex-acp` 2.6.0 «starts the Codex App Server» сам **[П]** (README репозитория, строка 7); ходит ли он в общий демон и пустит ли второй клиент — **[НП]**. Клон `src/codex-acp` (zed-industries, 2026-07) устарел: встраивал `codex-core` `rust-v0.137.0` **[П]** `Cargo.toml:27` | `--bg` несовместим с `-p`; про SDK/`claude-agent-acp` docs молчат **[НП]** | — |
| Ограничения | experimental, «lifecycle contract may change» **[П]** | только подписка claude.ai; API-key/Bedrock/Vertex не поддержаны (remote-control.md) | — |

Столбец claude: флаги `--bg`, `--remote-control`, `--cloud`, `--teleport`, команды `agents`, `attach`, `remote-control
--spawn` — **[П]** мной по `claude --help`; поведение и ограничения — по docs-URL из отчёта субагента (лично не перечитывал).

**[В]** «Бесплатный» пульт от вендора надёжно существует только в связке «оркестратор ↔ общий codex app-server» (не ACP).
Для claude бесплатный пульт (Remote Control, agent view) привязан к интерактивному TUI — то есть к тому самому способу
запуска, от которого уходим; и к подписке.

### 2.6. ACP-клиенты, пригодные как пульт

Собрано субагентом по документации, README и коду репозиториев на 2026-11-10; **ни один клиент не запускался**. V = видел
в документе/коде, U = не подтверждено.

| Клиент | Произвольная команда-агент | Удалённый агент | `session/list` + открыть существующую | Параллельные сессии | Активность |
|---|---|---|---|---|---|
| **Zed** | V: `agent_servers.<name>` `{type:"custom", command, args, env}` | URL-транспорта нет (V); в SSH-проектах команда резолвится через remote server, где реально бежит — U | V: «Import Threads» тянет сессии по ACP, нужен `session/list`; постоянная синхронизация — U | V: Threads Sidebar | v1.27.0, 2026-11-04 |
| **JetBrains AI Assistant** | V: `~/.jetbrains/acp.json` | subprocess; remote не описан | частично: `session/load` при открытии чата (V), `session/list` — нет данных | U | 2026.2 |
| **VS Code «ACP Client»** | V | нет данных | V: список через `session/list` | нет, «one agent active at a time» (V) | пуш 2026-05-16 |
| **CodeCompanion.nvim** | V: `adapters.acp` | нет данных | V: `/resume` = `session/list` + `session/load` | U | v20.1.0, 2026-11-08 |
| **agentic.nvim** | V: `acp_providers` | нет данных | V | V | пуш 2026-11-08 |
| **avante.nvim** | V: `acp_providers` | нет данных | V | U | v0.5.0, 2026-10-31 |
| **Emacs agent-shell** | V | V: `agent-shell-command-prefix` оборачивает запуск (docker/devcontainer; ssh — U) | V: выбор из resumable-сессий | V | пуш 2026-11-09 |
| **Toad** (TUI) | V: `toad acp "<command>"` | `toad serve` отдаёт TUI в браузер; агент локальный | частично: `session/load` есть, `session/list` в клиенте нет — список свой (V) | V | v0.6.20, 2026-05-26, без пушей с тех пор |
| **acpx** (headless CLI) | V: `--agent '<cmd>'` | только stdio; команда может быть мостом | V: `sessions list` через `session/list`; `session/resume`/`load` | V: именованные сессии, очередь промптов на сессию | v0.22.0, 2026-11-07 |
| **hydra-acp** | V | V: `daemon listen` (TLS + token), `hydra://host/id` | V: TUI со списком и attach к работающим | V | v0.1.217, 2026-11-09, 47 звёзд, 1 автор |
| **Obsidian Agent Client** | V | нет данных | V | V | 0.13.0, 2026-09-14 |
| **AionUi** (desktop + WebUI) | V | агенты локальные; свой WebUI для телефона | частично | V | v2.2.2, 2026-09-09 |
| **ACP UI** (desktop/Android/web) | V: `agents.json` | V: `transport: "websocket"`; web/mobile только WebSocket | частично: `session/load`, список локальный (U) | U | v0.1.16, 2026-05-25 |
| **Agmente** (iOS), **Ferngeist** (Android), **Panda** (web) | нет (только endpoint) | V: `ws(s)://` к мосту stdio↔ws | Panda — V; остальные U | U | Agmente 2026-03; Ferngeist v1.6.1, 2026-11-08 |
| **marimo** | нет: пять зашитых агентов (V) | ws через `stdio-to-ws` | нет, `MAX_SESSIONS = 1` (V) | нет | — |

Источники: `src/agent-client-protocol/docs/get-started/clients.mdx`; zed `docs/src/ai/external-agents.md`,
`crates/agent_ui/src/thread_import.rs`; https://www.jetbrains.com/help/ai-assistant/acp.html;
CodeCompanion.nvim `doc/agent-client-protocol.md`; github.com/carlos-algms/agentic.nvim;
github.com/xenodium/agent-shell; github.com/batrachianai/toad `src/toad/acp/api.py`; github.com/openclaw/acpx
`docs/sessions.md`, `docs/session-watch.md`; github.com/smagnuso/hydra-acp; github.com/formulahendry/acp-ui;
github.com/rebornix/Agmente; github.com/arafatamim/Ferngeist; marimo `frontend/src/components/chat/acp/state.ts`.

Что из этого следует **[В]**:

- «Произвольная команда» есть почти у всех — шим `<cli> acp` подключится к любому редактору без доработок клиента.
- `session/list` + открытие существующей сессии умеют Zed, CodeCompanion, agentic.nvim, avante, agent-shell, acpx, Obsidian.
  Хаб **обязан** реализовать `session/list` и `session/load` с replay — иначе клиент покажет только «новую сессию».
- Удалённого ACP-транспорта в стандарте нет, и редакторы его не умеют; мобильные клиенты (ACP UI, Agmente, Ferngeist)
  ходят на нестандартный WebSocket-мост (`stdio-to-ws`, `acpremote`, `aptove/bridge`).
- Наблюдатель за чужой живой сессией есть только вне протокола: `hydra-acp` (broadcast + очередь промптов + «первый ответ
  на permission выигрывает») и `acpx sessions watch` (replay + follow через локальный IPC к владельцу очереди).
- Уведомления «агент ждёт»: Zed (`agent.notify_when_agent_waiting`), Ferngeist (push), Obsidian (статус «waiting for
  permission») — только пока клиент подключён.

### 2.7. Готовые хабы и мосты

- `hydra-acp`: демон владеет агентами; `hydra-acp acp` — шим для редакторов; «prompts serialize through a per-session queue;
  permission requests race (first response wins)»; автор реализовал PR #533 целиком и предложил правки к нему. Статус
  «experimental», 1391 коммит одного автора с 2026-05-09. **[П]** README; `gh api repos/smagnuso/hydra-acp/contributors`;
  комментарий smagnuso в PR #533 от 2026-05-20
- `acp-multiplex` (Go, 12 звёзд, последний пуш 2026-03-02): первичный клиент + сокет для вторичных; автор:
  «Permission handling across devices works, as does replay», потребовалось нестандартное `turn_complete`. **[П]** PR #533
- Conductor из Rust SDK — линейная цепочка прокси, не хаб на много агентов. **[П]** `docs/rfds/proxy-chains.mdx:594-618`
- `goose serve` — референс-реализация HTTP/WS-транспорта RFD (по отчёту субагента, **[НП]** мной).

## 3. Вердикты

**H10 — частично подтверждена.** «Подключиться по ACP как к control pane» реально, если оркестратор сам является
ACP-агентом-хабом: тогда главная сессия и любая рабочая открываются из Zed/Toad/agent-shell через `session/list` +
`session/load`. Неверна только буквальная версия («подключиться к ACP-серверу харнесса, с которым уже говорит оркестратор»):
протокол этого не даёт (PR #533 открыт), а обход через второй процесс адаптера ломает сессию (2.4). Цена варианта — хаб надо
написать: fan-out событий, очередь ввода, гонка разрешений, replay. `hydra-acp` показывает, что это объём одного автора за
полгода, а не исследовательская задача.

**H1 — частично подтверждена (в моей части).** Для *управления* агентами tmux не нужен: ACP даёт структурированные
ход/разрешение/состояние вместо send-keys и capture-pane, и убирает класс багов вроде #8150. Для *человека* tmux сегодня
незаменим в трёх местах: (1) диалоги самого харнесса вне протокола (`/login`, истёкшая авторизация, model switch);
(2) выживание сессий при рестарте демона; (3) нулевой порог входа с телефона по ssh. Первое закрывается аварийным выходом
в родной TUI (вариант e), второе — resume из транскрипта, третье — терминальным ACP-клиентом по ssh. То есть tmux уходит из
ядра, но остаётся как необязательный инструмент оператора.

**H4 — уточнение с точки зрения пульта.** Пульту нужен один долгоживущий процесс-владелец сессий (демон) — это следует из
1:1 природы ACP независимо от экономии памяти. Но «один ACP-сервер на много сессий» и «человек подключается к тому же
серверу» — разные вещи: stdio-сервер адаптера принимает одно соединение, и «тем же сервером» для человека будет хаб, а не
процесс харнесса.

## 4. Следствия для дизайна и варианты

### 4.1. Как человеку попасть в ту же живую сессию

| Вариант | Сложность | UX | claude | codex | pi | Риски |
|---|---|---|---|---|---|---|
| **(a) Оркестратор = ACP-хаб** (`<cli> acp` шим → демон; fan-out, очередь промптов, первая реакция на permission выигрывает) | средняя–высокая | лучший: любая сессия города в любом ACP-клиенте, replay + живой стрим | да | да | да (через адаптер) | своя семантика multi-client до принятия #533; как клиенты отрисуют `user_message_chunk`, пришедший вне replay (чужой ввод), — **[НП]**; v2 — Draft |
| **(b) Handoff** (демон закрывает свой клиент, человек делает resume в своём клиенте/TUI, потом возвращает) | низкая | рваный: сессия выпадает из оркестрации на время визита | да (`claude --resume`, `session/load`) | да (`codex resume`) | да (`pi --session`) | без жёсткой блокировки — ветвление транскрипта (2.4); нужно уметь «догнать» состояние после возврата |
| **(c) Свой CLI/TUI/web поверх журнала событий**, ACP только «вниз» | низкая для CLI, высокая для web | предсказуемый, единый для всех харнессов; diff/markdown рисовать самим | да | да | да | объём UI-работы; повтор судьбы dashboard Gas City (просмотр есть, диалога нет) |
| **(d) Нативные пульты харнессов** | низкая | отличный у codex, у claude — только для TUI/bg-сессий | частично, нужна подписка | да при работе через общий app-server-демон; через `codex-acp` — **[НП]** | нет | три разных пульта; experimental; привязка к вендору; для claude противоречит headless-запуску |
| **(e) tmux/родной TUI только как просмотрщик** (оператор сам запускает TUI с resume) | минимальная | знакомый, но только по одному агенту и только при остановленном владельце | да | да | да | то же, что (b); живую сессию «подсмотреть» нельзя |

**[В]** Варианты не исключают друг друга. (c) и (a) делят одно ядро — журнал событий сессии с курсором и очередь ввода
в демоне; (a) есть сериализация этого журнала в ACP `session/update`. (b)/(e) — один и тот же механизм
`takeover/release`, нужный в любом случае как аварийный выход.

Что нужно от хаба, раз спека этого не даёт: единственный писатель в агента (демон), очередь ввода с пометкой источника
(человек / order / nudge), рассылка `session/request_permission` всем подключённым с отменой у проигравших, replay из
журнала при `session/load`, `session/list` по всему городу (а не по `cwd`). Всё это укладывается в v1 без расширений;
`_meta` — для источника ввода. **[В]**

### 4.2. Главная сессия

Сегодня desk — это агент-профиль (модель, prompt, hooks, deny-правила) + tmux-окно. **[П]** `metropolis/agents/desk/agent.toml`.
В новой системе **[В]**:

- desk = named session профиля `desk`, которой владеет демон (как любой другой), с `mode = always`;
- человек открывает её через `<cli> acp` в Zed/Toad/agent-shell либо через `<cli> chat desk`;
- «тот же конфиг» гарантирует демон: он создаёт сессию у харнесса с нужными `systemPrompt`, `mcpServers`, cwd и
  hooks; клиент человека не участвует в конфигурации. Иначе каждый клиент принесёт свои `mcpServers`, и desk из Zed будет
  отличаться от desk из Toad.

Инструменты такой сессии:

| Вариант | За | Против |
|---|---|---|
| CLI оркестратора как bash-команды (как сейчас: `gc metropolis …`, `gc mail …`) | ноль новой инфраструктуры; одинаково для claude/codex/pi; hooks уже умеют фильтровать команды (coordinator-deny) | права = права шелла; вывод неструктурирован; permission-запросы на каждую команду, если не настроить allow |
| MCP-сервер оркестратора (stdio/HTTP, один на город) | типизированные инструменты, точечные разрешения, один процесс на город (см. H9) | ещё одна поверхность; у pi MCP — через пакет/расширение **[НП]** |
| MCP-over-ACP от хаба | без второго канала и порта | RFD Active, только MCP 2026-07-28; поддержка в адаптерах **[НП]** |

Рекомендация **[В]**: v0 — CLI (уже работает и проверено паком владельца), MCP — когда появится общий реестр MCP.

### 4.3. Уведомления

ACP доставляет события только подключённому клиенту: `notice` (Completed), `state_update: requires_action` (v2 draft),
`session/request_permission`. Отсутствующему человеку протокол ничего не пошлёт. **[П]** `docs/rfds/session-notices.mdx:11-15`.
Поэтому **[В]**: источник уведомлений — журнал событий демона (`requires_action`, formula done/failed, quota/429, crash),
приёмники подключаемые: mail в ящик `human` (сохранить — это работает и не прерывает), desktop notify, ntfy/Telegram
webhook. Для claude как дополнительный вход годятся hooks `Notification` (`permission_prompt`, `idle_prompt`) — по отчёту
субагента они работают в `-p`, срабатывание каждого события там **[НП]**.

### 4.4. Удалённый доступ и безопасность

- Самый дешёвый «remote transport» — ssh: в клиенте custom agent command = `ssh host <cli> acp`. Работает с любым клиентом,
  умеющим произвольную команду, и не требует WebSocket-RFD. **[В]** Ограничение: клиентские `fs/*` и `terminal/*` (v1)
  указывали бы на машину клиента — хаб не должен их объявлять агенту (в v2 они удалены: **[П]** `docs/rfds/v2/overview.mdx:57`).
- WebSocket/HTTP — RFD Active; в v1-варианте без replay после обрыва. Для телефона это критично, поэтому хабу всё равно
  нужен собственный replay по курсору. **[П]** `streamable-http-websocket-transport.mdx:70-78`
- Аутентификация: в stdio-ACP её нет — кто запустил процесс, тот и клиент. Для шима это означает «права = доступ к
  unix-сокету демона» (owner-only), для сети — bearer + TLS (так делает `hydra-acp`: токен в `~/.hydra-acp`,
  `daemon listen tailnet`). **[П]** README hydra-acp. Ввод человека и ввод оркестратора должны различаться в журнале.

### 4.5. Наблюдение за многими агентами

tmux давал «глазами» N окон; ни один ACP-клиент-редактор не даёт сетку чужих сессий со статусами. Готовые сетки есть
только у вендорских пультов (`codex agents`, `claude agents`), у `hydra-acp` (TUI с переключением сессий) и у терминальных
мультиплексоров агентов вроде herdr («every pane is marked working, blocked, or idle» **[П]** `src/herdr/README.md`) — но
последние снова про терминалы. **[В]** Сетку придётся строить самим, и она дешёвая, если есть журнал событий.

Минимум для v0 **[В]**:

```
<cli> sessions                # таблица: агент, риг, bead, state (running/idle/requires_action/error/quota), возраст
<cli> tail <session> [-f]     # лента событий сессии из журнала (replay + follow)
<cli> send <session> "…"      # ввод в очередь демона
<cli> respond <session>       # ответ на pending permission
<cli> acp [--session S]       # stdio-шим: демон как ACP-агент для Zed/Toad/agent-shell
<cli> takeover <session>      # демон паркует сессию и печатает команду resume для родного TUI; <cli> release — вернуть
```

Отложить: web-dashboard, WebSocket-транспорт, одновременных «водителей», мобильное приложение, v2.

### 4.6. Что владелец теряет и приобретает

| Теряет | Чем компенсировать |
|---|---|
| Родной TUI «как есть» (slash-команды харнесса, `/login`, model switch, статус-строка) | `takeover` для редких случаев; большинство slash-команд адаптеры отдают как `available_commands` |
| Универсальность: в tmux запускается любой CLI | осознанное сужение до claude/codex/pi |
| Агенты переживают рестарт демона | resume из транскрипта при старте; незавершённый ход теряется |
| Нулевая инфраструктура для телефона (ssh + tmux) | ssh + терминальный ACP-клиент или `<cli> tail/send` |

| Приобретает |
|---|
| Явное состояние «ждёт человека» вместо эвристик по экрану |
| Ввод через очередь: нет гонки nudge с диалогом |
| Главная сессия в IDE (diff, файлы, ссылки) вместо отдельного tmux |
| Один список всех сессий города, replay любой из них |
| Разрешения как данные (кто, что, когда одобрил) |

## 5. Открытые вопросы и непроверенное

- **[НП]** Поведение конкретных клиентов при `session/load` «чужой» сессии, в которой прямо сейчас идёт ход (покажут ли
  стрим; как отрисуют ввод, пришедший не от них). Нужен тест хаба с Zed и Toad.
- **[НП]** Регистрируются ли сессии `claude -p`/Agent SDK/`claude-agent-acp` в Remote Control или agent view; срабатывают
  ли все `Notification`-hooks в headless.
- **[НП]** Семантика двух одновременных «водителей» у codex app-server на практике (проверен только код, не запуск);
  виден ли `codex --remote`/`codex agents` app-server, поднятый `codex-acp` 2.6.0.
- **[НП]** Вся таблица клиентов 2.6 — по документам, без запуска; Tidewave, Sidequery, aizen найти не удалось.
- **[НП]** Конкурентный доступ к одной сессии у codex (`codex resume` параллельно с `codex-acp`) и у pi — не запускал;
  документация pi о блокировках молчит (`packages/coding-agent/docs/sessions.md`, grep по «lock|concurren» пуст).
- Примет ли ACP #533 в текущем виде и попадёт ли он в v2 — неизвестно; хаб должен жить без него.
- Нужен ли вообще «второй водитель», или достаточно «наблюдатель + очередь ввода»? Для одного оператора — скорее второе.
- Допущение: владелец работает один, на своей машине или по ssh; многопользовательский доступ не рассматривался.
