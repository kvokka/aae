SCORE: 7.2/10
VERDICT: continue
BLOCKERS: 0

# Ревью цикла 114

Дифф `b2accfb..15b2bd6`: 3 коммита кодера, 42 файла, +993/−90. В них добивка ревью 113 (Обяз. 1, Жел. 1–3) и
пакет WP-102 (шаг `await`) целиком. Вне `.loop` дерево чистое: незакоммичены только `.loop/JOURNAL.md`, `PLAN.md`,
`state.json` и `cycles/114/`, их коммитит `loopctl`.

`checks.md` снят на текущем HEAD `15b2bd6`: pass, 51 s, nextest 570/570 за 26.4 s. Гейт я не перезапускал.

## Обязательно до продолжения

Нет.

## Желательно

1. **Запись `run.awaiting` в схеме событий ничем не закреплена.** Если удалить строку
   `"run.awaiting" => &[("run", RUN), ("deadline", Leaf)]` в `crates/aae-contract/src/config/schema.rs:113`, весь
   workspace без chaos и acceptance остаётся зелёным (568/568). Без этой строки триггер или хук на `run.awaiting` не
   загружается: «unknown event type», `items.rs:316` и `load.rs:150`. А именно на это событие будет подписан мост
   WP-105/106. Вживую всё работает: я проверил загрузку, отказ на путь `deadlin` и срабатывание триггера. Стоит
   дописать контрактный тест в `aae-contract/src/tests/template.rs` рядом с проверками `event_path`, примерно
   `event_path("run.awaiting", "deadline", true) == Some(true)`, либо тест загрузки триггера на `run.awaiting`
   с `where` по `deadline`.
2. Тестами не покрыты `aae run resolve … --data @<file>` и исход `fail` через CLI. Кодер сам указал это в отчёте.
   Функция `data_arg` общая с `aae finish`, а `fail` покрыт в ядре, так что риск низкий. Вызов `run.preparing` при
   входе в `await` тоже не проверен отдельным тестом. Путь общий с `drain`/`formula`.
3. В `docs/guide/first-project.md:49` перечислены виды шагов «`exec`, `ai` or `drain`». Там нет `formula` (это
   было и до цикла) и нет нового `await`. Мелочь, но ссылка на раздел «Formula» снимет её.
4. Перенесено из ревью 113: Жел. 4 (флейк chaos `a_seed_replays_the_same` под нагрузкой) и Жел. 5 (путь `held` в
   codex, колонка `class` в `run list`). Всё это вне плана, решать планировщику.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-101 | done | Обяз. 1 и Жел. 1–3 ревью 113 закрыты, мутации проверены (см. ниже) |
| WP-102 | done | Все пункты «Готово, когда» есть: pass, fail с `back_to` и remarks, fail без `back_to`, оба `fail_on_timeout` на управляемых часах, чужой шаг отклонён, рестарт, `max_runs = 1`, упавшая проверка перед `await`, e2e на CLI. Документы (config «Formula», contracts §1.3/§6.1–6.4, invariants «Recovery», overview, frontends, architecture, openapi) совпадают с кодом. Жел. 1–3 |
| WP-103…WP-108 | todo | — |

Закрыто 16 пакетов из 22.

## Проверено

- **Гейт.** Взят из `checks.md`: HEAD совпадает, pass, 51 s, nextest 570/570.
  - Рост с 35 s до 51 s даёт сборка: правки в `aae-contract` и `aae-core` пересобирают зависимые крейты. В цикле
    110 было так же, 45 s. Сам nextest стабилен: 25.2–26.4 s за циклы 108–114.
  - Новые тесты быстрые. Самый долгий — `a_waiting_run_leaves_its_slot…`, 0.84 s; остальные в пределах 0.25 s.
- **Строки (`scripts/budget.sh`).** Ядро 11035 / 18000, фронт 2738 / 4000, всего 17708 / 30000. Самые большие файлы
  диффа ≤ 400 строк: `machine.rs` 397, `queue.rs` 386, `items.rs` 385. `todo!`/`unimplemented!` нет. Новых
  зависимостей нет. Русского в продукте и в сообщениях коммитов нет.
- **Мутации.** 24 штуки, каждая откатывалась через `git checkout`, после каждой проверял `git status`. Выжила одна
  (Жел. 1), остальные краснят нужный тест:
  - `recover.rs`: исключение `await` при рестарте → `a_restart_leaves_the_wait…`. Это инвариант Recovery п. 3.
  - `NO_PROCESS` без `'await'` → `a_waiting_run_leaves_its_slot…`.
  - `start_await`: `schedule` → `Ok(())` → тот же тест.
  - `row_ends` без `Await` → `a_failed_check_before_an_await…`. Это решение 126.
  - `resolve`: каждое из условий state / step_kind / step, лимит summary, лимит data → `an_outcome_for_another_step…`.
  - `await_timeout`: `fail_on_timeout` → `false` → тест мягкого таймаута; → `true` → оба теста таймаута с отказом.
  - Ветка `await` в `events.rs` удалена → 3 теста.
  - Дедлайн берётся из `AWAIT_TIMEOUT_MS` вместо `timeout` шага → 4 теста.
  - Ключ идемпотентности в `api/runs.rs::resolve` убран → e2e.
  - CLI без `awaiting` → e2e.
  - SQL-фильтр `?5` → `1 = ?5` → e2e.
  - Колонка шага в `run list` убрана → e2e.
  - Значение по умолчанию `fail_on_timeout` → `false` → контрактный тест.
  - `back_to` у `await` отброшен при загрузке → контрактный тест и `a_fail_sends_the_run_back…`.
  - `stream.rs:146` без `!self.open` и без охраны «последнее не `State`» → `a_background_subagent_keeps…`. Это
    Обяз. 1 и Жел. 1 ревью 113, закрыты.
  - Флаг в прозе скилла `--parent` → `--parents` и флаг в `sh`-блоке → `every_command_a_skill_names_exists`. Это
    Жел. 3, закрыт.
  - **Выжила:** удаление `run.awaiting` из `schema.rs` (Жел. 1).
- **Флейки.** 14 затронутых тестов (ядро `await`, e2e, `replies`, субагент claude, плагин) прогнаны 12 раз подряд и
  ещё 24 раза в цикле, параллельно с полным workspace-прогоном. Все зелёные, флейков нет.
- **Вживую, без модели.** Демон с изолированными `HOME`/`XDG_*` в `/tmp/aae-rev114.*`, формула
  `work(exec) → approve(await, back_to=work) → ship(exec)`:
  - `run list` и `run list --awaiting` показывают `running  approve`.
  - `resolve <run> work pass` → `rejected`, exit 1, состояние не меняется.
  - `resolve … approve fail --summary "redo please"` → раунд 2, `work.log` = `work []` / `work [approve: redo please]`.
  - `pass --data '{"pr":7}' --key k1`: повтор с тем же ключом даёт тот же ответ, прогон `passed`.
  - В `run show` в `steps` лежат исходы `work` и `approve`.
  - `timeout = "4s"` даёт `failed{check, await_timeout}`.
  - `fail_on_timeout = false` → `passed`, summary `timeout`.
  - `cancel` ждущего прогона работает.
  - **Рестарт.** SIGTERM, 6 s простоя, затем старт. Прогон с истёкшим за время простоя дедлайном получает
    `failed{check, await_timeout}`, как обещают invariants и contracts §6.4. Второй ждущий прогон продолжает ждать на
    той же попытке.
  - **Конфиг.** Триггер `on = "run.awaiting"` загружается и запускает свою формулу. `where` по `deadlin` отклонён
    при загрузке. `max_attempts` у `await` даёт ошибку загрузки. `config explain` показывает «await an outcome
    from outside». `--data '{bad'` даёт ошибку использования.
  - Демон остановлен.
- **Документы.** Каждое утверждение диффа `docs/` сверено с кодом: умолчания 168h/`true`, класс `agent` у `fail` без
  `back_to`, снимок `{fail_on_timeout}`, `steps` без текущего шага, отказ remark для не-`ai` шагов, 409/404. Ничего не
  ослаблено, истории нет. Строка overview «A human gate as a formula step» убрана по пакету плана.
- **Решения.** Владельца (126, 127, 120, 116, 62, 88) соблюдены: опроса нет, дедлайн — время стены, слот не
  держится. `docs/agents/` не тронут. Допущения WP-102 expert записал в журнал (08:45). Это выбор внутри пакета, а
  не замена существующих допущений.

## Нарушения правил

Нет.

## Следующий шаг

WP-103 (`if` и `back_from`, M, критичный) зависит от WP-102, и его можно брать. Он задевает загрузку конфига
(проверка путей, ссылки на шаг с `if`), движок (пропуск шага, событие `run.step_skipped`, исход последнего
выполненного шага) и документы. Это полный цикл, план expert до кода, как в WP-102. Если после WP-103 бюджет
останется, рядом можно закрыть Жел. 1 (одна строка теста) и Жел. 3. WP-104 (S) — отдельно или следом в том же цикле,
если WP-103 закончится до ~150k.
