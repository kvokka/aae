SCORE: 7.9/10
VERDICT: continue
BLOCKERS: 1

# Ревью цикла 115

Дифф `13aa91e..7109180`: 5 коммитов, 40 файлов, +1126/−160. Это Жел. 1–3 ревью 114, WP-103 (`if` шага и
`back_from`) и WP-104 (`rounds` и `[inputs]` по слоям). Вне `.loop` дерево чистое. Незакоммичены только
`.loop/JOURNAL.md`, `PLAN.md`, `state.json` и `cycles/115/`, их коммитит `loopctl`.

`checks.md` снят на текущем HEAD `7109180`: pass, 37 s, nextest 589/589 за 25.0 s. Гейт я не перезапускал.

## Обязательно до продолжения

1. **Прогон с `continue_from`, у которого пропущен первый шаг, продолжает чужую сессию в другом шаге.** Документы
   говорят, что продолжает только первый шаг: `docs/design/contracts.md:731` («Only the first step continues that
   session») и `docs/design/config.md:720`. Код делает иначе:
   - `queue.rs:115` проверяет только вид первого шага (`ai`), а его `if` не смотрит.
   - Если первый шаг пропущен по `if`, попытка не создаётся, и `row.attempt` остаётся 0.
   - В `machine.rs:148-167` запрос `continued` (нет попыток вовсе) и ветка `(0, _) => from` отдают сессию
     продолжаемого прогона следующему `ai`-шагу. Это может быть шаг другого агента, проверяется только harness.

   Расхождение появилось вместе с `if` в этом цикле. Варианты исправления:
   - отклонять `continue_from` (409), когда у первого шага есть `if`; это одна строка в `queue.rs:115` и строка в
     обоих документах;
   - либо отдавать `from` только шагу `formula.steps[0]`.

   Проверка: тест в `aae-testkit/tests/core` на формулу `a(ai, if = { "inputs.x" = true }) → b(ai)`, старт с
   `continue_from` без `x`. Сейчас `b` резюмирует чужую сессию, после правки должен быть отказ или новая сессия.
   Находку я вывел из чтения кода, тестом не воспроизводил: в цикле я тесты не пишу.

## Желательно

1. В `aae run show` прогон, у которого последний шаг пропущен, остаётся на этом шаге: `step: deploy` при исходе шага
   `review`. Так я видел вживую. Документ (`step` — «the step of the formula it is at») этим формально не нарушен, но
   для читателя `run list`/`run show` это путает.
2. `aae config explain --formula` не показывает `if` шага. В его описании в config.md этого нет, так что расхождения
   нет. Но строка вида `# runs only when …` рядом с `# a check: …` помогла бы отлаживать формулу с условиями.
3. Ветка «нет исхода `pass` у последнего выполненного» (`finish.rs`, `None if !…`) всегда даёт причину
   `every step was skipped`. Я не нашёл, как туда прийти с выполненным, но непрошедшим шагом. Если такой путь
   появится, причина будет врать: лучше различить «ничего не выполнялось» и остальное.
4. Перенесено из ревью 113/114: флейк chaos `a_seed_replays_the_same`, путь `held` в codex, колонка `class` в
   `run list`. Всё это вне плана.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-102 | done | Жел. 1–3 ревью 114 закрыты (`12afadc`, `2f1d7a2`) |
| WP-103 | done | Все пункты «Готово, когда» есть: пропуск, выполнение и пропуск по `back_from`, `if` по `inputs` и `steps`, ошибки загрузки, `step=` в `run watch`, CI. Документы совпадают с кодом, кроме Обяз. 1 |
| WP-104 | done | Слои `rounds` (умолчание, оркестратор, проект, формула) и входов (оркестратор, проект, вызывающий), `explain` с источником, CI. Замечаний нет |
| WP-105…WP-108 | todo | — |

Закрыто 18 пакетов из 22.

## Проверено

- **Гейт.** Взят из `checks.md`: HEAD совпадает, pass, 37 s (в цикле 114 было 51 s), nextest 589/589 за 25.0 s.
  Новые тесты укладываются в 1 s на весь набор из 36.
- **Строки (`scripts/budget.sh`).** Ядро 11223 / 18000, фронт 2746 / 4000, всего 17904 / 30000. Rust по tokei —
  44549 строк кода во всех `crates` вместе с тестами.
  - Самые большие файлы диффа ≤ 400 строк: `machine.rs` 398, `items.rs` 395, `load.rs` 379, `check.rs` 378.
    `retired.rs` вынесен из `files.rs` без изменений.
  - `todo!`/`unimplemented!` нет. Русского в продукте и в сообщениях коммитов нет. Новых зависимостей нет.
  - `docs/agents/` не тронут.
- **Мутации.** 14 своих, каждая откатывалась через `git checkout`, после всех `git status` чистый вне `.loop`. Все
  краснят нужный тест:
  - `run.step_skipped` не эмитится → e2e `a_skipped_step_shows_in_run_watch` и 3 теста `step_if`.
  - `back_from` в контексте `if` → `[]` → `a_step_waiting_for_a_check…`, `a_restart_keeps…`.
  - Корень `back_from` не принят в `step_if` → 4 теста. Убран корень `steps` → 3 теста.
  - `strangers` ничего не проверяет → `the_paths_of_an_if_are_checked_at_load`.
  - Класс «every step was skipped» → `agent` → `a_run_whose_every_step_is_skipped_fails`.
  - `rounds` формулы перебивается слоями → `rounds_come_from_the_strongest_layer…`. Умолчание 3 → 4 → 2 теста.
  - `needs` → `required` в каждом из трёх мест `check.rs` → `a_required_input_the_config_gives_needs_no_caller`.
  - Вход `task` из конфига принят → `a_value_for_a_task_or_of_the_wrong_type…`.
  - Конфиг бьёт вызывающего (`insert` вместо `or_insert_with` в `queue.rs`) →
    `the_caller_beats_the_project_which_beats_the_orchestrator`.
  - Строка `inputs.*` в explain убрана → `a_formula_is_explained_with_the_layers…`.
  - Таблица мутаций кодера (охраны `skip`, `advance`, `step_results`, `step_attempt == 1`, `ran`) правдоподобна.
    Её ключевые точки я перекрыл своими.
- **Флейки.** 36 затронутых тестов (`step_if` в ядре и e2e, `config_if`, `config_defaults`, `config_inputs`,
  `await_step`, `returns`, explain, схема событий): 27 прогонов подряд, 12 из них параллельно с полным workspace без
  chaos (589/589). Все зелёные.
- **Вживую, без модели.** Демон с изолированными `HOME`/`XDG_*` в `/tmp/aae-rev115.fb0d`. Оркестратор задаёт
  `rounds = 5` и `[inputs] deploy = true, focus = "top"`, проект — `rounds = 2` и `deploy = false`. Формула:
  `work → fixup(if back_from = review) → review(back_to = work, падает в 1-м раунде) → deploy(if inputs.deploy)`.
  - `config explain` печатает `rounds = 2  # project`, `inputs.deploy = false  # project`,
    `inputs.focus = "top"  # orchestrator`.
  - Старт без входов:
    - прогон `passed`, лог `work focus=top / review1 / work / fixup / review2`;
    - `run watch` показывает `run.step_skipped run_1 step=fixup` в 1-м раунде и `step=deploy` в конце;
    - исход взят от `review`, в `steps` нет `deploy`.
  - `--input deploy=true --input focus=mine`: вызывающий побеждает, `deploy` выполнен.
  - `deploy=maybe` даёт 409 и тип в тексте ошибки.
  - Всегда падающий `review` даёт `still failing after 2 rounds` (слой проекта). После `rounds = 3` и
    `config reload` — `after 3 rounds`.
  - `config check` ловит ошибки:
    - `deploy = "maybe"` в `aae.toml`, сообщение указывает на файл проекта;
    - `if` с `inputs.nope` и с `steps.deploy.result` (поздний шаг);
    - `back_from = "work"` (не проверка);
    - `rounds = 0`.
  - Демон остановлен.
- **Документы.** Сверены с кодом: config «Formula», Orchestrator/Project, explain, contracts §1.3/§2.1–2.3/§6.1/§6
  (таблица), frontends (`step=`), overview, guide. Ничего не ослаблено, истории нет. «Outcome of its last step» стало
  «of the last step that ran» по пакету плана. Расходится только место из Обяз. 1.
- **Решения.** Решения 128, 129, 130 соблюдены, `back_to` остаётся единственным прыжком. Допущения записаны в журнал
  (expert 09:20, coder 11:10), это выбор внутри пакетов, а не замена существующих допущений.

## Нарушения правил

Нет.

## Следующий шаг

1. Сначала Обяз. 1. Это малый объём: строка в `queue.rs`, тест в `aae-testkit` и строка в contracts §6 и config.md.
2. Затем WP-105 (демо 3: пак, формулы, скрипты; M). Он зависит от WP-102–104, все три закрыты. Пакет трогает
   `demos/` по прямой записи плана. Живой прогон в WP-105 не входит (это WP-107).
3. WP-108 (S) независим, его можно взять следом в том же цикле, если бюджет останется ниже ~150k.
