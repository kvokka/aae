# 06 — Слой хранения Gas City: beads (bd) + Dolt, требования к хранилищу и цена отказа от Dolt

Дата: 2026-11-10. Код: `/home/ubuntu/proj/work/gascity` @ f1f75f53c (пути ниже — относительно него). Живой город:
`/home/ubuntu/proj/share/gc` (бинарь gc собран с a86d05bd, bd 1.3.2, dolt 2.6.0). Замеры в живом городе — только чтением:
`SELECT` через `dolt --host 127.0.0.1 --port 47723 ... sql -q`, `bd --readonly ...`, `command gc status/storage status`, `ps`, `du`.
Скрипты и выгрузки: `lab/06-storage/`.

Метки: **[П]** = ПРОВЕРЕНО (сам видел), **[В]** = ВЫВОД, **[НП]** = НЕ ПРОВЕРЕНО.

## 1. Выводы за 60 секунд

- **В живом городе Dolt на 99 % хранит не задачи, а журнал cron-а.** Из ~35 450 строк только ~280 (0,8 %) — настоящая работа
  (task/bug/feature/epic/convoy). 34 422 строки — `order-tracking` wisps («ордер N отработал»), ~5 000 новых в сутки; ещё 665 — шаги и
  control-биды формул. **[П]**
- **Цена этого сейчас:** `dolt sql-server` RSS 1,37 ГБ (uptime 14 дней), 441 МБ на диске + 1,4 ГБ бэкапа при ~8 МБ полезных данных;
  ~40 SELECT/с и ~11 новых соединений/с в спокойном городе; 17 МБ лога, где 99 % строк — `nothing to commit`. **[П]**
- **gc говорит с хранилищем тремя путями сразу:** линкованная Go-библиотека `steveyegge/beads` → SQL к Dolt (`NativeDoltStore`, так
  работает контроллер владельца), subprocess `bd` (`BdStore`, fallback + все агенты из промптов), гибрид `ProxiedStore`. Всего 8 backend-ов
  `Store` и ~23 обёртки. **[П]**
- **Версионирование Dolt в продуктовой логике не используется.** Ветка одна (`main`), `DOLT_BRANCH/CHECKOUT/REBASE` — 0 файлов Go,
  time-travel — только как совет оператору. Реально нужны от Dolt: SQL-сервер для многих писателей, `DOLT_COMMIT` как граница
  транзакции, push на remote как бэкап. **[П]**
- **Upstream сам уже уходит от «всё в bd/Dolt».** `internal/coordclass` делит биды на work и 5 инфраструктурных классов (graph, sessions,
  messaging, orders, nudges), а `[storage]` переносит инфраструктуру в локальный pure-Go SQLite (`sqlite-beads`). В gc уже есть свой
  SQLite-движок бидов на 5 таблиц и multi-process WAL. **[П]**
- **Эта миграция «на ходу» дорогая:** `storebinding` + `storeref` = 33,7k строк, аудит нашёл 32 места, где инфра-бид читается/пишется
  не через тот store; последние 100 fix-коммитов про dolt/bd/store уложились в 3,5 недели. **[П]**
- **Требования gc к трекеру задач малы; требования к «state оркестратора» велики.** Ядро: CRUD, 3 статуса, типы-строки, labels,
  assignee, parent, deps `blocks`, ready, comments. Тяжёлое: metadata key-value (214 объявленных `gc.*` ключей, 1 895 мест чтения,
  запросы `ready --metadata-field`), CAS по revision, атомарный claim, атомарная заливка графа. **[П]**
- **br 0.6.0 не имеет metadata вообще** (0 упоминаний в `create/update/ready/list --help`), а готовый адаптер `contrib/beads-scripts/gc-beads-br`
  кодирует metadata в labels `metahex:<hex>:<hex>`. Значит «положить всё состояние gc в br» — плохая идея; «положить в br только работу» —
  хорошая. **[П]**
- **H5: частично подтверждена.** br+bv заменяют bd+Dolt как *трекер задач*; как *универсальный substrate* (сессии, ордера, run-state
  формул, nudges) — нет, и не должны: это состояние оркестратора, ему место в собственной SQLite.
- **H7 (вклад): ~145k строк non-test Go (~16 % от 932k) существуют только ради bd/Dolt/split-специфики**, плюс 17k строк bash/toml в
  паке `bd` и ~357k строк тестов. 23 % всех fix-коммитов репозитория — про dolt/bd/beads/store. **[П] цифры, [В] атрибуция**

## 2. Факты

### 2.1. Как gc говорит с beads

| Путь | Что это | Где | Строк (non-test) |
|---|---|---|---|
| `NativeDoltStore` | В gc влинкована `github.com/steveyegge/beads v1.3.4` (`go.mod:27`), она открывает SQL-соединения к `dolt sql-server` | `internal/beads/native_dolt_store.go:16-17` | 8 879 |
| `BdStore` | subprocess `bd <verb> --json` на операцию, таймаут 120 с | `internal/beads/bdstore.go:37`, `:1935` | 6 690 |
| `ProxiedStore` + `proxyendpoint` | чтение native через прокси bd, запись через bd CLI (дефолт нового `gc init`: `bd db-proxy-child` + свой dolt на scope) | `engdocs/design/beads-proxied-local-default.md:14-17` | 7 217 |
| `DoltliteReadStore` | ещё один backend bd | `internal/beads/doltlite_*.go` | 2 054 |
| `SQLiteStore` | собственный pure-Go SQLite-движок бидов gc (modernc), без bd и Dolt | `internal/beads/sqlite_store*.go` | 5 079 |
| `FileStore` / `MemStore` | JSON-файл / память | `internal/beads/filestore.go`, `memstore.go` | 2 304 |
| `exec.Store` | пользовательский скрипт, ~14 операций | `internal/beads/exec/exec.go`, `docs/reference/exec-beads-provider.md:84-97` | 746 |
| `CachingStore` | write-through кэш + reconcile-сканер поверх любого backend | `internal/beads/caching_store*.go` | 9 772 |

**[П]** `command gc status --json` в живом городе: `"beads_store": "NativeDoltStore", "native_store_eligible": true`. Агенты же зовут `bd`
напрямую из промптов (в паках владельца: `bd show` 86, `bd update` 44, `bd comments` 33, `bd gate` 31, `bd close` 22, `bd list` 18,
`bd create` 12, `bd dep` 8 — grep по `gascity-packs/packs`).

**[П]** Кэш нужен именно потому, что агенты пишут мимо контроллера: «Writes that emit no gc event (agents running bd directly, humans,
raw SQL) reach the cache when the background reconciler's scan notices them… The cache is therefore always potentially stale»
(`internal/beads/caching_store.go:19-28`). 174 коммита трогали `caching_store*`.

**[П]** Методов `Ready(...)` в non-test коде — 31 тип (8 backend-ов + обёртки: federated legs, class stores, policy store, fenced store…).
Опциональных capability-интерфейсов в `internal/beads` — около 50 (`ConditionalWriter`, `GuardedBatcher`, `GraphApplyStore`,
`MetadataCASWriter`, `DoltTagger`, …).

**Что такое split-store / residency / storebinding / storeref / typed store boundary.** **[П]** `internal/coordclass/class.go:4-15`:
«Everything in Gas City currently persists through one beads.Store, backed by bd/Dolt. The "work vs infrastructure split" initiative
carves that one store into persistence classes… INFRASTRUCTURE data — everything that merely uses beads as a convenient store».
`storebinding` (29 889 строк, с 2026-08-06) — front doors классов, миграция, манифесты, fences; `storeref` (3 825) — «в каком store
физически лежит id»; `splittest` (4 825) — строгие проверки маршрутизации. Конфиг — `examples/storage/city.toml`: либо все 6 классов на
`work`, либо work в bd, а 5 инфра-классов в один `sqlite-beads` (`.gc/store/graph/beads.sqlite`); миграция — ручная `gc storage migrate
--from-work`. Город владельца не разделён: `command gc storage status` → `[storage] configured: false`, а v2-reconciler требует split
(комментарий в `city.toml` владельца, `session_reconciler = "legacy"`; событие `storage.binding.not_configured` ×21 в `events.jsonl`).

**[П]** Цена перехода: `engdocs/contributors/infra-class-store-routing-audit.md:25` — «32 MISROUTED, 4 AMBIGUOUS, 11 CORRECT» (классы
багов: stranded-write, silent-empty, silent-stale; например «live agents' work released out from under them»).

### 2.2. Что gc хранит в бидах (живой город, 2026-11-10)

Всего: `issues` (durable) ≈ 970 строк в 5 БД (hq 339, mpa 415, mpf 112, gcp ~100, dot 4; gcp рос во время замера), `wisps` 34 483
(все в hq). **[П]** (`SELECT issue_type,status,COUNT(*) ... GROUP BY`).

| Что | Как закодировано | Объём в городе | Время жизни | Класс gc |
|---|---|---|---|---|
| Работа | type task/bug/feature/epic, без workflow-metadata | ~265 | навсегда | work |
| Convoy | type `convoy`, deps `tracks`; синтетические — `gc.synthetic*` | 37 (23 синтетических) | навсегда | work |
| Шаги и control-биды формул | type task/spec/gate + `gc.kind` ∈ scope-check (219), wisp (132), scope (70), workflow (42), workflow-finalize (42), ralph (39), spec (39), cleanup, drain; `gc.root_bead_id`, `gc.step_ref`, `gc.outcome`… | 665 (69 % durable) | навсегда (durable!) | graph |
| Order run history | wisp, labels `order-tracking`, `order-run:<name>`, `exec`; metadata только `close_reason` | **34 422 (99,8 % wisps), ~5 000/сутки** | 7 дней (`reaper.sh:114` `PURGE_AGE=168h`) | orders |
| Сессии | wisp type `session`, label `gc:session`, **в среднем 60 metadata-ключей, максимум 83** (`state`, `generation`, `runtime_lease_*`, `wake_request`, `current_claim_bead_id`…) | 56 (2 открытых) | 30 дней после закрытия | sessions |
| Mail | wisp type `message` | 4 | до архива | messaging |
| Nudges | основная очередь — файл под flock (`internal/nudgequeue`: `StatePath`, `LockPath`); в бидах — «тени» type `chore` + `gc:nudge` | 0 | — | nudges |
| Op-journal | type `op`, labels `gc:op`/`gc:fact`/`gc:rt` (`internal/opjournal`, 13 400 строк, с 2026-10-13) | 0 | — | sessions |
| Human gates | type `gate` (пак владельца, `bd gate`) | 12 | навсегда | work/sessions |
| agent/role/rig | «NO Go creation site» (`coordclass/class.go:38-41`) | 0 | — | — |
| События | **не в бидах**: `.gc/events.jsonl` 87 МБ, 252 747 событий с 19.10; 77 % — `order.fired`/`order.completed` | — | без ротации | — |

**[П]** Metadata: в durable-бидах 275 различных ключей, в среднем 16,7 на бид; в коде объявлено 214 `gc.*` ключей
(`internal/beadmeta/keys.go`), 1 895 чтений `Metadata[...]`, 302 записи `SetMetadata*`, 141 запрос по metadata в non-test Go.

**[П]** 28 % order-запусков за неделю — ордера, обслуживающие само хранилище и гигиену бидов (beads-health, mol-dog-doctor,
order-tracking-sweep, dolt-remotes-patrol, reaper, wisp-compact, compactor, backup, phantom/stale-db, orphan-sweep, cross-rig-deps);
строго Dolt/bd — 17,7 %. Каждый такой запуск сам пишет wisp в Dolt.

### 2.3. Что gc требует от хранилища

| Возможность | Использование в gc | Категория |
|---|---|---|
| CRUD, title/description/priority/assignee/parent, close reason, reopen, delete | `Store` (`internal/beads/beads.go:1069-1222`) | ядро любого трекера |
| Статусы | наружу только open/in_progress/closed; bd-шные blocked/deferred/review/testing/hooked/pinned сворачиваются в open (`beads.go:249-252`) | ядро |
| Типы | свободная строка + 15 custom (`types.custom` в `.beads/config.yaml`) | ядро + мелкое расширение |
| Labels, точное совпадение, префикс | маршрутизация, `order-run:*`, `gc:session` | ядро |
| Deps | `blocks` (127 литералов), `parent-child` (53), `tracks` (25), `waits-for` (15), `conditional-blocks` (14), `relates-to` (7); в живых данных только blocks 1 070, tracks 690, parent-child 601, discovered-from 10, related 3 | blocks/parent — ядро; остальное — bd/Gas Town |
| `ready` | gc считает сам: 12 исключённых типов (`beads.go:819-832`), 3 блокирующих типа рёбер, вето `gc.work_outcome=blocked` (`:858-884`), `defer_until` | ядро + логика оркестратора |
| **Metadata KV + запрос по key=value** | `bd ready --metadata-field "gc.routed_to=$target"` — основной work query (`internal/config/workquery.go:301`) | **расширение; в br отсутствует** |
| Атомарный claim | `bd update --claim` (`bdstore.go:1935`), release/transfer по текущему владельцу, `claim_fence` | расширение (в br `--claim` есть) |
| CAS по revision, guarded batch, metadata CAS | `UpdateIfMatch/CloseIfMatch`, `GuardedBatcher` (≤100 строк) | специфика, нужна оркестратору, не трекеру |
| Атомарная заливка графа | `bd create --graph` / `ApplyGraphPlan` | специфика молекул |
| Ephemeral-ярус (wisps) + purge, `no_history` | отдельные таблицы `wisps*` в Dolt | специфика bd/Dolt |
| Несколько БД с префиксами | `routes.jsonl`, `gc bd` маршрутизирует по id (`cmd/gc/cmd_bd_by_id.go`, 2 304 строки) | специфика |
| Cross-store deps | **не поддерживаются**: «cross-store dependencies are not supported» (`bdstore.go:3713-3727`); parent — слабая строковая ссылка | — |
| Comments | `bd comments` в паках владельца (69 строк в hq) | ядро |
| Аудит/события bd | `bd_events_journal` — 0 строк в городе; кэш живёт на polling | не используется |
| `bd mol`, slots, federation, bd-hooks | `bd mol` в Go не вызывается; `federation_peers` 0 строк; путь bd hook → gc event удалён (`caching_store.go:23-25`) | не используется |

**[П]** Для инфра-классов upstream уже описал *доменные* интерфейсы, а не «трекер»: `SessionsStore` (~60 методов: `SetState`, `Sleep`,
`WakeSession`, `CreateWait`…), `OrdersStore` (23: `LastRun`, `Cursor`, `CreateRun`…), `NudgeQueue` (16), `GraphStore` (30)
(`internal/storebinding/storebinding.go`, перечень — `lifecycle_events.go:189-204`).

### 2.4. Что даёт Dolt и чего он стоит

**Используется ли версионирование [П]:** `dolt_branches` в hq — 1 строка (`main`); `dolt_tags` — 0; коммитов 1 968 (hq), 2 703 (mpa).
grep по non-test Go: `DOLT_BRANCH` 0 файлов, `DOLT_CHECKOUT` 0, `DOLT_REBASE` 0, `dolt_history` 0, `DOLT_PUSH/PULL/FETCH` 0 (только в
bash пака), `DOLT_MERGE` — 2 файла (комментарии), `DOLT_TAG` — 1 (снапшот перед `gc storage migrate`), `DOLT_HASHOF('HEAD')` — детектор
изменений. Remotes у владельца есть (`origin`/`offsite` → `git+ssh://git@github.com/kvokka/gc`) — это off-box бэкап через ордер
`dolt-remotes-patrol`, не федерация.

**Цена в живом городе [П]:**

| Метрика | Значение | Источник |
|---|---|---|
| RSS `dolt sql-server` | 1 366 388 КБ, uptime 14д 6ч | `ps -eo pid,rss,etime,args` |
| RSS `gc supervisor` | 365 724 КБ | `ps` |
| Диск `.beads/dolt` | 441 МБ (hq 270, mpa 64, mpf 24, gcp 13) | `du -sh` |
| Диск `.dolt-backup` | 1,4 ГБ | `du -sh` |
| Полезные данные | issues ≈ 2,2 МБ + wisps ≈ 5,8 МБ | `SUM(LENGTH(...))` |
| Нагрузка за 60 с | +2 432 SELECT, +3 711 statements, +665 соединений, +49 insert | `SHOW GLOBAL STATUS` ×2 |
| Накоплено за uptime | 41,1 млн SELECT, 9,5 млн соединений, 0,93 млн insert | там же |
| `dolt.log` | 17 МБ, 60 405 из 60 874 строк — `nothing to commit`; 56 `serialization failure`; 6 `dangling references requested during GC` | `grep -c` |
| Бинарники | gc 179 МБ, bd 144 МБ, dolt 117 МБ; br 21 МБ, bv 42 МБ | `ls -l` |

**Обслуживание [П]:** пак `examples/bd` — 17 124 строк не-Go (`gc-beads-bd.sh` 5 582, `dolt/commands/compact/run.sh` 4 619,
`reaper.sh` >1 400) и 7 ордеров (dolt-health 30s, doctor 5m, remotes-patrol 15m, phantom-db 1h, compactor 2h с таймаутом 24h, backup 6h,
stale-db cron). Для stale-db есть отдельный LLM-агент `dog` (`examples/bd/dolt/agents/dog`). В Go: 82 файла с `dolt` в имени = 24 747
строк (+62 611 строк тестов), `cmd/gc/bd_env.go` 2 690, `beads_provider_lifecycle.go` 4 251.

**Инциденты [П]:** 7 768 коммитов (22.02–10.11.2026); `dolt` упомянут в 1 428 (18 %); из 3 750 fix-коммитов 874 (23 %) имеют
dolt/bd/beads/store/storage в теме, 260 — именно `dolt`. Темы последних: порты (42), compact (26), health/probe, start/stop, lock,
cleanup, backup, recover. Последние 100 таких fix-ов — с 16.10 по 09.11. Upstream-диагноз: «too many competing authorities… not one bug;
it is a contract drift problem» (`engdocs/design/beads-dolt-contract-redesign.md:15-21`).

**Опыт владельца (deja recall «dolt», 503 сессии) [П как записи истории]:** `beads-health` падал каждые 30 с («healthy but failed to
publish managed dolt runtime state»), два `dolt sql-server` с удалёнными `LOCK` (сессия d452e459); после тестов оставалось больше десятка
dolt по 200–750 МБ и до 70 % CPU (33885548); в `city.toml` владельца: «Every order run writes an order-tracking wisp into Dolt, so 30s
orders keep the hq noms journal growing». В skill-справочнике (`worktrees/gc-home-split/.claude/skills/gascity/references/operations.md:91-111`)
— 9 строк таблицы неисправностей только про Dolt/bd.

### 2.5. Производительность

- **[П]** `bd --readonly` в живом городе: `show` 64–97 мс, `ready --limit 5` 64–68 мс (mpa) и 192–201 мс (hq), `list` 71–116 мс.
  Сам вызов не медленный; дорога частота.
- **[П]** Комментарии в коде: `bd context` «~0.3-0.4 s» на scope (`internal/beads/contract/preflight_checker.go:117`); wisp-GC через bd
  «about 0.75 s each» (`cmd/gc/wisp_gc_work_lane.go:25`); managed dolt «~12s per call» (`cmd/gc/api_state.go:247`); заливка молекулы
  67 узлов/~100 рёбер не уложилась в 120 с — «2026-07-17 code red» (`internal/beads/native_dolt_store.go:66-76`).
- **[П]** `engdocs/design/idle-controller-call-rate.md:22-27`: простаивающий город — ~7 `bd`/с → ~463 `Com_select`/с; `:95-100`: один
  `gc order check` = 43 subprocess `bd` на CLI-fallback против 1 на native store; насыщение «rate-bound, not size-bound».
- **[В]** Кэши/батчи есть везде (`CachingStore`, `GetExactBatch`, `DepListBatch`, `ChildrenBatchLister`, ready-projection), и все они —
  следствие двух решений: частый опрос по тику и «агенты пишут в store мимо контроллера».

## 3. Вердикты

**H5 (br + bv заменяют bd + Dolt) — ЧАСТИЧНО ПОДТВЕРЖДЕНА.**
- За: Dolt-специфика продуктом не используется; настоящих задач сотни; gc сам имеет exec-провайдер, адаптер `gc-beads-br` и
  conformance-тест под br (`internal/beads/exec/br_test.go`); собственный SQLite-движок gc — 5 таблиц (`kv, beads, labels, metadata,
  deps`, `internal/beads/sqlite_store_schema.go:32-106`) с multi-process WAL и `BEGIN IMMEDIATE` (`sqlite_store.go:227-231`, `:424-433`).
- Против: gc использует bd как универсальную БД состояния. br 0.6.0 не даёт metadata KV и запросов по ней **[П]**, CAS по revision и
  атомарной заливки графа **[НП — по br смотрел только `--help`]**. Адаптер `gc-beads-br` (353 строки bash) кодирует metadata в labels
  и не умеет `ready --include-ephemeral` (`contrib/beads-scripts/gc-beads-br:14-15`, `:313`).
- Итог: br заменяет bd в роли трекера работы. Остальные 99 % строк надо не переносить в br, а вынести в state оркестратора.

**H7 (кода много больше, чем используется) — ПОДТВЕРЖДЕНА по слою хранения.**
- В файлах/пакетах с «хранилищным» именем: `internal/` ≈ 110k (beads 71 256, storebinding 29 889, storeref 3 825, bdflags 1 985,
  migrate 1 076, coordclass 867, dolt* 870…) + `cmd/gc` 77 303 в 170 файлах ≈ 188k non-test Go = 20 % от 932k. **[П]**
- Только ради bd/Dolt/split-специфики (за вычетом нейтрального: SQLite/Mem/File/exec, ядро `beads.go`, conformance) ≈ 145k (~16 %).
  **[В]**, метод: beads −19k нейтрального + storebinding + storeref + 54,9k файлов `cmd/gc` с dolt/bd/proxied/split/storage в имени +
  мелкие пакеты.
- Используется владельцем: 1 из 8 backend-ов, 1 из ~5 топологий, 0 из split-машинерии.

**Попутно:** H6 — 69 % durable-бидов города — шаги и control-биды формул: молекулы утраивают трекер. H8 — 77 % событий и 97 % строк
хранилища порождены тиковыми ордерами.

## 4. Следствия для дизайна

### 4.1. Разбиение «трекер задач» vs «state оркестратора»

| Данные | Куда | Почему |
|---|---|---|
| task/bug/feature/epic, deps blocks/parent-child, labels, priority, comments, close reason | **Трекер (br), JSONL в git рига** | человекочитаемо, нужно bv и людям; сотни строк |
| «Convoy» | трекер: эпик + дети | уже так устроено (`tracks`/parent) |
| Маршрут, claim, попытки, outcome, привязка к сессии (`gc.routed_to`, `gc.session_id`, `gc.attempt`, `gc.outcome`…) | **state оркестратора**: таблица `assignment(bead_id, …)` | меняется часто, не нужно в git, требует CAS |
| Run-state формул (шаги, retry, scope) | state: `run`, `run_step` | 69 % строк трекера сегодня; в git не нужен |
| Сессии | state: типизированная таблица `session` | сейчас 60–83 строковых ключа на бид |
| Ордера | state: `order_state(name, last_run_at, cursor)` + кольцевой журнал запусков | сейчас 5 000 бидов/сутки ради «когда запускался» |
| Nudges, waits/таймеры, mail агент↔агент | state: очереди | транзиентны; mail человеку можно дублировать в трекер |
| События | append-only лог с ротацией | уже файл; сейчас без ротации (87 МБ за 3 недели) |

### 4.2. Варианты

1. **«Чистый трекер + state в оркестраторе» (рекомендую).** br хранит только работу; оркестратор — единственный писатель
   координации и держит свою SQLite (WAL). Диспетчеризация push-ом: оркестратор сам выбирает ready-бид и отдаёт сессии, поэтому гонка
   «N агентов на `ready --claim`» исчезает, а metadata в трекере не нужна. Агентам остаётся узкий CLI: show/update/close/create/dep/comments.
2. **«Трекер с лёгкой разметкой».** То же, но маршрут пишется label-ом (`route:<agent>`) ради видимости в bv. Дёшево, но label — не CAS.
3. **«Всё в трекере» (как gc сейчас).** Потребует от br metadata KV, query по ней, CAS, ephemeral-ярус с purge — то есть форка br или
   собственного движка. Воспроизводит нынешнюю проблему: 99 % трекера — не задачи.
4. **«Без br: свой SQLite-трекер на 5 таблиц + экспорт JSONL для bv».** Схема уже есть в gc. Плюс — один движок на всё; минус —
   теряется экосистема br, а bv читает формат br/bd **[НП]**.

### 4.3. Что покрывается и что нет при «SQLite + JSONL, без демона»

- Тривиально: CRUD, статусы, типы, labels, parent, `blocks`, ready, comments, defer, claim (`br update --claim` есть **[П]**).
- Нужны расширения или обход: metadata KV, типы рёбер `tracks/waits-for/conditional-blocks`, 15 custom-типов **[НП]**.
- Вынести: всё из §4.1, кроме первых двух строк.

### 4.4. Риски

- **Worktree.** Сейчас все worktree рига смотрят в один сервер. У br `.beads/` лежит в checkout: каждый worktree получит свою копию
  JSONL и свою SQLite. Нужен один канонический путь БД на риг (env/флаг) и запрет агентам коммитить `.beads/` из рабочих веток. **[В]**
- **Конкурентные писатели.** SQLite WAL + `busy_timeout` выдерживает контроллер и CLI-процессы — gc так уже работает
  (`sqlite_store.go:424-433`), но с retry на `SQLITE_BUSY` (`:170-200`). В варианте 1 писатель координации один. **[П]/[В]**
- **Несколько ригов.** Cross-store deps в gc и так запрещены, паритет не теряется. Межриговые эпики — в трекере города (hq). **[П]**
- **Git-merge JSONL.** Экспорт должен делать один владелец (оркестратор) в определённые моменты; иначе конфликты в PR. Поведение
  `br sync` при конфликте — **[НП]**.
- **Бэкап.** Сегодня — Dolt remote + `.dolt-backup` (1,4 ГБ). Взамен: JSONL в git для работы, `VACUUM INTO`/копия файла для state. **[В]**
- **Потеря `AS OF`.** Единственное реальное применение истории Dolt — ручное восстановление; заменяется git-историей JSONL. **[В]**

## 5. Открытые вопросы и непроверенное

- **[НП]** Возможности br сверх `--help` (CAS, транзакции, поведение при параллельной записи из нескольких процессов, custom-типы,
  типы рёбер, merge JSONL) — зона отчёта про beads_rust; сопоставлять с таблицей §2.3.
- **[НП]** Доля нагрузки на Dolt от контроллера vs агентов vs exec-ордеров: 11 соединений/с намекают на короткоживущие процессы
  (`bd`/`gc` из скриптов ордеров и status-line), но разбивку не мерил.
- **[НП]** Насколько RSS 1,37 ГБ вызван именно 34k wisps и 209k `wisp_events`: не проверял на копии без них.
- **[НП]** Работает ли split (`sqlite-beads`) стабильно в бою: у владельца он выключен, по коду — «active migration» с потоком fix-ов.
- **[В], не измерение:** 145k «только ради bd/Dolt» — оценка по именам файлов и пакетов, не по графу вызовов; `CachingStore` (9,8k)
  отнесён к специфике, хотя часть его логики понадобилась бы любому хранилищу с внешними писателями.
- Счётчики строк в живом городе менялись во время замера (gcp: 92 → 111 issues за ~20 минут).
- `engdocs/architecture/beads.md` помечен «Last verified 2026-03-01» и описывает 4 backend-а; фактически их 8 — документация отстаёт.
