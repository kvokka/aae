# 12. beads_viewer (`bv`) и экосистема Flywheel как кирпичи и как прецедент

Дата: 2026-11-10. Гипотезы: H5 (часть про bv), H10, частично H1.
Пути ниже: `SRC` = `/home/ubuntu/proj/work/gascity/tmp/gc-alt-research/src`, `LAB` = `/home/ubuntu/proj/work/gascity/tmp/gc-alt-research/lab/bv`.
Версии в экспериментах: `bv` 0.25.0 и `br` 0.6.0 (те, что стоят у владельца). Последние релизы: bv 0.26.0 (2026-11-02), br 0.7.6 (2026-10-15), ntm 1.38.0, mcp_agent_mail_rust 0.3.40 — **не запускались** (правило 5: чужие бинарники не исполняю).

## 1. Выводы за 60 секунд

- `bv` — это **read-only аналитика и TUI поверх чужого хранилища**, а не менеджер задач. Менеджер — `br`. Связка «br + bv» заменяет «bd + Dolt» только если хранилищем считать br; bv в ней необязателен.
- bv 0.25.0 читает SQLite-базу br 0.6.0 **напрямую** (`source_kind: "sqlite"`), без экспорта в JSONL; также читает `.beads/issues.jsonl` и bd/Dolt (через автоматический `bd export`).
- Robot-режим пригоден для машин: 41 команда, `contract_version 1.0.0`, JSON Schema, стабильный конверт с `data_hash`/`claim_safe`. `--robot-next/plan/triage` на 10k бидов: ~150 мс с JSONL и ~950 мс с SQLite.
- Но в «горячий путь» диспетчера bv ставить не нужно: `--robot-plan` включает уже взятые (`in_progress`) биды и эпики, `--robot-triage` отдаёт top-10, а не очередь; атомарный claim и так делает `br update --claim`, а `br ready`/`br scheduler` дают то же без второго бинарника.
- Multi-repo (`.bv/workspace.yaml`) — рабочий аналог «city + rigs» для **просмотра**: два репо грузятся, claim-команда маршрутизируется в нужный риг. Межриговые зависимости в формате br (`external:api:<id>`) bv **не понимает** — бид остаётся заблокированным навсегда.
- `--robot-insights` на 10k бидов занял 36–54 с (на 1k — 0.3 с). Тяжёлую аналитику нельзя дёргать синхронно.
- В 0.25.x есть признанная автором гонка: запись br посреди загрузки могла показать заблокированную работу как готовую (исправлено в 0.26.0). Владельцу стоит обновиться до любого использования bv в автоматике.
- **Лицензия всего стека — «MIT + OpenAI/Anthropic Rider»** (не OSI-open-source, добавлена задним числом 2026-02-22). Автор не принимает внешние PR. Bus factor = 1.
- Автор Flywheel сам оркестрирует рой **через tmux** (`ntm`), и ntm разросся до 491k строк Go без тестов. «Тонкий стек» — миф образца 2025 года.
- Лимиты и квоты во Flywheel ловятся **регулярками по тексту пане/stdout** (`rate.?limit`, `\b429\b`, `capacity`) плюс недокументированный usage API. Это ровно та хрупкость, от которой владелец хочет уйти.
- ACP у автора есть только на периферии: драйвер в `flywheel_gateway` (25 звёзд) и `pi --acp` в его Rust-порте pi. В ntm ACP — строчка в планах. Его ответ на хрупкость tmux — свой терминальный движок (FrankenTerm), не ACP.
- `mcp_agent_mail` — готовый прецедент «один HTTP MCP-сервер на машину для всех харнессов» (`127.0.0.1:8765/mcp/`), но это 1.25M строк Rust с резервированием файлов 4.7 с по медиане на 60 агентах.
- Архитектура «тонкий оркестратор поверх br (+ bv как панель задач)» реалистична. Контроллер, формулы, ордера, ACP-слой и учёт квот придётся писать самим — в Flywheel их либо нет, либо они намертво привязаны к tmux.

## 2. Факты

### 2.1. Что такое bv

| Вопрос | Ответ | Источник |
|---|---|---|
| Язык/стек | Go 1.26.10, Bubble Tea/lipgloss, cobra, fsnotify; Rust только для WASM-графа в статическом экспорте | **[ПРОВЕРЕНО]** `SRC/beads_viewer/go.mod:4-25`, каталог `bv-graph-wasm/` |
| Порт на Rust | Есть отдельный репо `beads_viewer_rust` (`bvr` v0.4.1, 2026-10-13, 28 звёзд); называет Go-версию «legacy Go `bv`», «Legacy TUI parity is still in progress» | **[ПРОВЕРЕНО]** README bvr, строки 79, 1408 (`LAB/readmes/beads_viewer_rust.md`) |
| Источники данных | SQLite `beads.db` (приоритет 100) → JSONL из worktree (80) → локальный JSONL (50); имена `issues.jsonl`, `beads.jsonl`, `beads.base.jsonl`; `.beads/redirect`; `--db`/`BEADS_DB`/`BEADS_DIR` | **[ПРОВЕРЕНО]** `internal/datasource/source.go:33-50`, `README.md:4024-4033` |
| bd/Dolt | Определяет workspace bd и сам запускает `bd export` с ограничением по времени; при сбое показывает старый экспорт как stale и не выдаёт claim | **[ПРОВЕРЕНО]** `README.md:99-119`, `internal/datasource/load.go:241-262` |
| Совместимость с br 0.6.0 | Работает: `source_path: …/small/.beads/beads.db`, `source_kind: sqlite`, `valid: 6` | **[ПРОВЕРЕНО]** эксперимент: `cd LAB/small && bv --robot-next` |
| Live-reload | fsnotify + debounce 250 мс, fallback-поллинг 2 с; следит и за `-wal` | **[ПРОВЕРЕНО]** `pkg/watcher/debouncer.go:10`, `pkg/watcher/watcher.go:16`, `README.md:4031-4033` |
| Live-reload, замер | `br q …` → `bv --export-pages … --watch-export` перегенерировал сайт через ~1.15 с | **[ПРОВЕРЕНО]** эксперимент, `LAB/out/watch.log` |
| TUI | list, board (kanban, swimlanes), graph, tree, insights (10 панелей), history, flow matrix, sprint, labels, attention, alerts, time-travel, repo picker | **[ПРОВЕРЕНО]** `bv --robot-help` (раздел Views); `bv --debug-render board` отрисовал доску |
| Мутации | Почти нет: только правка frontmatter бида в `$EDITOR`. Создать/закрыть/назначить из TUI нельзя | **[ПРОВЕРЕНО]** `README.md:189-208` |

**Robot-режимы** (**[ПРОВЕРЕНО]** `bv --robot-help`, `bv --robot-capabilities` → `n_cmds: 41, contract_version: "1.0.0"`; `bv --robot-schema` → `schema_version: "1.0.0"`): triage / triage-by-track / triage-by-label / next / plan / priority / insights / graph (json, dot, mermaid) / alerts / drift / diff / history / orphans / file-beads / file-hotspots / impact / file-relations / related / blocker-chain / impact-network / causality / forecast / capacity / burndown / sprint-* / label-* / suggest / search / metrics.

Метрики графа: PageRank, betweenness (точная <500 узлов, выборочная выше), HITS, eigenvector, critical path, degree, density, циклы (Tarjan SCC), topo sort, k-core, articulation points, slack (**[ПРОВЕРЕНО]** `README.md:478-492, 4526-4533`). Каждая метрика несёт статус `computed|timeout|skipped`.

Прочее, проверенное в `LAB/small` (git-репо с закоммиченным `issues.jsonl`):

- Time-travel: `bv --robot-diff --diff-since HEAD~1` → `issues_closed: 1, issues_modified: 1`; `--as-of HEAD~1` → `source_kind: "git"`. **[ПРОВЕРЕНО]** Работает только если `issues.jsonl` коммитится в git.
- Корреляция бид↔коммит: `bv --robot-history` связал коммит с `sm-kde.1` методом `co_committed`, confidence 0.99; `--robot-file-beads schema.sql` → `closed: ["sm-kde.1"]`. **[ПРОВЕРЕНО]**
- Экспорт: `--export-md` (10.6 КБ, с mermaid), `--robot-graph --graph-format mermaid`, `--export-graph x.html` (316 КБ, один файл), `--export-pages` (6.1 МБ статический сайт с `beads.sqlite3`, за 0.19 с). **[ПРОВЕРЕНО]**
- «Agent swarm»: треки = компоненты связности графа (Union-Find). Пересечения по файлам **не** учитываются: «They do not detect overlapping file edits or reserve work» (`README.md:1455`). **[ПРОВЕРЕНО]**

### 2.2. Замеры (9 ядер, OrbStack VM; медиана из 3 запусков, мс)

Данные синтетические: `LAB/scripts/gen.py`, 40% закрытых, 0–3 блокирующих ребра на бид, эпик на каждые 50. Харнесс: `LAB/scripts/timeit.py`.

| Команда | 1k JSONL | 10k JSONL | 1k SQLite (br) | 10k SQLite (br) |
|---|---|---|---|---|
| `bv --robot-next` | 35 | 175 | 138 | 924 |
| `bv --robot-plan` | 38 | 133 | 112 | 1071 |
| `bv --robot-triage` | 42 | 158 | 110 | 960 |
| `bv --robot-triage-by-track` | 32 | 151 | — | — |
| `bv --robot-priority` | 37 | 286 | — | — |
| `bv --robot-graph` | 28 | 187 | — | — |
| `bv --robot-insights` | 308 | **38 265–44 994** | — | — |
| `bv --robot-insights --force-full-analysis` | 353 | 53 607 | — | — |
| `br ready --json` | — | — | 75 | 907 |
| `br scheduler --format json --limit 10` | — | — | 76 | 657 |

Дополнительно **[ПРОВЕРЕНО]**: пик RSS `bv --robot-triage` — 40 МБ на 1k, 116–119 МБ на 10k. Импорт JSONL в br (`br sync --import-only`): 1k — 4.8 с, 10k — 63 с. Размер вывода `--robot-plan` на 10k — 217 КБ, `--robot-graph` — 2.7 МБ.

Расхождение с документацией: README обещает для ≥2000 узлов таймауты 200–500 мс на метрику (`README.md:4531`), но `--robot-insights` на 10k работал 36+ с. Причину не разбирал. **[НЕ ПРОВЕРЕНО]** — какая стадия тормозит.

**Детерминизм** **[ПРОВЕРЕНО]**: `--robot-plan` побайтно одинаков между запусками (без `generated_at`). У `--robot-triage` порядок `recommendations[].id` стабилен (3 из 3), но `score` плавает в 9-м знаке — в формулу входят staleness/urgency от текущего времени.

### 2.3. Пригодность bv как «мозга» диспетчера

| Свойство | Наблюдение | Источник |
|---|---|---|
| Учёт claim | `--robot-next` выдаёт `claim_command` только для open + unblocked + unassigned; после claim вернул `actionable: false` | **[ПРОВЕРЕНО]** эксперимент в `LAB/small` |
| Атомарность claim | Делает br, не bv: второй `br --actor agent-B update --claim sm-kde.1` → `VALIDATION_FAILED … already assigned to agent-A` | **[ПРОВЕРЕНО]** эксперимент |
| `--robot-plan` и взятая работа | Включает `in_progress`-биды и открытый эпик как «actionable» | **[ПРОВЕРЕНО]** вывод: `[{"id":"sm-kde","status":"open"},{"id":"sm-kde.1","status":"in_progress"},{"id":"sm-zzk","status":"in_progress"}]` |
| Очередь на N агентов | `--robot-triage` даёт 10 рекомендаций (`nrec: 10` при 179 actionable); в 0.25.0 первой шла заблокированная | **[ПРОВЕРЕНО]** эксперимент; порядок изменён в HEAD: коммит `516e96a fix(triage): recommend ready work by default` |
| Несколько ригов | `.bv/workspace.yaml` с двумя репо: `loaded: 2`, claim для `api-c6w` идёт с `BEADS_DIR=…/rigs/api/.beads`; `--repo fe` сужает выбор | **[ПРОВЕРЕНО]** эксперимент в `LAB/city` |
| Межриговые зависимости | `br dep add fe-qie external:api:api-c6w` принят br; bv считает `fe-qie` заблокированным и после `br close api-c6w` (`blocked: 1`, `root_blockers: []`). В коде bv нет разбора `external:` — неизвестный id считается локальным | **[ПРОВЕРЕНО]** эксперимент; `pkg/workspace/loader.go:600-608`; формат br: `SRC/beads_rust/README.md:1483-1486` |
| События | Push-канала нет ни у bv, ни у br 0.6.0 (в `br --help` нет `events`/`serve`/`watch`). Только файловые события | **[ПРОВЕРЕНО]** `br --help` |
| Гонка чтения в 0.25.x | Release notes 0.26.0: «a `br` write landing mid-load can no longer show work as ready when it is blocked» | **[ПРОВЕРЕНО]** `gh release view v0.26.0 -R Dicklesworthstone/beads_viewer` |

**[ВЫВОД]** Для оркестратора достаточно `br ready --json` + `br update --claim`. bv добавляет ранжирование по графу (что разблокирует больше всего), но это совет, а не очередь. Разумная роль bv — фоновый советник (`--robot-triage --brief` раз в N минут или по изменению файла) и панель для человека.

### 2.4. Зрелость и риски

- **Лицензия.** `SRC/beads_viewer/LICENSE` (73 строки, прочитан целиком) — «MIT License (with OpenAI/Anthropic Rider)». Райдер: никаких прав OpenAI, Anthropic, их аффилиатам и любому, кто действует «on behalf of, for the benefit of, or under the direction of» них; запрет предоставлять им ПО и производные; «use» включает «executing, benchmarking, testing, analyzing, indexing»; райдер обязан сохраняться в производных; нарушение автоматически прекращает лицензию. Тот же текст в br, ntm, agent mail, cass, caam, bvr. **[ПРОВЕРЕНО]** Введён 2026-02-22 коммитом «chore: update license to MIT with OpenAI/Anthropic Rider», до этого был обычный MIT (`gh api repos/…/commits?path=LICENSE`). **[ВЫВОД]** Вызывать `br`/`bv` как внешние CLI — низкий риск; вендорить, форкать или линковать их код в новую систему — значит унаследовать райдер и не-open-source статус. Я не юрист; владельцу стоит оценить это самому, особенно если новая система будет публичной.
- **Bus factor = 1.** Все коммиты — Jeffrey Emanuel (`git shortlog -sn`). Политика: «I do not accept outside contributions for any of my projects… I'll have Claude or Codex review submissions» (`SRC/ntm/README.md:2022`). **[ПРОВЕРЕНО]**
- **Темп.** За 30 дней: bv 60 коммитов, br 260, ntm 269, agent mail 948, cass 687 (`gh api …/commits?since=2026-10-10`). bv: 15 релизов с 2026-06-09 (`gh release list`). Большинство issues закрывается за 0–5 дней, отдельные висели 5–9 недель (#208, #197); открыто 3. **[ПРОВЕРЕНО]**
- **Качество релизов.** 0.26.0 выпущен с `eligible: false` в собственном release gate (обойдён вручную) и с открытым #215 «15 known vulnerabilities» в бинарниках. **[ПРОВЕРЕНО]** release notes, `gh issue list`.
- **Связка bv↔br.** bv зависит от внутренней схемы SQLite br (у br свой движок fsqlite с sidecar-файлами `-wal-cert`, `-fsqlite-ns-*` — видны в `LAB/small/.beads/`). Собственный репо bv ломался: «`.beads/beads.db` was at schema 0 and rejected by br 0.5.7 (`SCHEMA_MISMATCH expected 17, found 0`)» (`CHANGELOG.md:801`). **[ПРОВЕРЕНО]** Сочетание bv 0.26 + br 0.7.6 — **[НЕ ПРОВЕРЕНО]**. JSONL-путь от схемы не зависит и на 10k в 6 раз быстрее.

### 2.5. Flywheel: как автор сам гоняет рой

**ntm (Named Tmux Manager).** **[ПРОВЕРЕНО]** по `SRC/ntm` @ ccb43d0:

- «NTM is intentionally `tmux`-centric»; «Does NTM replace tmux? No» (`README.md:1998, 1969-1971`).
- Размер: 490 926 строк Go без тестов, 738 602 строки тестов; 99 каталогов в `internal/` (включая `coordinator`, `supervisor`, `pipeline`, `workflow`, `recovery`, `rotation`, `quota`, `ratelimit`, `handoff`, `swarm`).
- Умеет: spawn/add/send/interrupt/broadcast, dashboard и palette, checkpoints, ротацию контекста с handoff, robot-режим, `ntm serve` (REST/SSE/WebSocket/OpenAPI), pipelines (YAML: `steps`, `depends_on`, `foreach`, шаблоны — `e2e/pipeline/testdata/brennerbot-incident.yaml`), workflows с ручными гейтами, координатор с авто-назначением (по умолчанию выключено, `README.md:620-622`), `ntm swarm --scan-dir` на несколько проектов.
- Лимиты детектятся регулярками по тексту пане: `internal/robot/patterns.go:189-192`, `internal/robot/health.go:440-443`, `internal/ratelimit/tracker.go:572,691-694`. В тестах — случай «stale rate-limit pattern scrolled above a current idle» (`internal/robot/activity_test.go:861`): ложные срабатывания от старого скроллбэка — известная проблема.
- Восстановление при исчезновении пане: «verifies absence across the entire tmux server, rereads agent registries…» (`README.md:655-658`). Авто-ротация аккаунтов — отдельный резидентный монитор, только Linux, только при доказуемом транскрипте (`README.md:1423-1447`).
- ACP: в Go-коде нет (`grep -rlwE "ACP|acp" --include=*.go` пусто). Только в планах: «Future-proof architecture to support Agent Client Protocol (ACP) adapters if `tmux` spawning is replaced» (`docs/planning/PLAN_TO_ADD_WEB_UI…__GEMINI.md:191`).

**Почему tmux** — **[ВЫВОД]** из совокупности: (1) ставка на подписочные аккаунты через интерактивные CLI (caam: «You're paying $200-275/month for fixed-cost AI coding subscriptions», `README.md:105`); (2) человек может в любой момент подключиться к пане; (3) любой CLI-харнесс работает без адаптера. Цена — полмиллиона строк проверок идентичности пане и разбора текста.

**Куда автор идёт дальше.** Не к ACP, а к собственному терминалу: FrankenTerm — «Swarm-native terminal platform… real-time pane capture, change-point pattern detection». При этом его README прямо описывает боль: «You build glue with `tmux send-keys`, regex parsing of scrollback, and brittle sleep loops» (`LAB/readmes/frankenterm.md:654`). **[ПРОВЕРЕНО]**

**ACP на периферии.** `flywheel_gateway` (TypeScript/Bun, 25 звёзд) имеет четыре драйвера: `sdk`, `acp`, `tmux`, `ntm`. Таблица возможностей (`apps/gateway/src/config/drivers.ts`): у tmux `structuredEvents: false, toolCalls: false, checkpoint: false`; у acp всё `true`, кроме `terminalAttach`. ADR-004: «the tmux driver only infers state from terminal output». Но направление 2026 — «NTM execution plane (in progress)». Его Rust-порт pi поддерживает `pi --acp` (`LAB/readmes/pi_agent_rust.md:844`). **[ПРОВЕРЕНО]**

**mcp_agent_mail (Rust).** **[ПРОВЕРЕНО]** по `SRC/mcp_agent_mail_rust/README.md`:

- Транспорт: HTTP MCP на `127.0.0.1:8765/mcp/`; один процесс владеет хранилищем — stdio-экземпляр на тот же root отказывается стартовать (строки 566-573). «One server handles multiple projects. Each project is identified by its absolute filesystem path» (2142).
- Идентичности: `register_agent(project_key, program, model)` → имя вида `GreenCastle` или явное (827-846). Bearer-токен опционален.
- Подключение: `am setup run` пишет конфиги claude (`.mcp.json`/settings, `"type": "url"`), codex (`~/.codex/config.toml`, `[mcp_servers.mcp_agent_mail] url = …`), gemini, OMP (431-566). Оригинальный pi (badlogic) в списке детекта отсутствует — только OMP. **[НЕ ПРОВЕРЕНО]** работает ли с pi 1.5.0.
- 45 инструментов, 25 ресурсов; advisory-резервации файлов по glob с TTL + pre-commit guard.
- Ограничения (2125-2132): одна машина; резервации обходятся `--no-verify`; на 60 агентах `file_reservation_paths` — медиана ~4.7 с, p99 ~15 с (v0.3.39).
- Объём: 1 254 069 строк `.rs` в `crates/` (с тестами).

**cass.** 34 коннектора. Нужные пути (`SRC/coding_agent_session_search/README.md:419-432, 464`): Codex — `~/.codex/sessions` и `archived_sessions` (rollout JSONL, возможно `.jsonl.zst`); Claude Code — `~/.claude/projects` (session JSONL); pi — `~/.pi/agent/sessions` (JSONL, переопределяется `PI_CODING_AGENT_DIR`). **[ПРОВЕРЕНО]** Это подтверждает тезис владельца «логи и так есть»: форматы стабильны настолько, что сторонний индексатор живёт на них год. Любопытно: Grok Build хранит `updates.jsonl` как «authoritative ACP session-update stream» (строка 451).

**caam.** Подмена OAuth-файлов (`~/.claude.json`, `~/.codex/auth.json`). outOfQuota ловится двумя путями: (а) регулярки по stdout/stderr — для Claude в том числе `(?i)capacity` и `\b429\b` (`SRC/coding_agent_account_manager/internal/ratelimit/detector.go:39-50`); (б) проактивно — `https://api.anthropic.com/api/oauth/usage` (`internal/usage/claude.go:14`), про который README честно пишет «undocumented and may not be reliable» (`README.md:729`). `caam run` перезапускает **headless**-команду с другим аккаунтом; «Interactive sessions… are never replayed» (1886-1950). **[ПРОВЕРЕНО]**

**dcg / slb / ubs / ACFS** — кратко **[ПРОВЕРЕНО]** по README: dcg — один бинарник, ставящий нативные хуки в claude, codex ≥0.125.0, gemini, OMP и др. (прецедент «стандартизированных хуков поверх разных харнессов», H9); slb — «two-person rule» для опасных команд; ubs — статический сканер багов; ACFS — установщик всего стека на VPS, сервисы (`am`, `cm`, cass watch) живут в tmux-сессии `acfs-svc`.

### 2.6. Сопоставление понятий

| Gas City | Flywheel | Комментарий |
|---|---|---|
| city | нет; ACFS-машина + `ntm swarm --scan-dir=/projects` | города как объекта с конфигом нет |
| rig | проект = каталог; `.bv/workspace.yaml`; br routes | br знает раскладку Gas Town: «town-level routing… `mayor/town.json`» (`beads_rust/README.md:1468-1470`) |
| agent / session | пане tmux + имя Agent Mail | идентичность двойная, ntm сверяет их («discrepancy report») |
| bead | bead (br) | то же |
| formula | ntm pipeline YAML / workflow templates; чаще — промпт «marching orders» | появилось поздно; изначально «люди и промпты» |
| order (cron/событие) | **ничего** | в README ntm планировщика нет |
| molecule / wisp | **ничего** | план → биды руками или агентом |
| convoy | эпик + дочерние; треки bv | подтверждает, что так жить можно |
| sling / hook-очередь | `ntm assign`, либо агент сам берёт через `bv --robot-next` | pull-модель |
| nudge | `ntm send`, mail-nudge координатора | |
| mail | Agent Mail (MCP) | богаче, чем в Gas City |
| controller/reconciler | координатор в мониторе сессии ntm (опрос 15–30 с) | тоже тики, и только пока жива tmux-сессия |
| event bus | ntm events/SSE; у br/bv нет | |
| pack | recipes / session-templates / personas | |

**[ВЫВОД]** Flywheel тоньше Gas City там, где решение оставлено агенту (выбор работы, декомпозиция, разрешение конфликтов через почту) и человеку (запуск, расписание). Ему хватает, потому что автор сидит рядом с роем. Как только потребовалась автономность (авто-назначение, восстановление, ротация аккаунтов), ntm вырос до размеров, сравнимых с Gas City.

## 3. Вердикты

**H5 (в части bv) — частично подтверждена.** Хранилище заменяет br, не bv. bv 0.25.0 читает базу br 0.6.0 без посредников, даёт человеку панель, а машине — версионированный JSON. Оговорки: bv ничего не пишет; межриговые зависимости br он не видит; в 0.25.x гонка чтения; лицензия с райдером; один автор; тяжёлая аналитика на 10k — десятки секунд.

**H10 — частично.** «Чем человек смотрит на задачи» закрывается bv полностью (kanban, граф, история, алерты, статический сайт). Но bv не показывает агентов: нет списка сессий, статусов, стрима. Единственный мостик — клавиша `V` (сессии cass по биду, постфактум). У самого автора панель управления — это три разных TUI (ntm dashboard, `am`, bv). Значит, «подключиться к ACP» покрывает разговор с одним агентом, bv — задачи, а обзор флота (кто жив, кто упёрся в квоту) остаётся третьим компонентом, который надо писать.

**H1 — частично, с важной поправкой.** Прецедент Flywheel не подтверждает, что tmux не нужен: главный альтернативный стек тоже на tmux, и автор уходит не в ACP, а в собственный терминал. Но он же документирует цену: состояние «infers from terminal output», лимиты ловятся регулярками с ложными срабатываниями, восстановление требует обхода всего tmux-сервера. ACP-first в продакшене в этой экосистеме никто не эксплуатирует — владелец будет первопроходцем.

## 4. Следствия для дизайна и варианты

1. **Вариант A — «br-only ядро» (рекомендую).** Оркестратор говорит только с `br` (`ready --json`, `update --claim`, `close`, `dep`). События — inotify на `beads.db-wal`/`issues.jsonl` (тем же приёмом, что bv) вместо тиков. bv ставится рядом как панель для человека и запускается оркестратором изредка как советник по приоритетам. Плюс: одна жёсткая зависимость, bv можно выкинуть или заменить на bvr.
2. **Вариант B — «bv как планировщик».** Диспетчер берёт `bv --robot-triage-by-track` и раздаёт треки агентам. Плюс: готовое распараллеливание по компонентам графа. Минусы: фильтровать `in_progress` самому, читать через JSONL (быстрее и не зависит от схемы), привязка к семантике ранжирования, которая меняется между релизами.
3. **Вариант C — «Flywheel целиком без ntm».** br + bv + Agent Mail как общий MCP на город, свой ACP-оркестратор вместо ntm. Плюс: почта и резервации готовы, конфиги claude/codex пишутся `am setup run`. Минусы: 1.25M строк чужого Rust в критическом пути, медленные резервации, pi не поддержан из коробки. При worktree-на-агента резервации файлов почти не нужны, а почту можно сделать комментариями к бидам.

Что взять готовым: br (хранилище, claim), bv (панель задач, экспорт, time-travel), формат путей логов из cass (для восстановления сессий), идею dcg (один инсталлятор хуков на все харнессы), идею `am` (один HTTP MCP на машину, `project_key` = путь рига).
Что писать самим: контроллер, формулы, ордера, ACP-слой, обзор флота, учёт квот. Последнее — не копировать регулярки caam/ntm: через ACP ошибка лимита должна приходить структурно (проверяют другие отчёты).

Для multi-rig: либо один `.beads` на город с префиксами/метками ригов, либо по базе на риг и `.bv/workspace.yaml` только для просмотра; межриговые блокировки в последнем случае держать в оркестраторе.

## 5. Открытые вопросы и непроверенное

- bv 0.26.0 + br 0.7.6: совместимость, исправлена ли гонка, изменилась ли скорость SQLite-пути — не запускал.
- Почему `--robot-insights` на 10k занимает 36+ с вопреки заявленным таймаутам — не разбирал.
- Интерактивный TUI bv живьём не запускал (нет pty); виды подтверждены по `--robot-help`, README и `--debug-render board`.
- Поведение bv при конкурентной записи br под нагрузкой (десятки claim в секунду) не тестировал.
- Понимает ли bv межриговые зависимости, созданные через br routes (а не `external:`), — не проверял.
- Agent Mail с оригинальным pi 1.5.0 и вообще живой запуск `am` — не делал.
- Юридическая применимость райдера к сценарию владельца (инструменты запускаются агентами Claude/Codex по его поручению) — вне моей компетенции.
- Мотивы автора оставаться на tmux — мой вывод из косвенных признаков, прямого заявления не нашёл.
