# WP-57 · Риг из метки бида — дизайн

## Решение: вариант (в) — ядро берёт метку из `ready_beads` внутри `create`

Ядро уже знает метки готовых бидов синхронно: таблица `ready_beads` (тело бида целиком), и `pick_bead` уже читает её
для отказа «another rig». Тот же запрос отвечает на «какой риг у бида». Новой операции трекера, поиска во фронтенде и
«прогона без рига» не нужно.

- (а) `Tracker::show` отвергнут: §10.1 [Р] — операция добавляется, только когда без неё нельзя; звать её пришлось бы
  из `aae-front` (ядро в транзакции не ждёт), туда же тащить `Trackers` и разбор «какой вход формулы — бид»; правки в
  трейте, br, `MemoryTracker`, обёртке в `tests/epic.rs`; повтор с тем же ключом ходил бы в br ещё раз. Это M.
- (б) «риг позже, на claim» отвергнут: `runs.rig NOT NULL`, workdir, очередь по workdir, ключи ворот и `for_run`
  нужны при создании; и «нет рига — ошибка до запуска» превратилась бы в провал уже созданного прогона.

Граница варианта (в), записать в журнал как РЕШЕНИЕ по [Д] §9.3: метка известна только у бида из готового списка.
Бид заблокирован, отложен, занят или записан после последнего чтения трекера — старт без рига отвергается с текстом
«name the rig». Готовый список перечитывается по `TrackerChanged` и при старте, отставание — доли секунды; отказ не
запоминается под ключом идемпотентности, повтор проходит. Та же граница уже есть у отказа «another rig» (`beads.rs`,
комментарий в `pick_bead`).

## Правки

1. `crates/aae-core/src/run/msg.rs`: `Create.rig: Option<String>` (rustdoc: `None` — the rig of the bead the start
   names). `triggers.rs:317` — `Some(rig)`.
2. `crates/aae-core/src/run/beads.rs`: вынести из `other_rig` сбор меток `fn rigs(bead: &Value) -> Vec<&str>` и
   добавить `rig_of_bead(tx, project, input: Option<&Value>) -> Result<String, Error>` (~20 строк): вход не строка →
   отказ 1; нет строки в `ready_beads` → отказ 2; меток `rig:` ноль → отказ 3; больше одной → отказ 4; одна → она.
3. `crates/aae-core/src/run/queue.rs` `create`: порядок «проект → формула → риг». После `project.formula(..)`:
   `let rig_name = match c.rig.take() { Some(r) => r, None => Self::rig_of_bead(tx, &c.project, <значение входа типа
   Bead>)? };` затем прежний `project.rigs.get(&rig_name)` (метка с ригом, которого нет в конфиге, даёт прежнее
   `no rig `x` in `p``). Ниже `c.rig` → `rig_name` (`for_run`, `pick_bead`, INSERT). Побочный эффект: при неверных и
   риге, и формуле первой теперь называется формула.
4. `crates/aae-front/src/api/runs.rs`: `CreateRun.rig: Option<String>` с doc-комментарием (попадает в OpenAPI),
   проброс как есть. `docs/api/openapi.json` перегенерировать (`crates/aae-front/tests/openapi.rs`).
5. `crates/aae-front/src/api/mcp.rs`: `start_run` — `"required": ["project", "formula"]`, у `rig` описание
   «Optional when an input names a bead with one `rig:` label.»; описание инструмента — «…on a rig (named, or that of
   the bead)». Поведение то же: `start_run` зовёт `runs::create`.
6. `crates/aae-front/src/cli/commands.rs`: `rig: Option<String>` и `#[command(allow_missing_positional = true)]` на
   варианте `Start` → `aae run start <PROJECT> [RIG] <FORMULA>`. Проверено опытом на clap 4.6.7: `p f`, `p r f`,
   `p f --input bead=x`, `p --key k f` разбираются верно, `debug_assert` проходит. Особенность: опция между ригом и
   формулой (`p r --key k f`) перестаёт разбираться — в репозитории таких вызовов нет (тесты, demos, docs проверены);
   в документации написать «options go after the positionals». В теле запроса `rig: null` при отсутствии.

## Тексты отказов (все — `rejected`, HTTP 409, до создания прогона)

1. `no rig: name one, or a bead with a `rig:` label` — рига нет, входа-бида по id нет (в том числе `next_ready`).
2. ``bead `{id}` is not in the ready list of `{project}`: name the rig``
3. ``bead `{id}` has no `rig:` label: name the rig``
4. ``bead `{id}` is labelled for several rigs: name one``
5. Риг назван, метка другая — без изменений: ``bead `{id}` is labelled for another rig than `{rig}` `` (готовый
   список знает бид) либо `failed{infrastructure, bead_of_another_rig}` на claim.

## Идемпотентность `POST /v1/runs`

Менять нечего. Хэш запроса считается по телу, как оно пришло (без рига); разрешение рига — внутри той же транзакции
писателя, после проверки ключа: повтор успешного запроса возвращает первый ответ и метку не перечитывает, даже если
она с тех пор изменилась. Отказ (`Err`) откатывает транзакцию и под ключом не хранится (`writer.rs`, `process`).

## Тесты

- `crates/aae-testkit/tests/common/beads.rs`: `fix_on(rig: Option<&str>, bead)`, `fix` = `fix_on(Some("web"), ..)`.
- `crates/aae-testkit/tests/beads.rs`, один тест `a_start_without_a_rig_takes_it_from_the_label_of_the_bead`
  (`MemoryTracker::add(&labels, None)` + `w.changed()`):
  - бид `rig:web`, риг не назван → `{"run"}`, у прогона `rig == "web"`;
  - бид без метки, риг не назван → отказ 3; прогонов не прибавилось, и `w.fix(json!(bead))` с ригом после этого
    проходит (бид никто не занял);
  - бид `rig:api`, риг `web` → отказ 5 (если такой assert уже есть рядом — сослаться, не дублировать);
  - заодно по одной строке: бид не из готового списка (без `changed()`) → отказ 2; `rig:api` без рига → `no rig
    `api` in `shop``.
- `crates/aae/tests/cli.rs`: `run start shop check` (без рига и без бида) → код 1, stderr `aae: no rig: …` —
  доказывает необязательный позиционный аргумент и путь CLI → API → ядро. MCP отдельного теста не требует, кроме
  снимка схемы инструментов, если он есть (grep `start_run` в тестах).
- Остальные ~19 литералов `Create { rig: .. }` в `crates/aae-testkit/tests` — механически `Some(..)`, по ошибкам
  компилятора.

## Документы

- `docs/design/contracts.md` 6.1, пункт «A run belongs to a project and a rig»: риг назван при создании, либо взят
  из единственной метки `rig:` бида, названного входом (бид должен быть в готовом списке, иначе отказ с просьбой
  назвать риг), либо это риг триггера; нет рига или риг неизвестен — отказ до появления прогона.
- `docs/api/frontends.md`, CLI: `aae run start <project> [<rig>] <formula>`, опции после позиционных; MCP
  `start_run` — `rig` необязателен; грубо по строке на каждый.
- `docs/design/tracker-br.md`, «Beads and runs» — одна фраза, если там описано правило «another rig».

## Не делаем

Риг для `next_ready` без рига (подбор по всем ригам); риг дочернего прогона по умолчанию от родителя; хранение меток
неготовых бидов из потока изменений (колонка в `beads` + миграция) — добавить, если граница выше начнёт мешать.

## Размер

S, ближе к верхней границе (~60–70k): ядро ~30 строк, фронтенды ~10, тесты ~45 новых + ~20 механических правок,
документы ~8 строк, перегенерация OpenAPI. Бюджеты §14 не затронуты.
