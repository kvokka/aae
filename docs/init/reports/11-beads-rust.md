# 11 — beads_rust (`br`) как хранилище задач оркестратора вместо bd+Dolt (H5)

Дата: 2026-11-10. Исходники: `src/beads_rust` @ `8f62708` (main от 2026-11-09, `Cargo.toml:3` = 0.7.6 + незарелизенные коммиты; пути
ниже — относительно клона). Бинарники: `br` 0.6.0 (пин владельца) и **0.7.6** (последний релиз, 2026-10-15; скачан в
`lab/br/dl-0.7.6`, sha256 сверен с `checksums.sha256`), `bd` 1.3.2 (embedded Dolt, без сервера), `bv` 0.25.0.
Стенд: Linux aarch64 (OrbStack VM), 9 vCPU, 12 ГБ. Скрипты и логи: `lab/br/scripts/`, `lab/br/*.log`.
Метки: **[П]** = ПРОВЕРЕНО (сам видел в коде/выводе), **[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО.

> **Инцидент (нарушение правила 2, см. §5.1):** при тесте worktree `br` поднялся по дереву до `/home/ubuntu/.beads` (пустой
> трекер владельца от 2026-09-21) и создал там один тестовый бид `br-mq2 "from w1"`. Сам я его не удалял (это была бы ещё одна
> запись вне lab). Убрать: `cd ~ && br delete br-mq2 --hard`.

## 1. Выводы за 60 секунд

- **Как трекер рабочих задач br годится; как «универсальный substrate» вместо bd+Dolt — нет.** У бида нет `metadata`
  key-value, есть лимит 64 labels, нет событий/подписки, нет транзакции на несколько команд, нет эпика поверх нескольких БД. **[П]**
- **Конкурентность внутри одной БД корректна.** 16 процессов × create/claim/update/close: 0 потерь, 0 двойных claim, DB == JSONL,
  `doctor` healthy; 65–99 `SIGKILL` посреди записи — без потерь подтверждённых записей и без порчи (0.6.0 и 0.7.6). **[П]**
- **Но запись строго последовательна:** один `flock` на `.beads/.write.lock`, ~14 create/с и ~24 update/с при любом N; при 16 писателях
  p95 create = 1,9 с. Читатели параллельны (60–70/с на 5k бидов). **[П]**
- **Стоимость любой команды растёт линейно с размером БД, включая чтение одной строки.** 0.7.6: `show` 31 → 65 → 500 мс,
  `close` 81 → 311 → 3200–4400 мс на 100 / 5k / 50k бидов; RSS процесса до 1,25 ГБ на 50k. БД надо держать маленькой. **[П]**
- **Worktree одного репо автоматически делят одну БД** основного checkout-а; claim атомарен между worktree. Отдельные **клоны** —
  отдельные БД: двойной claim, git-конфликт в `issues.jsonl`, после которого в клоне отказывает даже `br ready`. **[П]**
- **Движок — не SQLite, а FrankenSQLite** (собственный pure-Rust движок автора, <1 года, nightly Rust). В августе 2026 он портил БД
  при многопроцессной записи; аварийный бэкенд на stock SQLite автор выпустил и откатил. **[П]**
- **Событийность придётся делать самим, но это возможно без поллинга CLI:** `inotify MOVED_TO` на `issues.jsonl` срабатывает ровно раз
  на мутацию, а таблицу `events` можно хвостить обычным SQLite read-only (0,05 мс, `PRAGMA data_version` растёт на каждую запись). **[П]**
- **Лицензия — MIT с «OpenAI/Anthropic Rider»** (не OSI), тот же rider у frankensqlite, asupersync, bv. Для личного инструмента риск
  низкий, для распространяемого продукта — реальный (§2.7). **[П]**
- **Bus factor = 1, внешние PR не принимаются** (1 merged из 63 внешних); 25 релизов за 10 недель; 457 issues за 10 месяцев,
  из них десятки про потерю данных/блокировки. **[П]**
- **Совместимость с bd односторонняя:** bd 1.3.2 импортирует JSONL от br; br 0.7.6 отвергает экспорт bd с комментариями и молча
  теряет `metadata`. bv 0.25.0 читает `beads.db` от br 0.7.6 напрямую. **[П]**
- **bd без Dolt-сервера (embedded) — не альтернатива:** в 10–15 раз медленнее br на 5k бидов, 2 create/с при 8 писателях. **[П]**
- **Рекомендация: вариант B** — br только для рабочих бидов (одна БД на город), состояние оркестратора в своей SQLite, br за узким
  адаптером, чтобы уход на собственный стор (вариант C) был дешёвым.

## 2. Факты

### 2.1 Архитектура

| Вопрос | Ответ | Источник |
|---|---|---|
| Источник истины | SQLite-файл `.beads/beads.db`; `issues.jsonl` — экспорт для git | `docs/ARCHITECTURE.md:29` **[П]** |
| Движок | `fsqlite` =0.4.10 (FrankenSQLite), «br never links a C SQLite»; WAL | `Cargo.toml:50-62`; `pragma journal_mode` → `wal` **[П]** |
| Формат файла | Совместим со stock SQLite 3.53: python `sqlite3` читает БД | эксперимент §2.5 **[П]** |
| Синхронизация DB↔JSONL | **Автоматическая по умолчанию**: `sync.auto_flush=true` (полная перезапись JSONL после каждой мутации, temp+rename), `sync.auto_import=true` (перед командой, если JSONL изменился) | `br config schema`; inotify-лог `lab/br/watch.log` **[П]** |
| Демон | Нет. Каждая команда — отдельный процесс; cold start `br --version` 1,8 мс | `t.py` **[П]** |
| Git | br запускает `git` только на чтение (`changelog`, `orphans`, `stats`, `vcs-status`, `doctor`); коммитов/хуков нет | `grep 'Command::new("git")' src` — 5 файлов **[П]** |
| Файлы в `.beads/` | `beads.db`, `-wal`, `-shm`, `-wal-cert`, `-wal-cert-head`, `-fsqlite-ns-gate/-use`, `.fsqlite-migration-state`, 4–6 lock-файлов, `.br_history/`, `.br_recovery/`, `beads.base.jsonl` | `ls` после `br init` **[П]** |
| Размер кода | 327 529 строк Rust в `src/` (144 файла; ~половина — inline-тесты), 197 876 строк в `tests/`, ~7 000 `#[test]`, 631 пакет в `Cargo.lock` | `wc -l`; `docs/ARCHITECTURE.md:38` **[П]** |
| Toolchain | `nightly-2026-08-31`, MSRV 1.89 | `rust-toolchain.toml`; `Cargo.toml:5` **[П]** |

Схема бида (`br schema issue`; `src/model/mod.rs:462`): `id, title, description, design, acceptance_criteria, prerequisites, notes,
status, priority(0–4), issue_type, assignee, owner, labels, dependencies, comments, due_at, defer_until, estimated_minutes,
external_ref, ephemeral, pinned, is_template, agent_context(JSON), close_reason, closed_by_session, source_repo…`. **Поля
`metadata` у бида нет** (есть только у зависимости). **[П]**

- Статусы: `open, in_progress, blocked, deferred, draft, closed, tombstone, pinned` + произвольная строка (`Status::Custom`,
  `src/model/mod.rs:60-74`); `br update --status review` принят. Типы: `task, bug, feature, epic, chore, docs, question` + custom
  (`:196-207`). **[П]**
- Зависимости: `blocks, parent-child, conditional-blocks, waits-for, related, discovered-from, replies-to, relates-to, duplicates,
  supersedes, caused-by` + custom (`:272-286`). **[П]**
- ID: `<prefix>-<base36 от SHA-256(title, desc, creator, ts, nonce)>`, длина 3–8, растёт с числом бидов; дети — иерархические
  `t1-znx.1` (`docs/ARCHITECTURE.md:653-690`; эксперимент). **[П]**
- Аудит: таблица `events` (монотонный `id`, `event_type, actor, old_value, new_value, agent_name, harness, model`) — только в БД, в
  JSONL не экспортируется; CLI отдаёт её лишь по одному биду (`br audit log <ID>`), глобальной ленты нет. **[П]**

### 2.2 Конкурентность (эксперименты `scripts/conc.py`, `crash.py`, `par.py`)

Механизм: все мутирующие команды берут эксклюзивный `flock` на `.beads/.write.lock`, таймаут 30 с
(`src/main.rs:340-348`, `src/sync/mod.rs:112`). «Concurrent writer support» движка не используется: lock введён из-за
«fsqlite's concurrent-write deadlock» (`src/main.rs:2136-2139`, issue #243). **[П]**

| Сценарий (br 0.7.6, одна БД) | Результат |
|---|---|
| 8 проц. × 10 create | 80/80, 13,7 оп/с, p50 430 мс, p95 1284 мс, 0 ошибок |
| 16 проц. × 12 create | 192/192 уникальных ID, 13,7 оп/с, p50 1064 мс, p95 1913 мс |
| claim-гонка: 16 проц. на 12 бидов | у каждого бида ровно 1 победитель; 180 отказов `rc=4 VALIDATION_FAILED "already assigned to agN"` |
| 16 проц. добавляют разные labels в один бид | 64 принято, 0 потеряно; остальные — отказ `labels: exceeds 64 labels` |
| 16 проц. `--append-notes` в один бид | 48/48 строк на месте |
| close + `ready` вперемешку, 16 проц. | 360 оп., 30 оп/с, 0 ошибок |
| итог | `jsonl lines == db issues`, `sync --status`: `dirty_count 0`, `doctor ok` |
| 8 проц. × 40 create, 65 `SIGKILL` в случайный момент | acked-но-пропало = 0; DB == JSONL; следующая запись и `doctor` — ok |
| 16 параллельных `ready --limit 5` (5k бидов) | 70 оп/с, p50 201 мс — читатели не сериализуются |

0.6.0 — та же корректность, на 20–40 % медленнее (`lab/br/conc-run1.log`). **[П]**

Ловушки, найденные по дороге **[П]**:

- **Claim различает агентов только по имени actor.** Без `--actor`/`BR_AGENT_NAME` assignee = `unknown`, и второй claim с тем же
  именем проходит (`rc=0`). Ключ `claim_exclusive=true` в 0.7.6 этого не изменил ни в одном из трёх написаний (код guard-а:
  `src/cli/commands/update.rs:1301-1325`). Оркестратор обязан выдавать уникальный actor на сессию.
- **Обнаружение `.beads` идёт вверх по дереву без потолка** (`src/config/mod.rs:303-369`), мимо границы git-репо, до `$HOME`.
  Агент в каталоге без `.beads` молча пишет в чужой трекер. Всегда задавать `BEADS_DIR`.
- Есть CAS: `br update --if-unchanged <updated_at>` → `rc=6 UPDATE_PRECONDITION_FAILED`. Есть `br close --suggest-next` →
  `{"closed":[…],"unblocked":[…]}`.
- «ready + claim» — две команды; гонка разрешается отказом claim и повтором.

**Worktree** (`lab/br/wt`): linked worktree получает из git только tracked-половину `.beads` (без БД); br в нём резолвится в `.beads`
основного checkout-а (`src/config/mod.rs:317-336`, `:388`), одинаково в 0.6.0 и 0.7.6. 8 claim-ов из двух worktree на один бид →
1 победитель. Побочный эффект: JSONL меняется только в основном checkout-е (`M .beads/issues.jsonl`), копии в worktree устаревают.
(У владельца `.beads/*` в глобальном gitignore, `~/.config/git/ignore:26` — JSONL в git не попадает вовсе.) **[П]**

**Независимые клоны** (`lab/br/clones`): A и B оба успешно claim-ят один бид; `git pull` → `CONFLICT … .beads/issues.jsonl`; с
маркерами конфликта `br ready` отвечает `CONFIG_ERROR: Merge conflict markers detected`; после взятия чужой версии `sync --flush-only`
→ `SYNC_CONFLICT`, `sync --merge` и `--merge --force` → `beads.base.jsonl is missing… choose --force-db or --force-jsonl`. Клон
стоит, пока не вмешается человек. Автор прямо пишет: `--claim` «is not a distributed lock across independent clones» и советует одного
writer-координатора (issue #594). **[П]**

### 2.3 Производительность (медиана, мс; `scripts/t.py`, генератор `scripts/gen.py`: 2 % эпиков, 60 % closed, 30 % с `blocks`)

| Команда | br 0.7.6 n=100 | 5k | 50k | br 0.6.0 5k | 50k¹ | bd emb. n=100 | bd emb. 5k |
|---|---|---|---|---|---|---|---|
| `ready --limit 10` | 13 | 58 | 425–488 | 326 | 1984 | 170 | 788 |
| `show` | 31 | 65 | 494–511 | 153 | 1565 | 276 | 1059 |
| `list --all --limit 0` | 13 | 60 | 534 | 133 | 1350 | 262 | 1529 |
| `create` | 65 | 155 | 1423–1508 | 395 | 3312 | 382 | 1420 |
| `create --no-auto-flush` | 39 | 77 | 743 | 216 | 1651 | — | — |
| `update --add-label` | 35 | 41 | 457 | 144 | 1380 | 220 | 801 |
| `update --claim` | 88 | 278 | 2740–4341 | 746 | 8586 | 253 | 758 |
| `close` | 81 | 311 | 3193–4401 | 986 | 10402 | 397 | 1165 |
| импорт JSONL | — | 6,6 с | 82 с | 22 с | 379 с | 1,6 с | 262 с |
| пик RSS процесса | 49–81 МБ | 80–191 МБ | 354–1263 МБ | ≤192 МБ | ≤1339 МБ | 170–180 МБ | 400–430 МБ |
| на диске | 0,5 МБ | 8,4 МБ | 82 МБ + 100 МБ `.br_recovery` | 8 МБ | 78 МБ | 13 МБ | 930 МБ |

¹ мерилось при фоновой нагрузке от других экспериментов — порядок величины. bd на 50k не мерил: импорт 5k занял 262 с.

- Флаги `--no-auto-import/--no-auto-flush/--allow-stale` на `show` при 50k ничего не меняют (≈0,95–1,0 с под нагрузкой) — стоимость
  в открытии БД, а не в sync. **[П]** Причина — движок читает/проверяет БД и WAL целиком при каждом open (issue #598: «Every br
  process re-reads the whole WAL at open»; RSS `show` 354 МБ на файле 82 МБ). **[В]**
- Auto-flush удваивает цену записи и линеен по размеру трекера (issue #559, подтверждено автором). **[П]**
- Архивации/компакции закрытых бидов нет (`compaction` в br — это VACUUM); есть только `br delete --hard`. **[П]**
- bd embedded: 8 параллельных `create` → 2,0 оп/с, p95 13,8 с; `ready` тоже сериализуется (3,7 оп/с). **[П]** bd в server-режиме не
  мерил (живой Dolt не трогал); по отчёту 06 сервер держит 1,37 ГБ RSS.

### 2.4 Функциональное сопоставление с нуждами оркестратора

| Нужно | br 0.7.6 | Комментарий |
|---|---|---|
| Эпик + дети, «готовые дети эпика X» | **есть** | `--parent`, `br ready --epic X` (рекурсивно), `epic status`, `epic close-eligible` **[П]** |
| `ready` с учётом `blocks` | **есть** | таблица `blocked_issues_cache`; эпик с открытыми детьми и ephemeral в `ready` не попадают **[П]** |
| assignee / атомарный claim | **есть** | в пределах одной БД; см. ловушку с actor |
| Кастомные статусы и workflow | **есть** | `.beads/policy.yaml`: переходы, required fields, gates (`br gate report`), WIP-лимиты (`capacity`), состав «ready» **[П]** |
| labels | **есть, ≤64 на бид** | фильтры `-l`, `--label-any`, `--exclude-label` |
| metadata key-value + запросы по ним | **нет** | обход: labels (лимит 64) или `agent_context` (JSON-строка, без запросов). Адаптер `gascity/contrib/beads-scripts/gc-beads-br` кодирует metadata в labels `metahex:…` — упрётся в 64 **[П]** |
| comments | **есть** | `comments add/list` |
| ephemeral / wisp | **частично** | `--ephemeral`: не экспортируется в JSONL, скрыт из `ready`; id с `-wisp-` тоже считаются локальными (`src/cli/commands/stats.rs:516`) |
| molecules / formulas / templates | **нет, намеренно** | весь Gastown-слой исключён: типы `gate, agent, role, molecule, rig, convoy`, команды `bd gate/agent/molecule/rig/convoy/hop/session` (`docs/porting/PLAN_TO_PORT…:404-442`); запрос на шаблоны отклонён (issue #575) **[П]** |
| defer / due | **есть** | `defer_until` скрывает из `ready`; `--overdue` |
| gate как «ждём внешнего события» (bd) | **нет** | `br gate` — это policy-гейты переходов, другое понятие |
| Несколько БД + маршрутизация по префиксу | **частично** | `.beads/routes.jsonl`: `show/update/close/dep…` по id уходят в чужую БД и берут её lock (`docs/CLI_REFERENCE.md:98-128`); `ready/list` чужие БД не агрегируют **[П]** |
| Cross-repo зависимости | **обход** | `dep add` между БД → отказ «resolve to different projects»; `--parent` на чужой эпик → `Issue not found`; `external:<proj>:<cap>` снимается не закрытием бида, а закрытым бидом с label `provides:<cap>` — перепроверено на 0.7.6, вывод прошлых сессий владельца верен **[П]** |
| Транзакция на несколько операций / атомарная заливка графа | **нет** | одна команда = одна транзакция; обход: `br dep import edges.jsonl`, пакетный `update id1 id2…` |
| Поиск | **есть** | `search`, сохранённые `query` |
| История / аудит | **частично** | `events` в БД, но CLI — только по одному биду; `br history` — это бэкапы JSONL |
| Уведомления / watch / hooks | **нет, отклонено** | исполняемые хуки отклонены как противоречащие non-invasive (issue #581); обход — §2.5 |
| JSON-вывод, коды возврата | **есть** | `--json` везде, ошибки — JSON-конверт в stdout, коды 0–8 по категориям (`br capabilities --json`); но «`br schema` is not a stable API» **[П]** |
| MCP-сервер | **не в релизных бинарниках** | `br serve` → `unrecognized subcommand`; нужен свой билд `--features mcp` на nightly (`Cargo.toml:219-220`) **[П]** |
| Rust-библиотека | **формально да** | crate `beads_rust` на crates.io, `pub mod storage/model/sync` (`src/lib.rs`), но API не стабилизирован и тянет nightly + rider **[П]** |
| AGENTS.md | **есть** | `br agents --add/--update`; `br robot-docs`; сам ничего не ставит (bd init в lab создал `.claude/ .codex/ .cursor/ .agents/ AGENTS.md CLAUDE.md` и hooks) **[П]** |

### 2.5 Как узнать «бид закрыт» без поллинга (`scripts/watch.py` + python `sqlite3`)

- `issues.jsonl` получает `IN_MOVED_TO` ровно один раз на каждую мутирующую команду, не получает на чтениях и на
  `--no-auto-flush`. `beads.db`/`-wal` получают `CLOSE_WRITE` и на чтениях — шумный сигнал. **[П]**
- Stock SQLite read-only на живой БД: `PRAGMA data_version` +1 на каждую запись br;
  `SELECT … FROM events WHERE id > ?` — 0,05 мс; 50 записей br при открытом читателе (в т. ч. с удержанной read-транзакцией) —
  0 отказов, `doctor ok`. Пока читатель подключён, WAL не чекпойнтится (3,8 МБ за 50 записей → 32 байта после отключения). **[П]**
- Это зависимость от внутренней схемы (версия 19, меняется часто) и от области с недавними багами (#521, #532, #544 — recovery
  под stock-reader-ом). Читателя открывать на событие и закрывать, не держать. **[В]**

### 2.6 Зрелость

- Старт 2026-01-15; 39 версий на crates.io; **25 релизов с 2026-08-05 по 2026-10-15**; после 0.7.6 релизов нет, а main уже не читает
  БД от 0.7.6 без обслуживания (issue #596, открыт). `gh release list`. **[П]**
- Issues: 457 всего, 22 открытых; в заголовках: «data loss/silent/lost/drops» — 36 (19 с августа), «deadlock/lock/hang/wedge» — 62,
  «corrupt» — 24 (`lab/br/issues-all.json`). **[П]**
- **Август 2026:** v0.5.3 портил мигрированные БД при многопроцессной записи (#457 page aliasing, #460 freelist, #461 `comments add`
  уничтожал тела комментариев в JSONL); причина — checkpoint FrankenSQLite не учитывал снимки чужих процессов. Лечение — checkpoint
  только когда процесс «provable sole opener». v0.5.4 вышел на stock SQLite (rusqlite) и через час заменён: «the operator rejected it…
  That rejection is the standing decision» (`docs/reliability/ENGINE_OPERATING_MODEL.md:27-75`). Апрель 2026: то же у стороннего
  демона-оркестратора (issue #241). **[П]**
- Монолиты `sqlite.rs` (47k строк), `sync/mod.rs` (37k), `doctor.rs` (30k) «grew around the 2026-08 corruption incidents»
  (`docs/ARCHITECTURE.md:977`); `src/` вырос с 248k до 327k строк за два месяца. **[П]**
- Стратегия безопасности — «refuse instead of lose»: при сомнении команды отказывают до ручного `br doctor …`. Регрессии такого рода:
  0.7.0→0.7.1 (#521 «wedge on a stale WAL index»), 0.7.5→0.7.6 (#569 «blocks every command, reads included»). Для автономного
  оркестратора это риск простоя, а не потери. **[П]/[В]**
- Contributions: «I do not accept outside contributions for any of my projects» (`README.md:1541`); внешних PR 63: 61 closed,
  1 merged, 1 open. В 61 коммите shallow-клона — один автор (+dependabot). **[П]**
- Зависимости автора: `fsqlite` (50 версий за 9 мес., 512 issues), `asupersync`, `rich_rust`, `tru`, опц. `fastmcp-rust` — всем <1 года. **[П]**
- Тестов много: ~7 000 `#[test]`, model-based и multi-process linearizability, CI-гейты на recovery. **[П]**
- Роадмапа нет; запросы на хуки (#581), шаблоны (#575), демон (#180) закрыты без реализации. **[П]**

### 2.7 Лицензия

`LICENSE`: «MIT License (with OpenAI/Anthropic Rider)». Rider: права не предоставляются OpenAI, Anthropic, их аффилиатам и любому,
кто действует «on behalf of, for the benefit of, or under the direction of» них; запрещено «provide… make available, or otherwise
permit access to the Software or any derivative work… to or for any Restricted Party»; «use» включает executing, testing,
analyzing, indexing, включение в datasets/eval; rider обязан сохраняться во всех копиях и производных; нарушение автоматически
прекращает лицензию. **[П]** Тот же текст в `src/beads_viewer/LICENSE`; crates.io помечает `fsqlite`, `beads_rust`, `tru`,
`fastmcp-rust` как `non-standard`, `asupersync` — `LicenseRef-MIT-OpenAI-Anthropic-Rider`. **[П]**

Следствия **[В]** (не юридическая консультация):

- Вызывать установленный отдельно бинарник `br` из своего инструмента можно; свой код при этом не производная работа.
- Вендорить/линковать crate/форкать — rider переходит на ваш дистрибутив: его нельзя давать сотрудникам и подрядчикам OpenAI/Anthropic.
- Формулировки широкие и не проверены судом: буквально неясно, считается ли «making available… for a Restricted Party» работа
  Claude Code/Codex с исходниками br. Автор сам ведёт проект через Claude и Codex (`README.md:1541`), то есть намерение явно не в
  этом, но текст это не закрепляет.

### 2.8 Философия br и что она значит для оркестратора

«Non-Invasive: No daemons, no git hooks, no automatic commits» (`docs/ARCHITECTURE.md:26-32`); проект — «stable snapshot» classic
beads, сознательно разошедшийся с Gas Town (`README.md:37`). Следствия **[В]**: событийность, фоновая доставка, реакция на
изменения — целиком на оркестраторе; br — пассивный файл с CLI. Общий стор на несколько worktree работает из коробки, на несколько
репо/машин — нет. Заявления «нет демона» и «нет git-операций» подтверждены; «sync явный» — нет, по умолчанию он автоматический.

## 3. Вердикты

- **H5 — частично подтверждена.** br+bv заменяют bd+Dolt как менеджер *рабочих задач* при условиях: одна БД на город; ≤ ~5k
  бидов (регулярная чистка закрытых); уникальный actor; явный `BEADS_DIR`; состояние оркестратора — не в br. Как замена *всего*,
  что Gas City кладёт в bd (сессии, order-tracking, шаги формул, mail; `Store.ListByMetadata/SetMetadata/Tx` в
  `gascity/internal/beads`), — опровергнута: нет metadata, транзакций, событий, а при ~35k строк, как в живом городе (отчёт 06), каждая
  запись стоила бы 1–3 с (**[В]**, интерполяция между 5k и 50k).
- **H6 (вклад).** Со стороны стора подтверждается: молекул/формул в br нет принципиально, а «эпик + дети + `ready --epic`» есть
  нативно и работает.
- **H8 (вклад).** br событий не даёт; событийная модель возможна только поверх inotify + хвоста `events` (§2.5) либо если все записи
  идут через оркестратор.
- **H11 (вклад).** Отказ от Dolt снимает сервер на 1,4 ГБ, но обменивает его на молодой нестандартный движок с одним мейнтейнером.
  Это аргумент держать трекер за интерфейсом, а не против переписывания.

## 4. Следствия для дизайна и варианты

| Вариант | Плюсы | Минусы | Оценка |
|---|---|---|---|
| **A. br — единственное хранилище** (состояние в labels/`agent_context`) | один стор; bv видит всё | нет metadata и запросов по ней; 64 labels; нет транзакций; O(N) на команду при тысячах служебных бидов; последовательная запись ~14–24 оп/с; служебный шум в `ready`/bv | **нет** |
| **B. br — только рабочие биды; сессии, прогоны формул, события, mail — в своей SQLite** | br делает то, что умеет; агенты и человек работают привычным `br`/`bv`; БД маленькая → 30–150 мс на команду; нет демона | два стора, связь по bead-id; события добывать самим (§2.5); риски движка/лицензии/мейнтейнера остаются; нужен pin версии и канареечное обновление | **рекомендую** |
| **C. Свой стор (SQLite) для всего + экспорт/импорт beads-JSONL для bv** | одна транзакционная БД, свои события, metadata, нет чужого движка и rider-а в критическом пути; по отчёту 06 в gc уже есть SQLite-движок бидов на 5 таблиц | надо написать `ready`/deps/claim и CLI для агентов; bv по умолчанию читает `beads.db`, а не JSONL — проверить режим JSONL; теряется готовый `br` в промптах | **запасной выход / цель, если B начнёт мешать** |
| **D1. bd embedded (без сервера)** | совместимость с Gas City | 2 create/с при 8 писателях, 0,8–1,5 с на команду при 5k, 930 МБ на 5k бидов, `bd init` ставит hooks и конфиги харнессов | **нет** |
| **D2. bd + Dolt-сервер** | metadata, транзакции, уже работает | ровно то, от чего хотим уйти (1,37 ГБ RSS) | статус-кво |
| **D3. Внешние трекеры (GitHub Issues, Linear)** | UI, уведомления | сеть, лимиты, нет локального `ready`/claim | не для ядра **[НП]** |

Рекомендуемая форма B **[В]**:

1. **Одна `.beads` на город**, rig — label (`rig:api`) или slug в id, а не отдельная БД: эпик над несколькими ригами возможен только
   так. Worktree ригов видят её через `BEADS_DIR`, не через автообнаружение.
2. **Все обращения — через адаптер** на ~10 операций (create, show, update, claim, close, ready(epic), list, dep add, comment,
   subscribe). Он задаёт `BEADS_DIR`, `--actor <session-id>`, `--json`, `--lock-timeout`, разбирает коды 0–8 и повторяет `rc=4` на claim.
3. **События:** inotify на `.beads/` → короткое read-only чтение `events WHERE id > last` → публикация в шину оркестратора. Запасной
   путь без чтения БД: diff двух снимков `br list --all --json`.
4. **Гигиена:** `sync.auto_flush=true` оставить (JSONL — единственный бэкап, из него br пересобирает БД); регулярный
   `br delete --hard` закрытых; `ephemeral` для служебного; мониторить `br doctor --json` и размер `.br_recovery/`.
5. **Версии:** pin, обновление только после прогона `conc.py`/`crash.py` на копии; не обновляться в первую неделю после релиза.
6. **Не использовать:** отдельные клоны с собственными БД, git-merge JSONL, `routes.jsonl` для разбиения города, crate-линковку,
   `claim_exclusive`.

Граница применимости: если рабочих бидов станут десятки тысяч или понадобится >20 записей/с — B перестаёт работать, переход на C.

## 5. Открытые вопросы и непроверенное

### 5.1 Инцидент

`git add -A` в lab-репо ничего не закоммитил (глобальный gitignore владельца `.beads/*`), поэтому в worktree не оказалось `.beads`,
и `br create` (0.7.6) ушёл вверх до `/home/ubuntu/.beads`. Изменено: одна строка в `issues.jsonl` (`br-mq2`), `beads.db` и его
sidecar-файлы, `last-touched`; созданы 2 пустых lock-файла. **[П]** Схема, судя по всему, не мигрировала: `user_version` = 19, как у
свежей БД 0.6.0; копия каталога читается br 0.6.0 (`lab/br/incident-homebeads-copy`). **[В]** `shared-server/` не тронут. Ещё два
временных файла я создал и удалил в `/tmp/claude-1000/`. После этого все эксперименты шли с изолированными `HOME`/`XDG_*` и
«потолком» `lab/br/.beads` (`scripts/env.sh`).

### 5.2 Не проверено

- **main/HEAD не собирался** (нет cargo): все эксперименты — на релизах 0.7.6/0.6.0, цитаты кода — из HEAD. **[НП]**
- Долгоживущий процесс (`br serve` MCP, crate в демоне): по #598 и §2.5 WAL не чекпойнтится, пока есть второй открывший. **[НП]**
- Почему `claim_exclusive` не сработал в 0.7.6 — баг или моя ошибка в ключе; константу лимита 64 labels в не-тестовом коде не нашёл
  (лимит наблюдался экспериментально). **[НП]**
- macOS/Windows, сетевые ФС, реальные описания по 10–25 КБ (мои — 400 Б), сотни комментариев на бид. **[НП]**
- Сколько *рабочих* бидов копится у владельца за месяц — от этого зависит, как часто чистить (отчёт 06: ~280 рабочих из ~35k строк).
- Юридическая трактовка rider-а. **[НП]**
- bv в режиме чтения JSONL без `beads.db` (нужно для варианта C) — к отчёту 12.
