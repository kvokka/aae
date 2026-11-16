# WP-78 · план expert · цикл 95

Обследовано на b944759. Логики мало (ядро: ~+40 строк нетто, схема состояния не меняется), но касание широкое:
~50 файлов на префиксы и ~40 на задачу, большей частью механика в тестах, демо и доках. В одну сессию 150k не
влезает. **Делится на два пакета; первым идёт WP-78a (префиксы), вторым WP-78b (задача).** Порядок такой, потому
что после префиксов риг любой строки-id выводится без списка готовых, и «задача» ложится на это без промежуточной
семантики; в обратном порядке тесты рига по метке пришлось бы править дважды.

Проверено руками на br 0.7.6 (временный каталог, без модели), сверх записи журнала 2026-11-16T10:06:
- `BD_ISSUE_PREFIX=api br create A --deps parent-child:<корень>` → `api-aiv`; `br show --json` отдаёт
  `"parent": "<корень>"` — поле `epic` адаптера (`aae-br/src/lib.rs:346`) остаётся верным, `next_ready` с `epic` жив.
- Две зависимости разом: `--deps "parent-child:<корень>,blocks:<id>"` работает (бид не готов, пока блокер открыт).
- `BD_ISSUE_PREFIX=w br create E --parent <корень>` → `<корень>.1`: `--parent` сильнее окружения.
- br приводит префикс к нижнему регистру (`MP` → `mp-…`), принимает `my-api`, `a_b`, `1x`; хеш дефисов не содержит.
- Дочерний через `--parent api-aiv` → `api-aiv.1`: префикс рига наследуется.

## Решения по неясностям (допущения, записаны в журнал)

1. **Риг по id**: префикс — всё до последнего `-` (`id.rsplit_once('-')`); `api-x1.2` → `api`, `my-api-q59` → `my-api`,
   `root-79u.1` → не риг → бид корня. Сравнение точное, со списком префиксов ригов проекта.
2. **Кто решает, что строка — существующий бид.** Ядро знает только список готовых и таблицу владения. Строка,
   известная ядру (`ready_beads` или `beads`), — бид сразу, как сегодня (владение при создании прогона, метки
   `agent:`/`harness:` действуют). Любая другая строка уходит в трекер на `preparing` нынешним эффектом `create`:
   адаптер и так читает `br list -a` ради ключа — там же ищет бид с `id == текст`; нашёл — возвращает его, нет —
   заводит эпик. Новой операции интерфейса нет (решение 52), гонки «бид заведён секунду назад, список не прочитан»
   нет, второго бида нет. Цена: текст ровно вида `<префикс рига>-<слово>` без названного рига пойдёт в этот риг
   (бид корня на риге допустим) — описать в `tracker-br.md`.
3. **Проверка «бид другого рига»** остаётся в двух местах, как сейчас: при старте для известного ядру id, иначе на
   claim (`bead_of_another_rig`). Не удалять claim-проверку: без неё текст вида id пришлось бы отвергать при старте.
4. **«Подсказка об ушедшей метке»**: бид с меткой `rig:` не идёт молча в корень — старт на нём отвергается одной
   строкой («the rig of a bead is the prefix of its id»); `drain` показывает это в `refused`. Плюс абзац в доках.
5. **`grep 'rig:'` из «Готово»** буквально невыполним: ключи ворот `rig:<project>/<rig>` (§5.7) — не метка и
   остаются. Проверка: `grep -rnE 'rig:' crates demos docs | grep -vE 'docs/init|rig: |rig:(<p|[a-z]+/|\{)|"rig:"'`
   находит только подсказку (код п. 4, её тест, абзац доков).
6. **Совпадение с префиксом БД корня** — только предупреждение `doctor` (ядро br не знает); ошибка конфига — только
   совпадение префиксов двух ригов. Ключа `prefix` у `[root]` нет (`deny_unknown_fields` уже даёт ошибку).
7. **Имя входа** спека не задаёт: в демо, доках и приёмке вход называется `task`; в остальных тестах имя `bead`
   остаётся, меняется только тип (дифф механический). Корень шаблона `{{ bead.* }}` не трогаем.
8. Смена `prefix` у рига с живыми бидами делает старые биды бидами корня — одна строка в `config.md`, кода нет.

---

## WP-78a · Префиксы ригов (первый) — критерии 32 (префиксы), 24 и 16 на префиксах

### Шаг 1. Конфиг: префикс рига, `project list` — коммит 1
- `crates/aae-core/src/config/files.rs:285` `RigFile`: `pub prefix: Option<String>`.
- `config/mod.rs:134` `Rig`: `pub prefix: Option<String>` (`None` у корня). Конструкторы `Rig {` — `places.rs:111`,
  `src/tests/mod.rs` (5 мест).
- `config/places.rs` (120 строк → ~165):
  - в цикле `places()` (:58): `prefix = r.prefix` либо акроним — первая буква каждого слова имени (разделители `-`,
    `_`), в нижнем регистре; форма: строчные ASCII-буквы, цифры, `-` только внутри (то, что br не перепишет);
    пустой акроним или плохая форма — ошибка с советом задать `prefix`;
  - два рига с одним префиксом — ошибка: ``rigs `a` and `b` both give their beads the prefix `x`: set `prefix` on one``;
  - `impl Project { pub fn rig_of(&self, bead: &str) -> Option<&str> }` — п. 1 решений; ищет только в `self.rigs`;
  - `:93–100`: комментарий о метке убрать; набор символов имени оставить, предел 46 (длина метки) убрать вместе с
    текстом ошибки (`grep '46 at most'` — только это место и `tracker-br.md:90`).
- `crates/aae-front/src/api/config.rs:52` (`place`): `"prefix": r.prefix`; `api/replies.rs:290` `Rig`: `prefix:
  Option<String>` с rustdoc; текстовый вывод `project list` в CLI — колонка префикса; `docs/api/openapi.json`
  перегенерировать (`AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`).
- Тесты → `crates/aae-core/src/tests/config_root.rs` (141 строка; `config.rs` — 399, туда не добавлять):
  `a_rig_takes_the_acronym_of_its_name_or_its_prefix_key` (`backend`→`b`, `my-project`→`mp`, `prefix = "api"`),
  `two_rigs_with_one_prefix_are_an_error`, `the_rig_of_a_bead_is_the_prefix_of_its_id` (`api-x1`, `api-x1.2`,
  `my-api-q59`, `root-79u.1` → None, строка без `-` → None). Ответ `GET /v1/projects` — `crates/aae/tests/replies.rs`.
- Ловушка: фикстура `src/tests/config.rs:174` держит риги `below` и `beside` в одном проекте — оба `b`; дать одному
  `prefix` в той же строке. Прочие фикстуры: `web`/`api`, `frontend`/`backend` — не сталкиваются.

### Шаг 2. Ядро: риг бида по префиксу — коммит 2 (вместе со всеми тестами, что стояли на метке)
- `crates/aae-core/src/run/beads.rs` (393 строки, после шага меньше):
  - `pick_bead` (:21–43): принимает `&Project`; ветка id — без чтения `ready_beads`:
    `project.rig_of(id).is_some_and(|r| r != rig)` → отказ; `ready_in` (:47–71) теряет параметр `rig` (у `drain` он и
    так `None`), фильтр «свой риг или корень» переезжает в `pick_bead` тем же `rig_of`;
  - `rig_of_bead` (:73–90) удалить; `queue.rs:78–85`:
    `named.and_then(|id| project.rig_of(id)).unwrap_or(&project.root.0)` — отказы «not in the ready list: name the
    rig» и «several rigs» исчезают;
  - `tracked` (:178): `other_rig(&json!(data), &row.rig)` → по `data.id` через конфиг проекта; `other_rig` (:382–386)
    удалить, `labelled` остаётся для `label()`.
- `run/queue.rs:48` `bead_agent`: бид с меткой `rig:` — `rejected` с подсказкой (п. 4).
- `run/bead_events.rs:98–102`: `rig` события — `rig_of(bead.id)`; `ready()` (:15) нужен конфиг — вызов в
  `run/mod.rs:177` становится `self.ready(…)`. У `emit_bead` уже 7 параметров (порог clippy): риг передать парой с
  именем проекта, не восьмым параметром. Проект, которого нет в конфиге, — `rig: None`, чтение не пропускать.
- Комментарии: `run/drain.rs:110`, `run/msg.rs:195`, `tracker.rs:9`.
- `crates/aae-testkit/src/tracker.rs:31`: рядом с `add` — `add_in(prefix, labels, epic)` (id `<prefix>-<n>`, счётчик
  общий); `add` по-прежнему даёт `b-<n>` — бид корня.
- Тесты ядра (`crates/aae-testkit/tests/`): `add(&["rig:web"…], …)` → `add_in("w", &[…], …)`, `rig:api` → `"a"`:
  `core/beads.rs` (7 мест; :156–160 остаётся на префиксе; :187–222; тест :302 переименовать в
  `…takes_it_from_the_prefix_of_the_bead_id…`: риг известен **до** `w.changed()`, ветки «several rigs» и «name the
  rig» уходят, добавить бид с меткой `rig:` → отказ с подсказкой), `core/drain_steps.rs` (17), `core/drain_rounds.rs`
  (4), `core/bead_events.rs` (1), `chaos/main.rs` (2). `core/trigger_scope.rs`, `core/root.rs` — перечитать: риг
  события теперь из id.
- Настоящий br: `crates/aae/tests/tracker.rs:56,67` и `crates/aae/tests/claude.rs:262` — `br create` с окружением
  `BD_ISSUE_PREFIX=w` вместо `-l rig:web` (хелперу `br()` нужен параметр окружения); `created["rig"] == "web"` держится.
- Приёмка в том же коммите (иначе красная): `crates/aae-acceptance/src/scenarios/c24_drain.rs:34–41` —
  `new() { BD_ISSUE_PREFIX=$1 br create "$2" --deps "parent-child:$AAE_BEAD" -l "$3" --silent; }`, B —
  `--deps "parent-child:$AAE_BEAD,blocks:$a"`, F — `BD_ISSUE_PREFIX=a … --deps "parent-child:$s"`, эпик S остаётся с
  `--parent`; проверить, что id несут `w-`/`a-`, а прогоны — свой риг; `c20_demo2.rs:21` — то же; заголовок строки 24
  в `scenarios/mod.rs:181` («rig prefixes and agent and harness labels»). `c16_beads.rs` меток рига не использует —
  только прогнать.

### Шаг 3. Префикс в окружении сессии и exec-шага; контракт br — коммит 3
- `config/mod.rs:122` `TrackerLink`: `pub prefix: Option<String>` — имя переменной, как `actor`.
  `crates/aae/src/harness.rs:89`: `p.tracker.prefix = Some("BD_ISSUE_PREFIX".to_owned())`.
- Один помощник в `places.rs`: окружение трекера для места — `tracker.env` плюс переменная префикса, если место —
  риг. Им заменить оба цикла: `config/compose.rs:183` (место — `r` после фильтра области на :136, то есть агент
  области `root` и прогон в корне переменную не получают) и `run/render.rs:34` (exec-шаг: место `row.rig`).
- `crates/aae-br/src/lib.rs:43` (`br()`): `.env_remove("BD_ISSUE_PREFIX")` — окружение демона не должно дать
  корневому биду чужой префикс.
- Тесты: `src/tests/config_layers.rs` (рядом с :64) — агент области `rig` на риге получает переменную с префиксом,
  агент области `root` и прогон в корне — нет; exec-шаг на риге — `core/exec.rs`.
  `crates/aae-br/tests/contract.rs`: тест :113 переписать без `rig:`; новый
  `a_bead_filed_with_a_rig_prefix_under_a_root_keeps_it_and_parent_loses_it`: `BD_ISSUE_PREFIX=api` +
  `--deps parent-child:<корень>` → id `api-…`, `ready()` отдаёт его с `epic == корень`, `under(корень)` его видит;
  тот же вызов с `--parent` даёт `<корень>.1` (закрепляет поведение, на котором стоит промпт планировщика);
  `Tracker::create` адаптера даёт id с префиксом БД.

### Шаг 4. `doctor` — коммит 4
- Чтение префикса БД сейчас в `crates/aae/src/daemon.rs:313–323`. Вынести в `aae_br::issue_prefix(binary, dir) ->
  Option<String>`, звать из `bv_workspace` и из `crates/aae/src/doctor.rs:68–78`: риг проекта с префиксом, равным
  префиксу БД, — `check("br prefix", "warning", …)` с именами рига и проекта и советом задать `prefix`.
- Тест — `crates/aae/tests/doctor.rs` (`cli.rs` — 378 строк, не туда): `br init --prefix w` + риг `web`.

### Шаг 5. Демо, доки, готовность — коммит 5 (правку текстов отдать субагенту, список ниже)
- Демо 1 и 2: `pack/formulas/plan.md` (:22 и :30) — префикс рига брать из `aae --json project list`, команда
  `BD_ISSUE_PREFIX=<prefix> br create "<title>" --deps parent-child:{{ bead.id }} -l agent:coder,harness:<harness> …`,
  порядок — `--deps "parent-child:{{ bead.id }},blocks:<id>"`, явно: `--parent` не использовать;
  `pack/formulas/deliver.toml` (:17, :18 — комментарий), оба `README.md`.
- Доки: `docs/design/tracker-br.md` (:20, :38–40, :82, :90 + абзац: кто даёт префикс, `--deps` против `--parent`,
  метки `rig:` нет, совпадение с префиксом БД, пп. 2 и 8 решений), `docs/design/contracts.md` (:549 и «rig» в 5.2),
  `docs/design/config.md` (:348, :516–524, :550–552 + ключ `prefix` в справке `[rigs.<name>]`),
  `docs/guide/first-formula.md` (:184, :201), `docs/api/frontends.md:50`, `docs/api/mcp.md:16`; rustdoc
  `crates/aae-front/src/api/runs.rs:29`, `api/mcp.rs:39`, `cli/commands.rs:17` → openapi перегенерировать.
- `crates/aae-acceptance/src/checks.rs:131` (строка 32): в заголовок — префиксы, в фильтр — новые тесты шагов 1–3,
  число ожидаемых тестов поднять ровно на добавленные (фильтр с нулём — сломан).
  `crates/aae-acceptance/src/readiness.rs`: убрать из `OPEN` записи `"24"` (:13–16) и `"32"` (:33–36);
  `docs/READINESS.md` — по правилам цикла.

**Готово 78a:** строки 16, 24, 32 зелёные; grep из п. 5 решений пуст, кроме подсказки; `scripts/loop/checks` зелёный.

---

## WP-78b · Задача вместо бида (второй) — критерий 28

### Шаг 1. Ядро и адаптеры — коммит 1 (вместе с механической заменой типа в тестах)
- `config/formula_file.rs:25`: `Bead` → `Task`. `type = "bead"` становится ошибкой serde со списком вариантов —
  своей подсказки не писать. Использования: `config/check.rs:185,192` (+ текст :182–187), `config/items.rs:147–152`
  («needs a required input of type `task`»), `items.rs:321`, `run/queue.rs:81,133`, `run/drain.rs:81`,
  `crates/aae-front/src/api/mcp.rs:207`.
- `run/queue.rs:130–166`, вход типа `task`:
  - строка пустая или из пробелов — отказ;
  - строка, известная ядру (`EXISTS` по `ready_beads` и `beads` для проекта), — нынешняя ветка «назван id»;
  - иная строка — нынешняя ветка запроса: `bead_agent(…, None)`, бида у прогона нет, вход хранит текст;
  - объект — только `{ next_ready = true, epic? }` (`pick_bead`); `{ request = … }` получает общий отказ с
    перечнем форм (`beads.rs:38–40`: «a task is a text, a bead id or `{ next_ready = true, epic = <id> }`»).
  - риг без названного (`queue.rs:81–83`): `rig_of(строка)` для любой строки, иначе корень.
- `run/drain.rs`: `request()` (:157–169) удалить; `requested()` (:255–260) заменить методом «ключ и текст задачи
  прогона»: ключ входа типа `task` из формулы прогона (тот же поиск, что :79–82 — вынести в один помощник),
  `inputs[key].as_str()` при `row.bead.is_none()`. Вызовы: `run/machine.rs:33` и `created()` (:225–228 —
  `inputs[key] = id`). Шапка модуля (:1–2).
- `tracker.rs:85` — rustdoc `create`: «бид задачи: существующий с id, равным `description`, иначе новый эпик, раз на
  ключ». `crates/aae-br/src/lib.rs:287–296`: в уже прочитанном списке сперва ключ (повтор эффекта), затем
  `b["id"] == description` → `self.one(id)`; сравнение точное, по списку, не через `br show` (тот принимает
  частичные id и флаги). `crates/aae-testkit/src/tracker.rs:232–261` — то же.
- Механика одним `sed` по списку `grep -rln 'type = \\"bead\\"' crates`: `type = \"bead\"` → `type = \"task\"`
  (~25 файлов тестов и сценариев; имя входа `bead` в них не менять).

### Шаг 2. Тесты задачи — тем же коммитом 1 или коммитом 2 (сборка зелёная в обоих случаях)
- `crates/aae-testkit/tests/core/drain.rs` (135 строк) — три теста переписать: текст → один корневой бид (:50);
  крах между созданием и ответом (:70, форма эффекта прежняя); :112 — пустая строка и `{request}` отвергнуты, без
  рига прогон в корне. Новые: `a_task_naming_a_bead_takes_it_up_and_files_no_second_bead` — бид вне списка готовых
  (без `changed()`) и бид из списка: в трекере по-прежнему один бид, он занят прогоном; бид, который держит другой
  прогон, — отказ либо `failed` с «held by».
- `src/tests/config_steps.rs:76–140` и `config.rs:259` — `drain` требует `task`; новый: `type = "bead"` — ошибка
  конфига, в тексте есть `task`.
- `crates/aae-br/tests/contract.rs`: к тесту :186 — `create` с `description`, равным id существующего бида,
  возвращает его, число бидов в `br list -a` прежнее; с текстом — новый эпик.
- Настоящий стек, `crates/aae/tests/tracker.rs` (145 строк): `br create` руками и сразу, не дожидаясь `bead.ready`
  и без рига, `POST /v1/runs` с `"inputs": { "bead": "<id>" }` → `passed`, бид закрыт, бидов столько же; затем
  `"bead": "Add a dark mode"` → один новый эпик с этим заголовком.

### Шаг 3. Фронтенды, приёмка, демо, доки — коммит 3
- `cli/commands.rs:17–21` (справка `--input`: текст, id, `{"next_ready":true}`), `api/runs.rs:29,51`,
  `api/replies.rs:122`, `api/mcp.rs:39` → openapi перегенерировать.
- Приёмка: `c24_drain.rs:22,28,97` и `c20_demo2.rs:101` — вход `task`, значение — строка; новая строка 28 в
  `checks.rs` (280 строк) — фильтр nextest по тестам шага 2 (`-p aae-core -p aae-testkit -p aae-br -p aae`), число
  точное; `readiness.rs` — убрать `"28"` (:17–20) из `OPEN`.
- Демо 1 и 2: `deliver.toml` и `implement.toml` — `task = { type = "task", required = true }`; `run.sh:41–42` —
  строка с `jq` уходит, `--input "task=$request"`; README.
- Доки: `contracts.md:196, :533`, `config.md:477, :548–551`, `tracker-br.md:30, :62–69` (+ п. 2 решений и его
  граничный случай), `first-formula.md:178–201`, `frontends.md:50`.

**Готово 78b:** строка 28 зелёная; `grep -rnE 'type = \\?"bead|\{ *request' crates demos docs` (вне `docs/init`) пуст.

---

## Риски

- **Инварианты данных.** Таблицы `beads`, `ready_beads`, `runs` и формат эффектов не меняются, миграции нет.
  Владение бидом задачи, неизвестного ядру, записывается на `preparing` в `created()` (как у запроса сегодня):
  второй прогон на тот же id получает не отказ при старте, а `failed` с «held by». Для id из списка готовых и из
  таблицы владения отказ при старте сохраняется — тест `beads.rs:60` держится.
- **Совместимость состояния.** Прогон, застигнутый обновлением в `preparing` с входом `{request}`: эффект `create`
  уже в очереди, `created()` перезапишет вход по ключу формулы — доедет. Чтения старой формы в новом коде не
  оставлять. Журнал: у событий `bead.*` поле `rig` теперь из id; форма события прежняя.
- **Восстановление.** Нового состояния нет: префикс считается из конфига на каждую попытку. Хаос-стенд
  (`tests/chaos`) гонять после шага 2 пакета 78a и шага 1 пакета 78b — граница «бид создан, ответ потерян» та же.
- **Тихая отправка в корень.** Планировщик, заведший бид с `--parent` или без `BD_ISSUE_PREFIX`, получит бид корня;
  ловится только промптом и отказом по метке `rig:` (п. 4). Принято владельцем (журнал 2026-11-16T10:40, следствие 1).
- **Столкновение акронимов** (`backend` и `billing`) — ошибка загрузки конфига там, где раньше её не было; текст
  ошибки обязан назвать оба рига и ключ `prefix`.
- **Префикс БД равен префиксу рига** — биды корня уходят в риг; только предупреждение `doctor`.
- **Бюджеты.** Ядро +~40 строк к 10399. Файлы у предела: `run/beads.rs` 393 (уменьшается),
  `src/tests/config.rs` 399 (новых тестов не добавлять), `tests/core/beads.rs` 388, `crates/aae/tests/cli.rs` 378,
  `scenarios/mod.rs` 365.
- **Не проверено.** Вывод `project list` в текстовом режиме и тест на него — найти при работе; живые демо
  (критерий 20, строка `OPEN "20"`) этими пакетами не перезапускаются, их промпты меняются вслепую.
