# Цикл 85 — coder
STATUS: done

## Взято в работу
Обяз. 2, жел. 2, 3, 4 ревью 84. Обяз. 1 ждёт владельца. Других пакетов в плане нет: WP-07 blocked.

## Сделано (коммиты)
- `2069b3d` — обяз. 2. В `demos/2-multirepo-qa/frontend-example/README.md` вместо абсолютного пути теперь `cd ../backend`;
  правка руками, записана в журнал. Защита — строка в `mise run ci`:
  `! git grep -nE '/(home|Users)/[A-Za-z]' -- . ':!.loop' ':!docs/init'`. Проверил: на старом README она падает,
  на новом проходит. Строка описана в `docs/guide/development.md`. `docs/init` исключён, потому что в
  `aae-context/03-br-adapter.md:88` есть `/home/ubuntu/.beads`: это вход владельца, его не трогал.
- `27abbaa` — жел. 4. `run.sh` демо 2 теперь выводит и «coder on backend». Строку Models в `RESULT-example.md`
  дописал руками: `aae config explain` показал, что модель та же, gpt-6-luna.
- `4666f71` — жел. 2. Агентский `max_runs` теперь проверяется на границе шага. `held()` (`run/gates.rs`) паркует
  попытку, если у агента прогона уже столько других начатых прогонов, сколько разрешает `max_runs`; drain-шаг слота
  не держит и не проверяется. Как это выглядит снаружи: `run.parked{gate: null}`, `waiting_for = "concurrency"`.
  Выход из парковки — по существующему `blocker()` в `schedule`. Подсчёт вынесен в `agent_full()` (`run/queue.rs`),
  его зовут и `blocker`, и `held`. Shortcut-комментарий из `finish.rs` снят. Тест
  `runs.rs::a_step_of_an_agent_at_its_limit_parks_until_a_slot_frees`: два шага (lead, затем dev), у dev
  `max_runs = 1`. Второй прогон паркуется на шаге dev и продолжает, когда первый прогон кончается. Мутация
  «`full = false`» тест роняет. Обновлены `docs/design/config.md` (known limit снят) и `contracts.md` (`run.parked`,
  `run.resumed`, состояние `parked`, переход `preparing`/`interrupted` → `parked`, §6 «Concurrency»).
- `15f1e4c` — READINESS перечитан на `4666f71`, маркер `ver` сдвинут. Ни один агент демо не задаёт `max_runs`,
  поэтому на их пути новая проверка возвращает false сразу. Баллы не менялись. Строка 22 зелёная.
- Жел. 3: в `.loop/PLAN.md` строка «21» помечена «не критерий §15».
- `scripts/loop/checks 85` на `15f1e4c`: PASS. ci 71 с; nextest 456/456; acceptance-ci 28/0/9.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
—

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 (`drain` и §5.6/§9.1) — ждёт владельца, код не менял.
- Обяз. 2 — закрыто: `git grep -nE '/home/|/Users/' -- demos/` пуст, в CI есть защита.
- Жел. 1 — факт, действий не требует.
- Жел. 2, 3, 4 — закрыты.

## Контекст на момент отчёта: ~115k
