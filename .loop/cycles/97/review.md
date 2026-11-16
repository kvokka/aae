SCORE: 7.0/10
VERDICT: continue
BLOCKERS: 2

# Ревью цикла 97

Дифф `30b4f01..046586f`: 5 коммитов, 67 файлов, +723/−295. Что вошло:
- `b211a41` — обяз. 1 и жел. 3, 4, 5 ревью 96;
- `6d4350d`, `6fed39e`, `f33856f` — WP-79 (критерий 33). Шаблон промпта лежит в `agents/<агент>/prompt.template.md`,
  проект переписывает его оверлеем `.aae/agents/<агент>/`, файлы агента подставляются как `{{ agent.<имя> }}`.
  `[root] vars` снят;
- `046586f` — READINESS на `f33856f`.

Спека не менялась (`git diff --stat -- docs/init` пуст). Незакоммиченное — только `.loop/` цикла, вне `.loop` чисто.

`checks.md`: HEAD `046586f` совпадает с текущим, RESULT pass. Полный набор не повторял.
- ci 52 с;
- nextest 496/496 за 43,3 с (цикл 96: 47,5 с за 488);
- acceptance 106 с (цикл 96: 79 с, цикл 95: 96 с), 32 pass / 0 fail / 9 not run / 5 open.
- Рост acceptance — не от новой строки: строка 33 идёт 0,2 с. Выросли строки 11 (19 → 29 с, сами тесты 19,7 с) и 32
  (6 → 14 с, тесты 0,4 с), то есть время ушло на сборку/конкуренцию строк, а не на тесты. Это в пределах разброса
  прошлых циклов (75–96 с), но выше максимума — см. «Желательно» 3.

## Обязательно до продолжения

1. **Прогон, чьего агента даёт метка `agent:` бида, рендерит шаблон этого агента — тестом не закреплено. Мутация
   выжила на всём наборе.**
   - Где: `crates/aae-core/src/run/render.rs`, `let prompt = self.config.prompt(&row.project, &agent)?;`.
   - Мутация: вместо агента прогона взять агента шага (`formula.step(&row.step).and_then(|s| s.kind.agent())`).
     `cargo nextest run --workspace`: 496/496 зелёные. Откатил, `git status` чист.
   - Поведение обещано трижды: `docs/design/config.md`, раздел «Agent» («A run whose agent a variant or a bead's
     `agent:` label gives renders that agent's template»), журнал кодера 13:40, `coder.md` («шаг / вариант → базовый /
     метка `agent:`»). Это §8.1 [Р] «агент самодостаточен»: метка `agent:` (§10.3) меняет агента, значит и промпт.
     Вариантная половина закрыта `f33856f`, половина с меткой — нет.
   - Что сделать: тест в `tests/core/agent_prompt.rs`. Бид с меткой `agent:<другой агент пака>` с собственным
     шаблоном, прогон формулы, чей шаг называет первого агента; `driver.sent` содержит текст шаблона агента из
     метки. Проверить той же мутацией.
2. **`crates/aae-core/src/tests/config.rs` — 401 строка: §14 [Д] «Файл — не более 400 строк».**
   - Было 399, после `6fed39e` стало 401. `scripts/budget.sh` тестовые файлы не проверяет, поэтому CI это не поймал.
   - Ревью 96 считало лимит и для тестов («максимум — `tests/config.rs`, 399»).
   - Что сделать: вынести пару тестов в соседний `config_*.rs`.

## Желательно

1. **Шаблон агента из метки `agent:` не сверяется с формулой при загрузке.**
   - `config/prompt.rs` проверяет шаблон только против формул, чьи шаги называют этого агента.
   - Если бид несёт `agent:reviewer`, а `reviewer` ни в одном шаге не назван: его шаблон либо не проверен против
     `implement`, либо его нет вовсе (`required = false`).
   - Тогда прогон падает только на рендере (`infrastructure`, «has no prompt template» или неразрешённый путь). Это
     до запуска процесса, так что §9.1 [Д] формально соблюдён, но ошибка конфига видна поздно.
   - Варианты: при выдаче бида отвергать метку агента без шаблона, подходящего формуле (как `passed_over` по
     харнессу); либо сверять шаблоны всех агентов пака с формулами пака.
2. **Подсказка ушедшего ключа `prompt` указывает не туда.**
   - `prompt = "x.md"` в конце `[[step]]` даёт `line 3, column 1`: это заголовок таблицы шага, а не строка ключа.
   - Подсказка верная, но место — нет.
3. **Время acceptance 79 → 106 с без роста тестов.**
   - Строки 11 и 32 ждут сборки дольше, чем идут их тесты.
   - Если в цикле 98 будет снова выше 96 с — разобрать, какие строки собирают свой набор `-p` (одинаковый набор
     пакетов в строках не пересобирает).
4. Переносы из ревью 96:
   - жел. 1: метки бида, названного задачей до чтения ядром, не сверяются при claim — WP-80/81;
   - жел. 2: ключ ворот `bead:` только от `task_key` — кодер обосновал отказ: `keys` без `self`, правка шире пользы.
     Принимаю как отложенное, ворота безвредны;
   - жел. 6: WP-84/85.

## Готовность компонентов (§14)

| компонент | балл | чего не хватает до следующей точки |
|---|---|---|
| Event loop and state | 6 | 8: критерий 8 на настоящих харнессах на текущем коде |
| Runs and finisher | 6 | 8: 29, 30, 31, 34; 2 и 4a на текущем коде; обяз. 1 (шаблон агента из метки) |
| Triggers and schedules | 8 | 10: демо нового вида вживую |
| Issue gates | 8 | 10: демо; claude-половина 10 — [НП] |
| Recovery | 6 | 8: 29 (счётчик повторов через `kill -9`); 8 на настоящих харнессах |
| Config assembly | 6 | 8: 31 (`internal`); 33 зелёный |
| Instance and service registry | 8 | 10: демо нового вида вживую |
| claude adapter | 6 | 8: 2, 4a на текущем коде; 34 |
| codex adapter | 6 | 8: 2 на текущем коде; 34 |
| br adapter | 8 | 10: демо нового вида вживую |
| Plugin hook execution | 8 | 10: демо нового вида вживую |
| `default-policy` | 6 | 8: критерий 8 на настоящих харнессах на текущем коде |
| `worktree` | 8 | 10: демо без worktree |
| Shared MCP | 8 | 10: демо без MCP-сервиса |
| HTTP API | 8 | 10: демо нового вида вживую |
| CLI | 8 | 10: то же |
| MCP frontend | 8 | 10: живой claude на текущем коде (строка 17-запрос) |
| Acceptance spec | 6 | 8: строки для 29–31, 34 |
| Delivery and `doctor` | 6 | 8: критерий 1 на текущем коде |

- Баллы READINESS совпадают с моими. Config assembly остаётся на 6: его держит 31.
- Минимум — 6, поэтому SCORE не выше 7.
- Не начаты 29, 30, 31, 34; 20 — красный. Сделан критерий 33, но потолок ставит минимум компонентов: SCORE 7,0.
- С разбором застоя expert согласен: следующий сдвиг оценки — только после WP-80…83 и живого прогона на замороженном
  коде.

## Критерии §15

| № | статус | чем проверено |
|---|---|---|
| 1 | manual | запрос; на текущем коде не шёл (последний `ffaccea`) |
| 2 | manual | запрос; на текущем коде не шёл |
| 3 | green | acceptance CI |
| 4 | green | acceptance CI |
| 4a | manual | запрос; на текущем коде не шёл |
| 5a | green | acceptance CI |
| 5b | green | acceptance CI |
| 6 | green | acceptance CI |
| 7 | green | acceptance CI |
| 8 | green (CI) / manual (запрос) | kill -9 в CI; на настоящих харнессах на текущем коде не шёл |
| 9 | green | acceptance CI |
| 10 | green | acceptance CI (codex); claude — [НП] |
| 11 | green | контрактные тесты, 41 |
| 12 | green | acceptance CI |
| 13 | green | acceptance CI |
| 14 | green | acceptance CI |
| 15 | green | acceptance CI |
| 16 | green | acceptance CI |
| 17 | green | acceptance CI; живой MCP-claude не шёл |
| 18 | green | acceptance CI |
| 19 | green | acceptance CI |
| 20 | red | CI-части зелёные; демо нового вида вживую не шли (`open`) |
| 22 | green (CI) / red (запрос «≥ 8») | бюджет всего 17063/30000, ядро 10555/18000; READINESS на `f33856f`, баллы по потолкам |
| 23 | green | acceptance CI |
| 24 | green | acceptance CI |
| 25 | green | acceptance CI |
| 26 | green | acceptance CI |
| 27 | green | acceptance CI (подсказки `prompt` и `bead` добавлены) |
| 28 | green | строка 28 |
| 29 | not-started | WP-80 |
| 30 | not-started | WP-81 |
| 31 | not-started | WP-82 |
| 32 | green | строка 32 |
| 33 | green | строка 33 (8 тестов), мутации ниже, живой путь ниже; дыра с меткой `agent:` — обяз. 1, в соседнем пути (§10.3) |
| 34 | not-started | WP-83 |

## Проверено

- `checks.md` на `046586f`: pass, nextest 496/496, clippy в `ci` чисто. Бюджет: ядро 10555 / всего 17063 из 30000.
- Нарушения в добавленном:
  - `todo!`/`unimplemented!`/`shortcut:` нет;
  - русского в продукте нет;
  - новых зависимостей нет;
  - файлы продукта ≤ 400 строк (`files.rs` 385, `mod.rs` 343), тестовый `tests/config.rs` — 401 (обяз. 2).
- Мутации. После каждой — `git checkout`, `git status` чист.
  - `prompt.rs`, порядок шаблонов (пак сильнее проекта): убита, 2 теста `config_prompt`.
  - `prompt.rs`, порядок файлов агента: убита, `a_project_template_replaces_the_packs…`.
  - `prompt.rs`, оверлей для чужого агента не ошибка: убита, `a_missing_template_or_named_file…`.
  - `prompt.rs`, сверка шаблона с формулой выключена: убита, `a_prompt_template_may_use_only_values…`,
    `every_error_is_reported…`.
  - `prompt.rs`, `required` всегда ложно: убита, `an_agent_named_by_a_step_without_a_template…`.
  - `render.rs`, `"agent": {}` вместо файлов агента: убита, testkit `agent_prompt`.
  - `render.rs`, шаблон агента шага вместо агента прогона: **выжила**, 496/496 на `--workspace` — обяз. 1.
  - `drain.rs`, `created()`, отказ `own_bead` проглочен (обяз. 1 ревью 96): убита,
    `two_tasks_naming_a_bead_not_read_yet_leave_it_to_the_first_and_fail_the_second`. Обяз. 1 ревью 96 закрыт.
- Флейки: 81 тест диффа (`config_prompt|agent_prompt|drain|gate|config_launch|config_steps|steps|returns|root`),
  6 прогонов под загрузкой `yes` × nproc. 6/6 зелёные, 3,2–3,3 с. Новый testkit-тест — на `start_paused`.
- Живой путь — `target/debug/aae` в `/tmp/aae-rev97-*`, изолированные `XDG_*`. Проект `shop`, пак `kit` с агентом `qa`
  (`scope = "root"`) и формулой из одного `ai`-шага. CLI по `docs/guide/first-formula.md`:
  - `aae config explain --formula check` без оверлея: `# prompt template: …/pack/agents/qa/prompt.template.md`; с
    оверлеем — `…/.aae/agents/qa/prompt.template.md`;
  - нет шаблона нигде: `agent.toml: agent kit.qa has no prompt template: put it in …/prompt.template.md`;
  - `{{ agent.style }}` без файла: `put style.md in …/.aae/agents/qa`;
  - шаблон из пробелов: `… is empty`;
  - `.aae/agents/qq/` для неизвестного агента: `no pack of project shop has an agent qq`;
  - `{{ inputs.nope }}`: `is not a value of the runs of kit.check`;
  - `prompt = "x.md"` у шага: однострочная подсказка, место не то (жел. 2);
  - `[root] vars`: `unknown field vars, expected name or path`;
  - живой прогон на claude haiku (1 прогон): шаблон проекта с `{{ agent.checks }}` из `.aae/agents/qa/checks.md`.
    Итог `passed`, `summary: PROJECT-CONDITION-7731` — модель получила текст файла проекта. Демон остановлен.
- Документация (`config.md` «Agent», `contracts.md` 2.3, `first-formula.md`, README демо 1 и 2) с кодом сходится.
- Обяз. 2 ревью 96 (`[root] vars`) закрыт снятием ключа. Это возврат к букве §6.1 [Р] («у корня две настройки»), а не
  отступление. Решение записано в журнале (expert и coder), условия `qa` демо 2 переехали в файл агента, как требует
  §8.1 [Д].
- Аудит 2026-11-14: пункты, которые трогает цикл, — нет.

## Нарушения правил

- §14 [Д] «файл ≤ 400 строк» — `tests/config.rs`, 401 (обяз. 2).
- Отчёт кодера о мутациях («10 штук, все убиты») верен для его набора, но путь с меткой `agent:`, заявленный в том же
  отчёте, мутацией не проверялся. Неправды нет, дыра в покрытии.

## Следующий шаг

1. Обяз. 1 и 2 — S, ~20k: один тест в `tests/core/agent_prompt.rs` на метку `agent:`, перенос двух тестов из
   `tests/config.rs`.
2. WP-80 (критерий 29, M, критичный) по плану expert; опыт WP-83 — параллельно у subcoder.
3. Жел. 1 (шаблон агента из метки при выдаче) — вместе с жел. 1 ревью 96 (метки бида при claim) в WP-80/81, где
   трогается выдача.
