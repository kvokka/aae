# WP-122 «Субагенты claude» — план реализации (expert, цикл 126)

Итог: ~80 строк продукта (ядро ~+30, адаптер claude ~+45), ~150 строк тестов, ~40 строк документации. Новых файлов
продукта нет. Сценарий без модели — тест в `crates/aae-claude/tests/harness/claude.rs` (настоящий `claude` + фейковый
провайдер `aae_testkit::anthropic::Provider`), он идёт в `mise run ci`; acceptance не нужен.

## 0. Что проверено руками (claude 2.1.326, свой одноразовый фейк Messages API, без модели, n=1)

Скрипты во временном каталоге, не сохранены. Это надо внести строкой в таблицу experiments.md «Subagent definitions
and nesting» с маркером `MEASURE` (src — этот файл).

- Точка в имени типа работает: `--agents <файл>` с ключом `kit.qa` → `init.agents == ["kit.qa"]`; главный поток
  получает «Available agent types for the Agent tool:\n- kit.qa: Checks the work. (Tools: All tools)»; вызов
  `Agent{subagent_type:"kit.qa"}` запускает субагента с его `prompt`. Двоеточие (`kit:qa`) тоже принимается.
- `mcpServers` определения — **массив**: `[{"<имя>": {type, command, args, env}}]`. Объект отвергается:
  «Invalid --agents configuration: kit.qa.mcpServers: Invalid input: expected array, received object», exit 1.
- `prompt` обязателен (строка), пустая строка принимается. `description`, `model`, `effort`, `hooks` (формат как в
  `--settings`) в одном определении приняты, вызов прошёл.
- С `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1` в `init.agents` только определённые типы; описание инструмента
  говорит: «the general-purpose agent is not available in this session, so choose "fork" or one of the listed agent
  types» — нативный `fork` (копия себя с контекстом) остаётся всегда.
- Путь к файлу у `--agents` подтверждён раньше (experiments.md, «`--agents` with a file path in `-p`») → файл.

## 1. Как конфиги целей доходят до адаптера

Снимок прогона — JSON `EffectiveConfig` в `runs.config` (`run/render.rs:117-140`, из `Config::for_run_as`). Каждая
попытка (`run/machine.rs:108-111`) читает его, `bind_services` (`:206`), `driver.split` (`:207`), и готовая
`ProcessPart` уходит в эффект `launch` (`:267-274`). Resume читает тот же снимок; после рестарта демона эффект
переигрывается с той же `ProcessPart`. Значит, достаточно положить цели в сам `EffectiveConfig`.

- `crates/aae-contract/src/config/assemble.rs`: поле
  `#[serde(default)] pub targets: BTreeMap<String, EffectiveConfig>` — «The configs of the agents of `subagents` a
  session of a run may call, by name; empty outside a run». Старый снимок → пустая карта → без `--agents`.
  `new()` — `targets: BTreeMap::new()`. `explain()` не трогать.
- `crates/aae-contract/src/config/subagents.rs`: новая
  `pub(super) fn targets(config: &Config, p: &Project, r: Option<&Rig>, me: &str, c: &EffectiveConfig) -> BTreeMap<String, EffectiveConfig>`:
  список = `c.subagents` либо `[me]`; для `t == me` — `c.clone()` (конфиг сессии, со слоями шага: не падает там, где
  агент не собирается один); для остальных — `config.named(p, r, t, Vec::new()).ok()` (несобирающаяся цель — уже
  ошибка загрузки в `unfit`; через `?` нельзя: `compose.rs:334` зовёт `for_run` на загрузке, и появились бы
  вторые ошибки); в карту идут только те, у кого есть `description`.
- `crates/aae-contract/src/config/compose.rs`, `for_run_as` (после `named`, строка 124):
  `config.targets = super::subagents::targets(self, p, r, &name, &config);`. Только здесь, не в `named`: иначе
  взаимные списки дают бесконечную рекурсию. `Config::effective` (explain, doctor, API) целей не несёт.
- Граница ядра цела: в `aae-contract` ни одного имени харнесса, только карта.
- Литералы `EffectiveConfig { .. }` в тестах — добавить `targets: Default::default()`: `aae-claude/src/tests.rs:17`,
  `src/tests/process.rs:21`, `tests/harness/claude.rs:31`, `aae-codex/src/tests.rs:17`,
  `tests/harness/common.rs:90`, `tests/harness/codex.rs:29`. Адаптер codex поле не читает (WP-123).

Сервисы целей. `bind_services` (`crates/aae-core/src/run/services.rs:35`) связывает только свой конфиг — у целей
сервисы и общие stdio-серверы сейчас не связаны. Правка (~10 строк): в начале `let targets = take(&mut config.targets)`,
в конце для каждой цели `t.services.retain(|_, s| s.value.mcp.is_some())` (сервис без MCP даёт только `env`, а `env`
цели до субагента не доходит — незачем его стартовать), рекурсивный `Self::bind_services(tx, row, t, daemon)`,
`bound` дополняется теми id, которых в нём ещё нет (иначе `use_services` дважды увеличит `sessions`), цель кладётся
обратно. `${AAE_API}` и заголовки `X-Aae-*` цели получают тем же кодом; `X-Aae-Agent` — агент прогона (субагент —
не прогон). Порты и токены в `process.agents` подставит существующий `service_proc::resolve` (замена по тексту всего
payload, `service_proc.rs:158`).

## 2. Сам агент в своём списке (допущение до ответа владельца, запись в журнале сделана)

Сам агент определяется в `--agents` тогда и только тогда, когда у него есть `description`; определение — конфиг
самой сессии. Без `description` тип не определяется. Почему: claude требует `description`, а текст, которого никто
не писал, aae в контекст модели не подставляет; правило одно на всех («в `--agents` идут агенты списка с
описанием»); вызов себя без описания у claude и так есть нативно (`fork`). Отвергнуто: подставной текст (имя агента
вместо описания) и «никогда не отдавать себя» (противоречит «the session gets the definitions of its agent's list»).

## 3. Имена типов

Имя типа — запись разрешённого списка как есть: `kit.qa` для агента пака, имя варианта для варианта. Столкновения
`kit.qa`/`other.qa` нет: имена полные и уникальные. Проверено (п. 0). Проверка встроенных типов WP-121 остаётся.

## 4. `mcpServers` и `hooks` цели, которых нет у сессии

- MCP — по имени: сервер цели идёт в `mcpServers` определения, если у сессии (после связывания) нет сервера с таким
  именем; одноимённый — сервер сессии, субагент видит его и так. Одно имя с разным определением — новая ошибка
  загрузки в `subagents::unfit`: «its `mcp.<k>` is not the caller's» (как уже есть для `env`; сравнение
  `Sourced.value`, `Mcp: PartialEq`).
- Хуки — по значению: включённый хук цели идёт в `hooks` определения, если у сессии нет хука с равным `Hook`
  (event, matcher, command, timeout) под любым именем — иначе он сработал бы дважды, хуки `--settings` срабатывают
  и внутри субагентов. `Stop` цели claude сам отдаёт как `SubagentStop`.
- До субагента не доходят (сказать в config.md): `env` цели (субагент живёт в процессе и окружении вызывающего),
  её блок `native`, сервис без MCP. Хуки сессии внутри субагента не отключаются (config.md:55 уже говорит).

## 5. Скиллы целей

Объединение в один плагин `aae` (`--plugin-dir`), видны всей сессии: `skills.entry(имя).or_insert(путь)` — сначала
сессии, потом целей. Одно имя с разными каталогами (у вызывающего и цели или у двух целей) — новая ошибка загрузки
в `unfit`: в замыкание `why` передать `seen = c.skills.clone()`; у цели скилл с именем из `seen` и другим значением
→ «its skill `<k>` is not the one the session has»; иначе `seen.extend`. Адаптеру веток ошибок не нужно.

## 6. Адаптер: `crates/aae-claude/src/part.rs` (316 → ~360 строк, предел 400)

- `process()`: `"agents": agents(config)?`; в `skills` дописать скиллы `config.targets` (п. 5). Комментарий о
  составе части дополнить `agents`.
- Вынести из `mcp()` `fn servers(config) -> Result<Map<String, Value>, Failure>` (записи серверов), из `settings()`
  — `fn add_hooks(into: &mut Value, hooks: impl Iterator<Item = (&String, &Sourced<Hook>)>) -> Result<(), Failure>`
  (пишет в `into["hooks"]`); оба нужны определению агента.
- `fn agents(config: &EffectiveConfig) -> Result<Value, Failure>`: на каждую `(name, t)` из `config.targets` —
  `{"description", "prompt": system_prompt или ""}`, `model` и `effort` только когда заданы, `mcpServers` —
  **массив** `[{имя: запись}]` из `servers(t)` без имён `config.mcp`, `hooks` через `add_hooks` (п. 4); пустые
  `mcpServers`/`hooks` не писать. Нет целей → `Value::Null`.
- `args()`: `if !p["agents"].is_null() { "--agents", file("agents.json") }` — в общем списке флагов, поэтому и на
  `--resume`. `materialize()`: в цикл файлов добавить `("agents", "agents.json")` (каталог 0700, токены сервисов).
- `env()`: `("CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS", "1")` в массив переменных адаптера — всегда (решение 143:
  по умолчанию только сам агент; встроенные типы видны только у codex). Вложенность не задаётся.
- Старая `ProcessPart` в очереди эффектов без `agents` → `Null` → флага нет.

## 7. Шаги (каждый — отдельный коммит, точечные тесты по ходу)

1. Ядро: поле `targets`, `subagents::targets`, вызов в `for_run_as`, две проверки в `unfit`, литералы в тестах.
   Тесты `crates/aae-contract/src/tests/` (в `config_subagents.rs` 360 строк — новые тесты в соседний модуль,
   общие заготовки сделать `pub(super)`):
   - `a_run_carries_the_configs_of_the_agents_it_may_call`: ключи `targets` — записи списка с `description`; конфиг
     цели — её собственный (слой шага вызывающего до неё не дошёл); вызывающий без `description` отсутствует, с
     `description` — присутствует и равен конфигу сессии; без списка и описания карта пуста; снимок без ключа
     `targets` читается (`serde_json`), карта пуста.
   - `a_target_mcp_server_of_the_callers_name_with_another_definition_is_an_error`.
   - `a_skill_name_with_two_directories_is_an_error` (вызывающий–цель и цель–цель).
2. Ядро: `bind_services` для целей. Тест в `crates/aae-testkit/tests/core/services.rs` (362 строки, место есть) —
   `a_service_of_a_target_is_bound_once_for_the_session`: у цели сервис с MCP, которого нет у вызывающего, и общий с
   ним: `w.services()` — по одному процессу, в части драйвера `targets[<цель>].mcp[<имя>].url` — готовый URL с
   заголовками, общий сервис посчитан один раз (после конца прогона потребителей нет). В
   `aae-testkit/src/control.rs` — `pub fn targets(run, attempt)` рядом с `mcp()` (3 строки).
3. Адаптер (п. 6). Тесты в `crates/aae-claude/src/tests.rs` (156 строк; в `tests/process.rs` уже 298):
   - `the_agents_of_the_list_reach_the_session_and_again_on_a_resume`: `part::process` на конфиге с целью `kit.qa`
     (описание, роль, модель, effort; хук, равный хуку сессии, и свой; сервер с именем сервера сессии и свой;
     скилл) — `p["agents"]["kit.qa"]` равен ожидаемому JSON целиком (свой хук и свой сервер есть, общих нет,
     `mcpServers` — массив), `p["skills"]` содержит скиллы обоих; `args` для `New` и для `Resume` содержат
     `--agents <files>/agents.json`; `materialize` пишет файл, равный `p["agents"]`; без целей — нет ни флага, ни файла.
   - `no_built_in_agent_type_is_left`: `part::env` даёт `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1` с целями и без.
4. Сценарий без модели, `crates/aae-claude/tests/harness/claude.rs` (266 строк) —
   `the_main_thread_sees_the_agents_of_the_list_and_no_other`: `turn(vec![Reply::Text("OK")], ..)` с
   `c.targets = {"kit.qa": цель с описанием, ролью, хуком, stdio-сервером `true`, скиллом}` (полное определение:
   claude сам проверит его форму, при ошибке `init` не придёт): `init["agents"] == ["kit.qa"]` (ни встроенных, ни
   чужих), тела запросов провайдера содержат `- kit.qa: <описание>`, `init["skills"]` содержит скилл цели.
5. Документация (в тех же коммитах, где меняется поведение):
   - `docs/design/config.md` «Subagents»: что получает сессия claude (п. 2–5: имя типа — запись списка; сам агент —
     только с `description`; MCP по имени, хуки по значению; скиллы видны всей сессии; что не доходит; встроенные
     типы выключены; глубина — `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` в `env` пользователя); в абзац ошибок
     загрузки — две новые; фраза «a target needs one, the caller does not» → вызывающий без описания собой не
     вызывается.
   - `docs/design/contracts.md` 4.6, claude, «Transport»: `--agents` (файл `agents.json` рядом с `settings.json`,
     на каждом запуске и resume) и переменная; 4.2 — поле `targets`, если `EffectiveConfig` там перечислен;
     раздел 8/сервисы — сервисы целей связываются вместе с сессией.
   - `docs/design/adapters.md`: адаптер доставляет `EffectiveConfig.targets` нативными определениями (рядом со
     строкой 122 о диалекте хуков).
   - `docs/design/experiments.md`: строка с находками п. 0 и `MEASURE`.
6. `scripts/loop/checks 126` на закоммиченном HEAD.

## 8. Вне пакета и риски

- Поведение меняется у всех агентов claude: встроенные `general-purpose`, `Explore`, `Plan` исчезают, без списка
  остаётся только `fork`. Это решение 143 и текст пакета; сказать в config.md прямо.
- Не делается: вызов субагента в тесте (нужен новый `Reply::Agent` в testkit; условие «готово» его не требует),
  поле `skills` определения (предзагрузка), `env` и `native` цели, `targets` в `explain`.
- Сейчас `scripts/budget.sh` красный: `crates/aae/tests/e2e/cli.rs` — 416 строк при пределе 400 (коммит `d7f91fa`,
  WP-124). До гейта разнести файл, иначе `mise run ci` не пройдёт независимо от WP-122.
- `TESTED` в `crates/aae-claude/src/lib.rs` — до 2.1.325, стоит 2.1.326; не трогать в этом пакете.
