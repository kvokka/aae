# WP-19 — план кодера (expert недоступен в этом цикле)

## Решения
- `aae-core/src/tracker.rs`: `Tracker` с подмножеством операций contracts 5.1, которое ядро вызывает сейчас:
  `claim`, `release`, `close`, `ready`. Остальные восемь добавляет пакет, который их вызывает (WP-31, WP-22b) — без
  кода «на вырост». Типы `Bead`, `TrackerError` — по contracts 5.2.
- Трекер — на проект (`<project>/.beads`, одна БД на проект): `run::start(.., trackers: BTreeMap<project, Arc<dyn Tracker>>)`.
  Проект без трекера: вход `bead` отклоняется («no tracker»).
- Схема V8: `beads(project, id, owner, generation, failures)` — владение (истина, §10.3) и счётчик неудач;
  `ready_beads(project, id, pos, epic, labels, body)` — последний `ready(None)` трекера (его же WP-31 сравнивает для
  `bead.ready`); `runs.bead`, `runs.bead_data`.
- Владение пишется при создании прогона (решение о запуске, инвариант 3): второй прогон на бид отклоняется с
  причиной; «следующий готовый» выбирается из `ready_beads` (эпик, без владельца, без закрытых ворот `bead:`, риг
  по метке `rig:` или без метки) и пишется владельцем в той же транзакции — одна операция в единственном писателе,
  поэтому 5 одновременных запросов получают 5 разных бидов. Нет кандидатов → ответ `{"nothing_ready": true}`;
  у триггера — `trigger.skipped{nothing_ready}`. Вход в `inputs` заменяется id бида.
- Claim в br — эффект при `queued → preparing` (contracts 6.2), ключ очереди `tracker:<project>` (все операции
  трекера проекта по порядку: claim раньше release/close того же бида). Прогон ждёт в `preparing` ответа claim
  (дедлайн `state` = start_timeout): ответ даёт данные бида для шаблона промпта (`bead.*`), затем рендер и хуки
  `run.preparing`. `Claimed` → `failed{infrastructure, bead_claimed}`; `NotFound`/`Invalid` → `failed{infrastructure}`;
  `Unavailable` → повтор эффекта с бэкоффом + `tracker.sync_failed`.
- Ответ claim проверяет поколение (инвариант 2): `beads.owner == run && generation == gen`, иначе отбрасывается.
- Терминальное: `passed` → `close`, `failed`/`cancelled` → условный `release` (владелец `aae:<run>`), если claim
  ставился; владение в состоянии снимается в той же транзакции; при `failed` (кроме `bead_claimed`) `failures + 1`,
  при `passed` — 0; `failures >= bead_failures` (конфиг оркестратора, умолч. 3) → ворота `bead:<project>/<id>`
  закрывает `core`. Открытие ворот `bead:` оператором обнуляет счётчик.
  `OwnerMismatch`/`NotFound` у release/close → `resync.diverged{subject: bead}`; исход прогона не меняется.
- После каждой операции трекера — эффект `sync` (`ready(None)` → `Msg::Ready`), и при старте писателя; `Msg::TrackerChanged`
  — вход для наблюдателя WP-31.
- Ключ ворот `bead:<project>/<id>` входит в ключи прогона с бидом.
- `BD_ACTOR=aae:<run>:<n>` и путь БД в окружении сессии — WP-31 (там известен путь БД); вызовы демона идут с
  владельцем `aae:<run>`.

## Шаги
1. tracker.rs, схема V8, конфиг `bead_failures`, `run::start` + исполнитель, `MemoryTracker` в aae-testkit.
2. Создание с бидом, `nothing_ready`, триггеры, RunView.bead, ключ ворот.
3. preparing ждёт claim, шаблон `bead`, терминальное close/release, счётчик → ворота.
4. Тесты `aae-testkit/tests/beads.rs` + тест ворот через `World::restart` (критерий 19).
5. contracts/config docs, READINESS, PLAN, журнал.
