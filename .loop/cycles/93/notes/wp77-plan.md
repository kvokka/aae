# WP-77 · Корень проекта и область агента — план (expert, цикл 93)

Основание: спека §6.1, §8.1, §9.3, §12; решения 95, 99; критерии 27 (корень) и 32 (область, один репозиторий);
правила, раздел 9, замечания 7, 9, 12 с поправкой. Код не тронут. Номера строк — на HEAD 2185454.

## Главная идея

Корень — ещё одно **место работы** той же модели, что риг, под своим именем. `Project` хранит его как запись типа
`Rig` рядом с картой ригов; поиск места по имени — одна функция. В строке прогона `runs.rig` лежит **имя корня**
(умолчание `root`), а не NULL. Отсюда без единой особой ветки:

- ключ ворот `rig:<project>/<имя корня>` (gates.rs:66), поле `rig` в конверте событий (row.rs:109), занятость
  рабочего каталога по `runs.workdir` (queue.rs:266), worktree по `run.workdir` (плагин), лимиты, триггеры
  `where rig = …` — работают как есть;
- миграции БД нет: `rig TEXT NOT NULL` (store.rs:34) остаётся;
- `Option<rig>` в ядре не появляется нигде, кроме входа `Create.rig` (он уже `Option`).

Имя корня и имена ригов — одно пространство имён (проверка при загрузке), поэтому поиск однозначен.

## 1. Где ядро сейчас требует риг

| Что | Где | Что сейчас |
|---|---|---|
| `Project.root: PathBuf` — это каталог `aae.toml`, места «корень» нет | config/mod.rs:100 | конфиг, `.beads`, `project.path` шаблона берут его |
| `Project.rigs` — единственные места работы | config/mod.rs:108, 129 | |
| старт без рига | run/queue.rs:78–87, run/beads.rs:75–88 | `None` → риг по метке `rig:` бида; нет бида или метки — отказ «name the rig» |
| рабочий каталог прогона | run/queue.rs:168 | `rig.path` |
| рендер шага | run/render.rs:18–21, 50–53 | `project.rigs.get(&row.rig)` — «rig is gone», `rig`-контекст шаблона |
| слои и «объясни конфиг» | config/compose.rs:116–121 (`place`), 59–67 (`for_run_as` берёт `rig: &str`) | |
| проверка «каждый ai-шаг собирается» | config/compose.rs:229 | цикл по `p.rigs.keys()`: в проекте без ригов не проверяется ничего |
| отпечатки экземпляров при reload | run/reload.rs:112 | цикл по `p.rigs.keys()` |
| триггеры | run/triggers.rs:59–76 (`assigned`), config/check.rs:79 | проектный триггер без `rig` не назначен никуда; `rig` сверяется с `rigs` |
| ключи ворот | run/gates.rs:66 | `rig:{project}/{row.rig}` — не меняется |
| события | run/row.rs:109, run/bead_events.rs:98–101 | `rig` из строки прогона / метки бида — не меняется |
| дочерние `drain` | run/drain.rs:110 | `rig: None` → по метке |
| «объясни конфиг» формулы | aae-front api/config.rs:193, cli/config.rs:41 | «needs its rig», `requires = "rig"` |
| `project list` | aae-front api/config.rs:52–55, api/replies.rs:280–284 | `root` — путь каталога `aae.toml`, корня нет |
| `run start`, API, MCP | cli/commands.rs:15–16, api/runs.rs:30, api/mcp.rs:34–37 | риг уже `Option`; правятся только тексты |
| doctor | aae/src/doctor.rs:96–99, 116 | особая ветка «ригов нет → `-`» — уходит |
| трекер рядом с `aae.toml` | aae/src/harness.rs:86, daemon.rs:120, doctor.rs:69 | `p.root.join(".beads")` |

## 2. Конфиг и модель

Файл (`config/files.rs`, сейчас 364 строки):

```toml
[root]              # вся секция необязательна
name = "root"       # умолчание
path = "."          # умолчание — каталог aae.toml; относительный — от него
vars = { test_cmd = "cargo test" }   # [Д], см. ниже
```

`RootFile { name: Option<String>, path: Option<PathBuf>, vars: BTreeMap<String,String> }`, `deny_unknown_fields`;
в `ProjectFile` — `#[serde(default)] root: RootFile`.

Модель (`config/mod.rs`, 388 строк — добавлять только поля):

- `Project.root: PathBuf` → **переименовать в `dir`** (каталог `aae.toml`: паки, `.aae/agents/`, `.beads`). Восемь мест:
  load.rs:321, variants.rs:105, compose.rs:224, render.rs:57, front api/config.rs:55, harness.rs:86, daemon.rs:120,
  doctor.rs:69.
- `Project.root: (String, Rig)` — имя и запись корня: `path`, `vars`; `triggers`, `settings`, `edits`, `agents` пусты.
- Новый файл `config/places.rs` (чтобы load.rs и mod.rs остались под 400):
  - `impl Project { pub fn place(&self, name: &str) -> Option<&Rig>` (корень по своему имени, иначе риг) и
    `pub fn places(&self) -> impl Iterator<Item = (&str, &Rig)>` (корень первым) `}`;
  - `impl Loader { fn places(...) }` — сюда **переезжает** цикл по ригам из load.rs:256–288 и добавляется корень;
  - `enum Scope { Rig, Root }` (`Default = Rig`, serde lowercase).

Проверки загрузки (все — `self.fail` одной строкой, не `RETIRED`: это не ошибка разбора serde):

- имя корня — тем же правилом, что имя рига (load.rs:258–266, вынести в одну функцию);
- путь корня — каталог;
- `rigs` содержит имя корня: ``rig `X`: the root has this name: rename the rig or set `[root] name` ``;
- путь рига равен пути корня (сравнивать после `canonicalize`, иначе `"."` не поймается):
  ``rig `X`: `<path>` is the project root: drop the rig; the root's name is `[root] name = "X"` ``;
- `vars` корня не прячут `name`/`path` (то же правило, load.rs:271).

Замены поиска: `p.rigs.get(x)` → `p.place(x)` в render.rs:19, queue.rs:86, compose.rs:119, check.rs:79;
`p.rigs.keys()` → `p.places()` в compose.rs:229, reload.rs:112, doctor.rs:96–99 и 116 (ветка `is_empty` удаляется).
variants.rs:116 и check.rs:88 (таблицы `agents`/`triggers` ригов) корня не касаются — не трогать.

Триггеры: проектный `[[trigger]]` без `rig` назначается на корень (triggers.rs:70:
`rigs.extend(trigger.rig.as_deref().or(project_trigger.then_some(root_name)))` — проектные триггеры лежат в первом
паке проекта). Паковый триггер на корень не назначается (ключа `triggers` у `[root]` нет) — записать в журнал.

Шаблон: `rig.name`/`rig.path`/`rig.<var>` прогона в корне — имя, путь и vars корня; `project.path` — путь корня [Д]
(render.rs:57).

## 3. `scope`

- Разбор: `Loader::agent`, config/items.rs:25–55 — там же, где вынимаются `harness` и `prompt`
  (`string(&mut table, "scope")`), значения `rig`/`root`, иное — ошибка с перечнем. Поле `Agent.scope: Scope`
  (mod.rs:156). В `[agents.<имя>]` ключа нет [Д]: вариант наследует область базового агента; там он останется
  «unknown field» от `Settings`.
- Слои (`config/compose.rs`): агент области `root` рига не видит — ни слоя `Layer::Rig`, ни правок
  `[rigs.<r>.agents.<a>]`, ни отображения на вариант. Одна вспомогательная функция
  `fn rigless<'a>(p, r: Option<&'a Rig>, agent: &Name) -> Option<&'a Rig>` (None для `Scope::Root`), вызов в
  `effective` (стр. 35) и в `for_run_as` сразу после того, как известно имя агента шага (стр. 81–89, отображение
  `r.agents` читать уже после фильтра). Так «объясни конфиг» агента `root` одинаков при любом риге и без него.
- Снимок: `EffectiveConfig.scope: Scope` с `#[serde(default)]` (assemble.rs:85; старые снимки живых прогонов
  читаются), ставится в `assemble` из `a.scope`; `explain()` печатает строку `scope = "root"  # agent`.
- Рабочий каталог шага: `runs.workdir` остаётся каталогом **прогона** (место или его worktree). Каталог сессии
  выбирает один помощник в run/render.rs (88 строк, место есть):
  `fn session_dir(&self, row: &Row, config: &EffectiveConfig) -> PathBuf` — путь корня, если
  `config.scope == Root && row.rig != имя корня`, иначе `row.workdir`. Вызов — machine.rs:186 (одна строка;
  machine.rs 384 строки, больше туда не класть).
- Прогон без рига: queue.rs:78–85. `rig_of_bead` (beads.rs:75–88) получает имя корня и возвращает его, когда бид
  не назван и когда у бида нет метки `rig:`; «несколько меток» и «бид не в ready-списке» — отказ, как сейчас (WP-78
  заменит метку префиксом и уберёт и это). beads.rs — 392 строки: правка должна быть не длиннее нынешнего кода.
  `exec`-шаги и агент области `rig` идут в `row.workdir` — без правок.

## 4. Корень без git

Проверено: ядро в рабочем каталоге git не вызывает. `br init`/`create`/`ready` в каталоге без git работают (0.7.6,
`mktemp -d`, руками). Что стоит на git:

| Что | Где | Нужно ли менять |
|---|---|---|
| worktree: `git rev-parse --git-common-dir` в workdir | aae-plugins worktree.rs:40–45 | да: сейчас причина прогона — `hook_failed:worktree`, текст git только в `plugin.hook_failed` |
| снятие worktree на терминале | worktree.rs:47–49 | нет: срабатывает только для пути `…/aae/worktrees/<run>` |
| паки и плагины из git | aae/src/git_cache.rs, config/sources.rs | нет: к корню не относится |
| doctor | aae/src/doctor.rs | нет: git рабочих каталогов не проверяет |
| фикстура приёмки `tracker()` делает `git init` в проекте | aae-acceptance scenarios/mod.rs:353 | нет (не обязательно); новый сценарий — без него |
| демо `run.sh`: `git init` только в ригах | demos/*/run.sh:25–27 | нет: `root/` уже без своего git |
| codex/claude в каталоге без git | адаптеры | в коде проверок нет (`--skip-git-repo-check` — флаг `codex exec`, адаптер ходит через app-server); вживую не проверено — см. риски |

Правка одна, в плагине: перед `rev-parse` (worktree.rs:39) проверить, что workdir в git
(`git rev-parse --is-inside-work-tree`), и если нет — **решение `deny`**, а не ошибка хука:
`{"decision":"deny","reason":"worktree: `<dir>` is not a git repository; formula `<f>` has `worktree = true`"}`.
`deny` на `run.preparing` уже даёт `run.failed`, класс `check`, с этой причиной (decide.rs:94, 204). Загрузка конфига
и `config check` формулу с `worktree = true` в проекте без git не отвергают: ошибка — этого прогона, не раньше.

## 5. Фронтенды, OpenAPI, БД

- CLI `run start <project> [rig] <formula>` (commands.rs:15): только текст справки — «Optional: without it the run
  works in the project root (or on the rig of the bead an input names)». `allow_missing_positional` уже стоит.
- API `POST /v1/runs` (runs.rs:30) — `rig` уже `Option`; описание поля. MCP `start_run` (mcp.rs:34–37) — два текста.
- `GET /v1/projects` (api/config.rs:52–55, replies.rs:280–284):
  `{ name, dir, root: { name, path, vars }, rigs: [...] }`; `rigs` — без корня. `project list` в CLI печатает
  ответ общим принтером (cli/mod.rs:35) — код не нужен.
- «Объясни конфиг» (api/config.rs:193, cli/config.rs:41): `--formula` без `--rig` — на корне (`rig.unwrap_or(имя
  корня)`), убрать `requires = "rig"`; `--agent` рига и так не требует, для `root`-агента риг ничего не меняет.
- OpenAPI: `AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`, закоммитить `docs/api/openapi.json`.
- Миграций БД нет. Переименование корня при живых прогонах ведёт себя как переименование рига («rig is gone»).
- doctor — см. п. 2 (минус ветка).

## 6. Демо

- `demos/1-planner-executor/root/aae.toml`: убрать `[rigs.root] path = "."` (секция `[root]` не нужна: умолчания).
- `demos/2-multirepo-qa/root/aae.toml`: `[rigs.root]` убрать; `[rigs.root.vars] checks` → `[root.vars] checks` —
  временно, до WP-79, который уносит текст в `.aae/agents/qa/checks.md` (замечание 13). `qa.md` (`{{ rig.checks }}`)
  не меняется.
- `pack/agents/planner/agent.toml` (оба демо) и `pack/agents/qa/agent.toml` (демо 2): `scope = "root"`.
- `run.sh` (оба, стр. 42): `run start <project> dev.deliver …` без `root`; пары в `config explain`
  (стр. 51–54 / 52–55): для `planner` и `qa` — без `--rig`.
- `plan.md` обоих демо: фраза «Your own rig `{{ rig.name }}` gets no task» — корень в `rigs` у `project list`
  больше не стоит; переписать на «the root is not a rig and gets no task».
- Шаг `format` демо 2 (`.[].rigs[].path`) менять не надо: корень из `rigs` ушёл, и это правильно.
- README демо (демо 2, стр. 22; демо 1 — схема ригов), `crates/aae/tests/role_prompt.rs:21,46`,
  `aae-acceptance/scenarios/c20_demo2.rs:100` (`"rig": "root"` — убрать поле).
- `aae config check` обоих демо зелёный на каждом коммите; живых прогонов нет (WP-84).

## 7. Тесты

Юнит-тесты ядра — новый файл `crates/aae-core/src/tests/config_root.rs` (tests/config.rs — 398 строк):

1. `a_project_without_a_root_section_has_the_root_next_to_its_file` — имя `root`, путь = каталог `aae.toml`.
2. `the_root_takes_a_name_and_a_path` — `[root] name = "mono" path = "../work"`: место `mono`, путь от каталога
   `aae.toml`, `Project.dir` прежний.
3. `a_rig_at_the_root_path_or_under_its_name_is_an_error` — обе ошибки, каждая одной строкой с подсказкой.
4. `an_agent_of_scope_root_takes_no_rig_layer` — `effective`/`for_run` агента `root` одинаковы с ригом и без;
   правка `[rigs.web.agents.<a>]` его не касается; неизвестное значение `scope` — ошибка.
5. `a_project_of_one_repository_checks_its_formulas_on_the_root` — без ригов шаг без харнесса даёт ошибку конфига
   (сейчас `check_runs` молчит).

Машина прогонов — новый файл `crates/aae-testkit/tests/root.rs` (сценарный драйвер; `workdir` плана сессии он уже
пишет, testkit lib.rs:141):

6. `a_run_without_a_rig_works_in_the_root` — проект без ригов, агент области `rig`: `rig` прогона = `root`,
   workdir сессии = корень; `exec`-шаг там же.
7. `a_root_agent_works_in_the_root_whatever_the_rig` — прогон на `web`, шаги: агент `root`, затем агент `rig`:
   первая сессия в корне, вторая в `web`.
8. `a_bead_of_the_root_runs_in_the_root` — проект без ригов, бид без метки, старт без рига: бид взят, прогон в корне.
9. `a_root_that_is_no_git_repository_runs` — корень — простой каталог: прогон `passed` (может совпасть с 6).
10. В `tests/worktree.rs`: `a_worktree_in_a_directory_without_git_fails_that_run` — `run.failed`, класс `check`,
    причина называет каталог; соседний прогон формулы без worktree в том же корне проходит.
11. Ворота: в `tests/gates.rs` — закрытый `rig:shop/root` держит прогон корня в `queued` (одна проверка, можно внутри 6).

Фронт: `crates/aae/tests/replies.rs`/`api.rs` — форма `/v1/projects` (корень отдельно, в `rigs` его нет);
`crates/aae/tests/cli.rs` — `run start <project> <formula>` и `config explain --formula` без `--rig`.

`acceptance-ci` (`crates/aae-acceptance/src/checks.rs`):

- строка 27 (checks.rs:111–122): дописать в заголовок «the root takes a name and a path and need not be a git
  repository», в фильтр — тесты 1–3 (ядро, счётчик 5 → 8), и вторым вызовом `tests(...)` либо второй строкой «27» —
  тест 9 из testkit (фильтр `-p aae-testkit`);
- новая строка 32, `Launch::Ci`: тесты 4 (ядро), 6, 7, 8 (testkit); заголовок — только про область и проект из одного
  репозитория, с пометкой, что префиксы добавит WP-78; `"32"` — в `ROWS` (checks.rs:326–329).

## 8. Шаги-коммиты (каждый собирается, `config check` демо зелёный)

| № | Коммит | Файлы | Объём |
|---|---|---|---|
| 1 | `Project.root` → `Project.dir` (механика, поведения нет) | 8 мест из п. 2 | ~10 строк |
| 2 | Корень в модели и конфиге: `[root]`, `places.rs`, `place`/`places`, замены поиска, проверки имён и путей, `project.path`, проектный триггер без `rig`; тесты 1–3, 5; **демо: убрать `[rigs.root]`, `[root.vars]`, `run.sh` пока с именем `root`** (оно остаётся верным именем места) | aae-core config/*, run/render.rs, reload.rs, triggers.rs; aae/src/doctor.rs; demos/*/root/aae.toml | ядро +~70, тесты ~110 |
| 3 | Прогон без рига идёт в корне: `rig_of_bead`, тексты отказов; тесты 6, 8, 9, 11 | run/queue.rs, run/beads.rs; testkit tests/root.rs | ядро ±0, тесты ~110 |
| 4 | `scope`: разбор, `rigless`, `EffectiveConfig.scope`, `session_dir`; тесты 4, 7 | config/items.rs, compose.rs, assemble.rs, mod.rs; run/render.rs, machine.rs | ядро +~35, тесты ~70 |
| 5 | worktree без git — `deny` с причиной; тест 10 | aae-plugins worktree.rs, tests; testkit tests/worktree.rs | +~10, тесты ~40 |
| 6 | Фронтенды: `/v1/projects`, explain без рига, тексты CLI/MCP, OpenAPI; тесты фронта | aae-front api/config.rs, replies.rs, mcp.rs, cli/*.rs; docs/api/openapi.json; crates/aae/tests | +~25, тесты ~40 |
| 7 | Демо: `scope = "root"`, `run.sh` без рига, промпты, README; `role_prompt.rs`, `c20_demo2.rs` | demos/**, crates/aae/tests/role_prompt.rs, aae-acceptance c20_demo2.rs | ~40 строк текста |
| 8 | Приёмка: строки 27 и 32 | aae-acceptance checks.rs | ~25 |
| 9 | Документы: `docs/design/config.md` («Project»: `[root]`, оба ключа и `vars`; «Agent»: `scope`), `contracts.md` (2.3 `project.path`, 6.1 старт без рига и каталог шага, 7 ключ ворот корня), `docs/api/frontends.md`, `mcp.md`, гайды `first-project.md`/`first-formula.md` (проект из одного репозитория) | docs/** | ~120 строк текста |

Итог: ядро ≈ +105 строк кода (12 796 → ~12 900 из 18 000), фронт +25, плагины +10, тесты ≈ 370. Один цикл.

Параллельно у `subcoder` после шага 4 (файлы не пересекаются): шаг 5 (aae-plugins + testkit worktree.rs), шаг 6
(aae-front + crates/aae/tests), шаг 9 (docs/). Шаги 1–4 — один исполнитель подряд (общие файлы ядра). Шаги 7 и 8 —
после 6.

## Допущения [Д] — coder записывает в журнал одной записью РЕШЕНИЕ при реализации

1. Секция `[root]`, ключи `name`, `path` (как предложено в плане цикла) и `vars`. `vars` владелец не называл: корень
   служит ригом исполнителя в проекте из одного репозитория, а шаблоны паков читают `rig.<var>`; заодно демо 2 живёт
   до WP-79 без временной правки промпта. Если владелец против — ключ убирается тремя строками.
2. В `runs.rig`, событиях и ключах ворот корень стоит под своим именем; `rig`-контекст шаблона прогона в корне —
   корень.
3. Шаг агента `root` в прогоне на риге работает в основном каталоге корня и **не занимает** его: занятость и
   worktree остаются свойством прогона (его места). Иначе шагу пришлось бы ждать каталог посреди прогона — новое
   состояние машины. Прогон в самом корне (без рига) корень занимает как обычно.
4. Агент `root` не получает слой рига, правки рига и вариант рига; `scope` задаётся только в `agent.toml`.
5. `project.path` шаблона — путь корня; каталог `aae.toml` в API — `dir`.
6. Проектный триггер без `rig` работает в корне; паковый на корень не назначается.

## Риски

- **Лимит 400 строк**: mod.rs 388, load.rs 387, beads.rs 392, machine.rs 384, queue.rs 367, files.rs 364,
  tests/config.rs 398. Новое — в `config/places.rs`, `run/render.rs`, новые файлы тестов; `scripts/budget.sh` гонять
  после шагов 2 и 4.
- **Продолжение сессии** (`continue_from`) берёт `workdir` прежнего прогона (queue.rs:167–186): для шага агента
  `root` в прогоне на риге каталог сессии всё равно корень — совпадает, если формула та же. Отдельно не тестируется.
- **codex в каталоге без git** вживую не проверен (в коде адаптера проверок нет). Проверит WP-84 на демо: `root/`
  демо уже без своего git, но лежит внутри репозитория aae — чистой проверки это не даёт; точечный прогон на `luna` в
  `mktemp -d` — по правилам живых моделей, с записью в журнал.
- **WP-78 перепишет** `rig_of_bead`, `other_rig` и метку `rig:` — в шаге 3 менять их минимально.
- `canonicalize` для сравнения путей рига и корня: путь рига может не существовать (уже ошибка «not a directory») —
  сравнивать только когда оба каталога есть.
- Бид с меткой `rig:<риг>` в прогоне корня получит отказ «labelled for another rig» (beads.rs:26, 177) — это верно
  и до WP-78 так и остаётся.
