# 17. Ландшафт оркестраторов кодинг-агентов (ноябрь 2026): tmux vs структурированные протоколы, build vs fork vs adopt

Дата среза: 2026-11-10. Звёзды, дата последнего push, лицензия, archived — из `gh api repos/<owner>/<repo>` на эту дату **[ПРОВЕРЕНО]**. README скачаны в `lab/landscape/readmes/` (только чтение, ничего не запускалось). LOC не измерял — размер кода ниже не указываю, чтобы не выдавать догадки за цифры.

## 1. Выводы за 60 секунд

- **Готовой «альтернативы Gas City» под задумку владельца нет.** Ближайшие — `kandev` (ACP-first, но AGPL и без beads), `NEEDLE` (beads-очередь + headless, но 33★, свой `bead-rs`, без ACP/pi) и `Orchestrator.inc` (лучший донор слоя сессий, но это desktop-продукт без трекера). Вердикт H11: писать своё тонкое ядро, заимствуя спецификации и идеи, а не форкать.
- **Индустрия действительно уходит от tmux, но не в ACP, а в нативные протоколы харнессов.** Лидеры (Symphony, t3code, Paseo, vibe-kanban) ведут Codex через `codex app-server`, Claude — через Agent SDK / `stream-json`, а ACP оставляют для «длинного хвоста» агентов.
- **ACP для всех трёх нужных харнессов — второй слой поверх нативного протокола.** `codex-acp` переписан поверх app-server, `claude-agent-acp` — обёртка над Agent SDK, для pi есть только community-адаптер поверх `pi --mode rpc`.
- **pi + ACP — самое слабое место H3.** Автор pi четыре раза отказал в нативном ACP (дек. 2025 — окт. 2026). Адаптер `svkozak/pi-acp` 0.0.34 по задуманной политике держит один живой pi-процесс на ACP-соединение и убивает остальные сессии; фикс (PR #131) не слит с 15.09.2026.
- **Мигрировавшие с tmux:** Overstory (headless по умолчанию, tmux — опция), Orchestrator.inc (tmux теперь «legacy or fallback»), `coder/agentapi` (эмуляция терминала + HTTP — deprecated и в архиве), `zed-industries/codex-acp` (архив, переезд на app-server).
- **Оставшиеся на tmux держат его как хостинг процесса, а не как протокол.** Agetor запускает в tmux по одному headless-вызову на ход (`codex exec --json`, `stream-json`), потому что tmux переживает рестарт оркестратора. Этот аргумент закрывается демоном-супервизором или detached-хостом на сессию.
- **Событийность (H8) у лучших гибридная.** Symphony — эталон headless — сам работает по poll-тику 30 с для трекера, но события агента стримит. Orchestrator.inc делает CDC из SQLite в SSE, а внешний мир (PR) опрашивает раз в 30 с.
- **Вендоры съедают однохарнессную оркестрацию.** В Claude Code уже есть supervisor-демон с фоновыми сессиями, agent view, workflows (до 1000 агентов на прогон), routines, channels, межсессионные сообщения через Unix-сокет. В Codex — общий app-server-демон, `codex agents`, сабагенты, worktrees по умолчанию, automations.
- **Смертность в сегменте высокая.** Vibe Kanban (28.6k★) закрывается, Overstory в архиве, его преемник Warren брошен через полгода, Crystal стал Nimbalyst, agentapi deprecated.
- **Уникальная ценность внешнего оркестратора сузилась** до: межхарнессность, работа в бидах, риги (связка репо), ордеры поверх своего трекера, маршрутизация квот между аккаунтами, единые хуки/политики/MCP на город.
- **Неожиданное:** nudge без tmux у Claude уже есть штатно — у каждой сессии (включая `claude -p`) inbox-сокет `CLAUDE_CODE_MESSAGING_SOCKET`, куда может писать внешний скрипт.

## 2. Факты

### 2.1 Сводная таблица

Обозначения транспорта: **T** — tmux, **PTY** — свой pty без tmux, **HS** — headless stream (`-p`/`exec --json`/`stream-json`), **SDK** — Agent SDK in-process, **AS** — `codex app-server`, **ACP**. Все строки **[ПРОВЕРЕНО]** по README/коду/`gh api`, кроме помеченных.

| Проект | Транспорт | Хранилище работы | Модель управления | UI человека | claude / codex / pi | Изоляция | Квоты / краши | Язык, ★, push, лицензия |
|---|---|---|---|---|---|---|---|---|
| Gas Town `gastownhall/gastown` | T | beads + Dolt | роли в Go, патрули | tmux attach | да / да / нет | worktree | патрули-роли | Go, 18.7k, 2026-10-28 (только deps), MIT |
| Gas City `gastownhall/gascity` | T по умолчанию; есть `acp`, `subprocess`, `k8s`, `herdr`, `t3bridge` (`internal/runtime/`) | beads (bd + Dolt) | контроллер-reconciler по тикам | tmux + dashboard | да / да / на бумаге | worktree | reconciler | Go, 1.5k, 2026-11-10, MIT |
| ntm `Dicklesworthstone/ntm` | T | `br`/`bv` + Agent Mail | человек + промпты; robot-JSON, REST/SSE | TUI | да / да / нет | worktree + file reservations | `ntm rotate` (аккаунты, контекст) | Go, 464, 2026-11-10, своя лицензия |
| agent-deck | T | реестр сессий | «conductor»-сессии + watchers | TUI, Telegram/Slack | да / да / нет | worktree, Docker | cost dashboard; **MCP socket pool** | Go, 1.2k, 2026-11-09, MIT |
| claude-squad | T | нет | человек | TUI | да / да / нет | worktree | нет | Go, 8.8k, 2026-08-20, AGPL-3.0 |
| ccmanager | PTY | нет | человек | TUI | да / да / нет | worktree | нет | TS, 1.3k, 2026-10-27, MIT |
| CAO `awslabs/cli-agent-orchestrator` | T + локальный `cao-server` + MCP | свои сессии | supervisor→workers, flows по расписанию | web UI, tmux | да / да / нет | tmux-сессии | — | Python, 1.5k, 2026-11-10, Apache-2.0 |
| herdr `herdrdev/herdr` | свой PTY-сервер + socket API | раскладка на диске | не оркестратор — «runtime» терминалов | TUI, remote | любой CLI | нет | детект working/blocked/idle | Rust, 49.8k, 2026-11-07, Apache-2.0 |
| Agetor | T как хостинг; внутри HS на ход; ACP для cursor/fx | SQLite | kanban | desktop | да / эксп. / нет | worktree | reattach после рестарта | TS, 123, 2026-11-10, MIT |
| NEEDLE `jedarden/NEEDLE` | HS (`claude -p`), воркеры в tmux | **beads** (`bead-rs`, SQLite) | детерминированная state machine | CLI | да / да / нет | общий checkout | явный handler на каждый исход, gates | Rust, 33, 2026-11-10, MIT |
| Overstory → Warren | HS по умолчанию, tmux опция → HTTP control plane | SQLite mail → run store | watchdogs | web UI | да / да / да (адаптеры) | worktree; bwrap/docker/k8s | spend caps, reconcile | TS; **архив 2026-05**; Warren **«No longer maintained (Nov 2026)»**, MIT |
| `coder/agentapi` | эмуляция терминала + HTTP | нет | — | chat UI | да / да / нет | — | — | Go, 1.5k, **архив, deprecated** |
| OpenAI Symphony | AS (stdio) | трекер (Linear), БД нет | poll-тик 30 с + reconcile + backoff | опц. dashboard | нет / да / нет | workspace на issue + hooks | exp. backoff, stall timeout | Elixir + SPEC.md, 28.0k, 2026-09-15, Apache-2.0 |
| t3code `pingdotgg/t3code` | AS + `@anthropic-ai/claude-agent-sdk` + ACP для прочих | свой сервер | человек | web/desktop/mobile | да / да / нет | worktree | usage-limits ingestion | TS, 54.3k, 2026-11-10, MIT |
| vibe-kanban | `stream-json` + control protocol (claude), AS (codex), ACP (gemini/qwen) | своя БД | kanban | web | да / да / нет | worktree | — | Rust, 28.6k, 2026-11-09, Apache-2.0; **sunsetting** |
| Paseo `getpaseo/paseo` | демон; AS (codex), свой claude-провайдер, ACP (copilot/cursor/generic) | демон | человек + агенты запускают агентов | desktop/mobile/web/CLI, WebSocket API | да / да / да | worktree, Docker | — | TS, 23.7k, 2026-11-10, Apache-2.0 (по README) |
| Orchestrator.inc (бывш. ComposioHQ) | TUI: нативный PTY, **tmux — legacy/fallback**; Chat: нативный протокол/ACP в detached-хосте на сессию | SQLite + `change_log` | CDC→SSE; опрос PR 30 с | desktop, mobile, kanban | да / да / ? | worktree | reaper, auth-факты | Go, 14.4k, 2026-11-09, Apache-2.0 |
| Orca `stablyai/orca` | «Chat or Terminal» **[транспорт в коде НЕ ПРОВЕРЕН]** | своё | агент-оркестратор + automations | desktop/mobile | да / да / «any CLI» | worktree, cloud VM | usage & accounts | TS, 120.5k, 2026-11-10, MIT |
| Paperclip | адаптеры `claude-local`, `codex-local`, `pi-local` **[протокол НЕ ПРОВЕРЕН]** | своя БД (тикеты) | heartbeats, routines | web | да / да / да | workspaces | бюджеты, авто-пауза | TS, 112.7k, 2026-11-10, MIT |
| kandev `kdlbs/kandev` | **ACP-first** (`claude-agent-acp`, `codex-acp`, `pi-acp`); AS эксп.; PTY passthrough | SQLite | kanban + workflows с gates + automations (cron/webhook) | web, mobile | да / да / да | worktree, Docker, SSH | — | Go, 1.1k, 2026-11-10, **AGPL-3.0** |
| claw-orchestrator | headless runtime; движки + `engine: 'acp'` | checkpointed runs | durable run kernel + acceptance-контракты | dashboard, CLI | да / да / нет | копии каталогов | resume после usage limit, `maxBudgetUsd` | TS, 604, 2026-11-10, MIT |
| Claudexor | typed-адаптеры к CLI | файлы | гонки best-of-N + ревью | desktop | да / да / нет | patches | **ротация аккаунтов по квоте** | TS, 524, 2026-11-10, MIT |
| acpx `openclaw/acpx` | ACP-клиент (CLI) | `~/.acpx/` | очередь промптов на сессию | NDJSON | да / да / да | cwd | — | TS, 3.4k, 2026-11-09, MIT, pre-1.0 |
| sandbox-agent `rivet-dev/agents` | HTTP/SSE поверх харнессов | universal session schema | — | API | да / да / да | sandbox | — | TS, 1.6k, 2026-11-08, Apache-2.0 |

Источники строк: `lab/landscape/readmes/*.md`; `lab/landscape/ao_architecture.md:8-13` (Orchestrator.inc); `lab/landscape/symphony_SPEC.md`; дерево `apps/server/src/provider/` и `apps/server/package.json` t3code; `crates/executors/src/executors/{claude,codex,gemini}.rs` vibe-kanban (`--output-format=stream-json`, `app-server`, `--experimental-acp`); дерево `packages/server/src/server/agent/providers/` Paseo.

### 2.2 Тренд: уход от tmux

**Кто мигрировал [ПРОВЕРЕНО]:**
- Overstory: «New projects spawn Claude agents headless and surface them through a web UI; `tmux attach` is the opt-in escape hatch» (README `jayminwest/overstory`).
- Orchestrator.inc: «A TUI session runs its agent inside a native PTY/ConPTY runtime, with tmux as a legacy or fallback runtime. A Chat session runs a native protocol controller without an agent terminal runtime» (`ao_architecture.md:8-10`).
- `coder/agentapi`: «AgentAPI is deprecated and no longer maintained» (README).
- `zed-industries/codex-acp`: «The new adapter is built on the new Codex App Server» (README, архив).

**Аргументы против tmux** (вторичные источники, сам читал только выдачу поиска **[НЕ ПРОВЕРЕНО]**): send-keys/capture-pane — «хрупкий screen-scraping самого себя»; tmux не различает «ждёт подтверждения» и «хвост лога» (https://codex.danielvaughan.com/2026/04/09/cmux-acpx-omx-three-layers-multi-agent-ux/, https://www.bitdoze.com/best-terminal-multiplexers/).

**Аргументы оставшихся [ПРОВЕРЕНО по README Agetor]:**
1. Живучесть: «run inside `tmux`, so quitting Agetor doesn't stop them, and the next launch reconnects». Там же признан минус ACP-варианта: cursor/fx идут дочерним процессом, и ход в процессе не переживает рестарт.
2. «Drive the real CLI; don't re-implement it»: Claude получает промпты нажатиями клавиш, структурированный вывод читается из его же JSONL-транскрипта.
3. Attach к родному TUI. Orchestrator.inc решил это двумя режимами с durable handoff TUI↔Chat, но только для Claude и Codex, где доказано, что resume id и protocol id — один разговор (`ao_architecture.md:395-400`).
4. Подписка. По справке Anthropic (обновление 07.10.2026) Agent SDK и `claude -p` при логине планом «still draw from your subscription limits», анонсированное разделение пулов поставлено на паузу (https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan). Аргумент «только TUI безопасен для подписки» сейчас слабее, чем весной, но политика менялась четыре раза за год — это риск, а не константа **[ВЫВОД]**.

### 2.3 pi и ACP (перепроверка по запросу оркестратора)

| Утверждение | Статус | Доказательство |
|---|---|---|
| pi идёт в ACP только через community-адаптер | **подтверждено** | в `src/pi-mono` grep `agentclientprotocol` по `*.ts/*.md/*.json` пуст; README pi перечисляет режимы interactive / print / JSON / RPC / SDK |
| версия 0.0.34 | **подтверждено** | `src/pi-acp/package.json`; HEAD `04d0a15` 2026-10-05 |
| «~98 открытых issues» | **уточнено** | `open_issues_count=98` — это 49 issues + 49 PR (`gh api search/issues`) |
| баг: убийство параллельных сессий | **подтверждено, и это политика, а не случайность** | `src/acp/agent.ts:384` и `:967` вызывают `closeAllExcept(session.sessionId)` в `session/new` и `session/load`; комментарий: «within a single ACP connection … keep only one live pi subprocess»; `src/acp/session.ts:203-208` |
| фикс PR #131 не слит | **подтверждено** | open, создан 2026-09-15, обновлён 2026-10-26; issue #152 открыт |
| автор pi отказался от нативного ACP | **подтверждено, 4 раза** | badlogic: #175 (2025-12-13) «no need for ACP … that's a no»; #241 (2025-12-19) «I don't want to carry that baggage if ACP turns out to be a dud»; #836 (2026-01-19) «do not want a dedicated acp mode in pi»; #10389 (2026-10-08, not_planned) «no, that is something you can build yourself» — https://github.com/earendil-works/pi/issues/175, /pull/241, /pull/836, /issues/10389 |

Дополнительно **[ПРОВЕРЕНО]**:
- Адаптер порождает по процессу `pi --mode rpc --no-themes` на сессию (`src/pi-rpc/process.ts:162`). Для pi гипотеза H4 «один сервер — много сессий» через ACP не выполняется.
- MCP-серверы из ACP-параметров «not wired through to pi»; нет `fs/*` и `terminal/*` (README, Limitations). Общий MCP через ACP для pi не заработает.
- Адаптер отстаёт от pi: issue #159 просит статус совместимости с pi 1.x, последний релиз адаптера — 24.09.2026, локально стоит pi 1.5.0.

**Эксперимент с параллельными сессиями не выполнялся.** Он требует запуска кода `svkozak/pi-acp`, а этот пакет не входит в белый список правила 5 CONTEXT.md (zed-industries / agentclientprotocol / anthropic / openai / badlogic), и в моём задании стоит «ничего не запускать из клонов». Механизм доказан статически: вызов безусловный, политика описана в комментарии, воспроизведение расписано в PR #131. Обход очевиден из того же комментария: один процесс `pi-acp` на сессию — тогда `closeAllExcept` никого не задевает **[ВЫВОД]**. Если нужен живой прогон — нужно явное разрешение на запуск этого пакета.

### 2.4 Что харнессы уже делают сами

**Claude Code 2.1.x [ПРОВЕРЕНО по code.claude.com/docs и `claude --help`]:**
- Фоновые сессии под supervisor-демоном: `claude --bg`, `claude agents --json`, `attach / logs / stop / respawn / rm`, `claude daemon status`. «If a session process exits unexpectedly … the supervisor restarts the process»; зависшая сессия перезапускается и продолжает ответ. Состояние — `~/.claude/jobs/<id>/state.json`. Research preview (https://code.claude.com/docs/en/agent-view).
- Dynamic workflows: JS-скрипт с `agent() / pipeline() / parallel()`, 16 параллельных агентов по умолчанию (до 256), 1000 агентов на прогон, resume по сохранённым результатам, авто-рестарт зависшего агента до 5 раз. **Ожидание сброса лимита работает только в интерактивной сессии с подпиской — не в `claude -p`, не в Agent SDK, не в фоновой сессии** (https://code.claude.com/docs/en/workflows).
- Scheduled tasks (бывш. routines): облако или self-hosted environment; триггеры — расписание, API POST, GitHub events; research preview (https://code.claude.com/docs/en/routines).
- Cross-session messaging: `ListAgents` / `SendMessage`, inbox — Unix-сокет на сессию, путь в `CLAUDE_CODE_MESSAGING_SOCKET`; сокет есть и у `claude -p` (кроме `--bare`); `notify_when_idle` — уведомление, когда другая сессия простаивает (https://code.claude.com/docs/en/cross-session-messaging).
- Channels: MCP-сервер пушит события в живую сессию; research preview, allowlist плагинов (https://code.claude.com/docs/en/channels).
- Agent teams: экспериментально, выключено по умолчанию; режим отображения `in-process` или `tmux` — то есть сам Claude Code использует tmux только как UI-раскладку (https://code.claude.com/docs/en/agent-teams, по выдаче поиска).

**Codex 0.166 [ПРОВЕРЕНО по `codex --help` и learn.chatgpt.com/docs/app-server]:**
- `codex agents` — «Browse all agent sessions on the shared local app-server daemon»; `codex queue` — поставить сообщение в очередь существующей сессии; `codex remote-control`; `codex cloud`; `--remote <ADDR>`.
- app-server: stdio (JSONL), WebSocket («experimental and unsupported»), Unix-сокет; `thread/resume`, `thread/fork`; много тредов в одном процессе (`thread/loaded/list`).
- Сабагенты (GA, лимит 6), worktrees по умолчанию, automations в desktop-приложении — по вторичным источникам **[НЕ ПРОВЕРЕНО]** (https://developers.openai.com/codex/changelog).

**pi 1.5:** режимы `text / json / rpc`, SDK, extensions, `pi mcp`; в репо есть пакет `durable` (последний коммит — «suspendable work design») **[ПРОВЕРЕНО: `pi --help`, `ls src/pi-mono/packages`]**. Встроенной оркестрации нет — она вынесена в расширения.

**Что из Gas City это делает ненужным [ВЫВОД]:** для одного харнесса — надзор за процессом, рестарт при краше, fan-out сабагентов, attach, расписания, nudge. **Что остаётся внешнему оркестратору:** маршрутизация работы между claude/codex/pi, персистентный граф работы в бидах, риги, ордеры от событий своего трекера, учёт квот между аккаунтами и харнессами, единые политики/хуки, merge-очередь.

## 3. Вердикты

**H1 (tmux не нужен, всё решается через ACP) — частично подтверждена.**
Подтверждено: при написании с нуля tmux как протокол общения не нужен; лидеры обходятся без него, мигрировавшие называют его legacy. Опровергнуто «через ACP»: краш, квота и resume у лучших решаются нативным протоколом харнесса плюс собственным супервизором. Остаются две функции tmux, которые надо заменить явно: (а) процесс агента должен переживать рестарт оркестратора — detached-хост на сессию (Orchestrator.inc) или вендорский демон; (б) attach человека к родному TUI — либо отказаться, либо поддержать resume той же сессии в TUI. Квоты: вендорское автоожидание лимита в headless-режимах не работает, оркестратор обязан обрабатывать лимит сам — backoff как в Symphony §8.4 или ротация аккаунтов как в Claudexor/ntm.

**H3 (claude, codex, pi нормально работают через ACP; хватит одного транспорта) — опровергнута со стороны рынка.**
Ни один крупный проект не использует ACP как единственный транспорт для claude и codex. Исключение — kandev (1.1k★), и даже он держит «Codex app-server … separate experimental native integration» и PTY passthrough. Для pi ACP — чужой адаптер с политикой «одна живая сессия на соединение» и без проброса MCP. ACP для всех трёх — надстройка, которая отстаёт от нативного протокола.

**H8 (тики → события) — частично подтверждена.**
Нормой стала гибридная схема: события хода агента — стрим; изменения локального стора — CDC/триггеры → SSE (`ao_architecture.md:207-213`); внешние системы — опрос. Symphony держит тик 30 с и «Reconciliation runs before dispatch on every tick» (`symphony_SPEC.md:725-750`). С локальным трекером (br/SQLite) простои между тиками устраняются полностью, но reconcile-проход как страховка остаётся у всех.

**H10 (control pane = подключение к ACP) — направление верное, механизм другой.**
Рынок пришёл к «демон + тонкие клиенты»: Paseo (WebSocket API, desktop/mobile/web/CLI), t3code, Orchestrator.inc, `claude attach`, `codex --remote`. Через ACP как таковой это пока делают только мелкие мосты: `2389-research/acp-relay` (6★, HTTP/WS), `formulahendry/ahp-acp-gateway` (19★). Умеет ли базовый ACP несколько клиентов на одну живую сессию — не проверял, это вопрос к исследователю спецификации **[НЕ ПРОВЕРЕНО]**.

**H11 (переписать с нуля) — подтверждена с оговоркой про объём.**
Форкать нечего, adopt целиком — тоже. Писать с нуля оправданно только как тонкий слой над вендорскими демонами и нативными протоколами.

## 4. Следствия для дизайна

### 4.1 Топ-3 ближайших проекта

| Проект | Чего не хватает до задумки | Основа или донор | Лицензия |
|---|---|---|---|
| **kandev** | нет beads (своя SQLite-модель задач), нет rigs/formulas/orders в терминах Gas City; UI-центричен; pi только через pi-acp | Основа — нет: AGPL-3.0 заразит всю систему. Донор идей: ACP-хост, executors (local/Docker/SSH), task-agent MCP, automations по cron/webhook | AGPL-3.0 |
| **NEEDLE** | нет ACP и pi; воркеры живут в tmux; свой `bead-rs` вместо `br`; нет ригов, ордеров, MCP-реестра; 33★, один автор | Основа — рискованно. Донор: «deterministic state machine that drives non-deterministic agents», явный handler на каждый исход, shipped-work gate | MIT |
| **Orchestrator.inc** | нет трекера-бидов, формул, ордеров; desktop-продукт с телеметрией и облаком; pi не заявлен | Основа — нет, слишком продуктовый. Лучший донор слоя сессий: detached-хост на сессию, controller generation fence, SQLite + CDC, handoff TUI↔Chat | Apache-2.0 |

Доноры второго ряда: **Symphony SPEC.md** (Apache-2.0; сам README предлагает «Implement Symphony according to the following spec» — готовая спецификация планировщика, retry, workspace hooks, учёта токенов), **Paseo** (Apache-2.0; провайдеры claude / codex-app-server / ACP / pi в одном демоне), **agent-deck** (MIT; MCP socket pool), **Claudexor** (MIT; credential profiles и ротация по квоте), **claw-orchestrator** (MIT; acceptance-контракты, которые агент не может переписать), **ntm** (интеграция с `br`/`bv`, ротация контекста).

### 4.2 Архитектурные уроки

1. **Восстановление — от трекера и файловой системы, а не от БД планировщика.** «Restart recovery is tracker-driven and filesystem-driven (without a durable orchestrator DB)» (`symphony_SPEC.md:729`, §14.3). Это аргумент за биды как единственный источник истины.
2. **Retry — экспоненциальный backoff с потолком, плюс короткая задержка «продолжения».** `delay = min(10000 * 2^(attempt-1), max_retry_backoff_ms)`, по умолчанию потолок 5 мин; после чистого выхода — 1 с (`symphony_SPEC.md` §8.4).
3. **Stall-детектор по времени последнего события агента**, а не по содержимому экрана (`symphony_SPEC.md` §8.5 Part A). Claude workflows делают то же: рестарт зависшего агента до 5 раз.
4. **Процесс агента отвязан от жизни оркестратора.** «Codex and all ACP Chat processes run in detached per-session hosts, so replacing the daemon … does not stop an in-flight turn» (`ao_architecture.md:10-13`).
5. **Событийность через CDC локального стора.** SQLite-триггеры → `change_log` → poller → broadcaster → SSE (`ao_architecture.md:207-213`). С `br` — аналог через его SQLite/JSONL.
6. **Control pane — демон с API и несколькими клиентами**, а не выделенная «главная сессия» (README Paseo: «Clients like the desktop app, mobile app, web app, and CLI connect to it»).
7. **Общий MCP — пул через Unix-сокеты.** «Socket pooling shares MCP processes across all sessions via Unix sockets, so each server runs once instead of once per session. A health check every 3 seconds restarts a crashed MCP behind a reconnecting proxy» (README agent-deck, «MCP Socket Pool»). Это прямое подтверждение идеи H9; есть и отдельный `atlasfoo/mcp-relayd` (1★).
8. **Кросс-харнессные хуки — на уровне рабочего пространства и оркестратора.** Symphony задаёт workspace hooks с таймаутами в `WORKFLOW.md` (§5.3.4, §9.4); политика approval/sandbox передаётся полями нативного протокола (§10.2). Единого стандарта хуков между харнессами на рынке нет **[ВЫВОД]**.
9. **Политика живёт в репозитории.** `WORKFLOW.md` = YAML front matter + шаблон промпта, перечитывается на лету (`symphony_SPEC.md` §5, §6.2). Близко к formula/order без молекул.
10. **Вердикт о завершении выносит рантайм, а не агент.** Acceptance-контракты claw-orchestrator («Verdicts the agent cannot write»); shipped-work gate NEEDLE — закрытие бида принимается только при запушенном коммите.
11. **Стоимость — поштучный ledger ходов плюс жёсткий бюджет.** Symphony: брать абсолютные тоталы треда и считать дельты, чтобы не задвоить (§13.5); claw: `maxBudgetUsd` на прогон; Warren: spend caps во время исполнения.
12. **Квоты между аккаунтами — отдельная подсистема.** Claudexor: «rotates a spent account out of the way on typed vendor limits», причём только там, где у вендора есть источник квоты; ntm: `rotate all-limited`.

### 4.3 Варианты (build vs fork vs adopt)

- **A. Тонкое ядро с нуля (рекомендую).** Драйверы: claude через Agent SDK/`stream-json` (или вендорский supervisor), codex через app-server, pi через `--mode rpc`. ACP — только как внешний фасад для Zed/JetBrains и для экзотики. Планировщик — по Symphony SPEC, источник работы — `br`. Риск: поддержка трёх нативных протоколов; но ACP-путь означает те же три протокола плюс три адаптера.
- **B. ACP-only, как просит владелец.** Работает для claude и codex. Для pi придётся форкать pi-acp или запускать по процессу адаптера на сессию. Теряются: общий MCP для pi, свежие фичи нативных протоколов. Экономия на едином клиенте реальна, но оплачивается зависимостью от трёх чужих адаптеров.
- **C. Форк kandev или Orchestrator.inc.** kandev отпадает из-за AGPL, если система не будет открыта под той же лицензией. Orchestrator.inc — Apache-2.0, но придётся вырезать desktop/cloud/телеметрию и вживлять биды; это дороже варианта A.
- **D. Не писать, а надстроить вендорские демоны** (`claude --bg` + `codex agents` + скрипты над `br`). Самый дешёвый старт и лучший способ проверить, сколько оркестратора вообще нужно. Минус: research-preview API меняются.

Разумная последовательность: начать с D как прототипа, вырастить в A.

### 4.4 Риск «написать то, что через полгода появится у вендоров»

- **Высокий** для: надзора за процессами, фоновых сессий, fan-out сабагентов, расписаний, мобильного и удалённого управления, worktree-изоляции. Всё это уже отгружено или в preview у Anthropic и OpenAI (§2.4).
- **Низкий** для: межхарнессной маршрутизации, бидов как стора, ригов, ротации аккаунтов. У вендоров нет стимула делать это для чужих харнессов.
- **Рыночный сигнал:** Vibe Kanban закрылся, потому что «couldn't find a business model» (https://www.vibekanban.com/blog/shutdown, 10.04.2026); автор Overstory дважды за год бросил проект; автор Gas Town, по вторичным источникам, ушёл в приватный харнесс (https://latent.space/p/ainews-reality-checks-on-ai-news/ **[НЕ ПРОВЕРЕНО]**, при этом репозиторий жив — последние коммиты только обновления зависимостей).
- **Следствие:** всё в зоне высокого риска — делегировать вендорским механизмам за тонким интерфейсом драйвера и не писать своё. Собственный код тратить только на зону низкого риска.

## 5. Открытые вопросы и непроверенное

- Живой прогон параллельных сессий pi-acp не делал (см. §2.3) — нужно явное разрешение на запуск community-пакета.
- Транспорты Orca и Paperclip в коде не смотрел; Conductor, Sculptor, Amp, Factory droids, Cursor background agents, GitHub Agent HQ, Claude Managed Agents, happy, omnara, OpenHands-ACP отдельно не исследовал — в таблицу не включал.
- Workflow-движки (Temporal/Restate/Inngest) под CLI-агентов: реального применения не нашёл; NEEDLE прямо пишет, что они «bad at non-deterministic agent steps». Целевого поиска не делал.
- Мультиклиентный attach к одной ACP-сессии — не проверял.
- Работает ли вендорский supervisor Claude (`--bg`) под управлением внешнего оркестратора так же надёжно, как под человеком, и как он ведёт себя при лимите подписки — раздел Limitations страницы agent-view не дочитан (обрезан при загрузке).
- Сведения о Codex automations, лимите сабагентов и статусе JetBrains ACP — из вторичных источников.
- Число звёзд у ряда репозиториев в выдаче `gh` выглядит аномально высоким; привожу как есть, на выводы оно не влияет.
