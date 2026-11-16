# Цикл 98 — coder
STATUS: partial

## Взято в работу
1. Обязательные пункты ревью 97 (1 — тест метки `agent:`, 2 — `tests/config.rs` > 400 строк).
2. Разбор застоя с expert (повторная сверка разбора цикла 97).
3. WP-80 (критерий 29) по плану expert; опыт WP-83 параллельно у subcoder.

## Сделано (коммиты)
- `da89828` — обяз. 1 и 2 ревью 97:
  - тест `agent_prompt::a_run_of_a_bead_labelled_with_an_agent_gets_that_agent_template`: бид с меткой `agent:qa`,
    формула `fix` с шагом агента `fixer`, `driver.sent` = шаблон `qa`. Мутация ревьюера (в `render.rs` шаблон агента
    шага вместо агента прогона через `for_run_as(.., None)`) — убита только новым тестом; откачена;
  - `a_prompt_template_may_use_only_values_its_runs_have` переехал в `config_prompt.rs`; `config.rs` 385 строк;
  - `scripts/budget.sh`: лимит 400 строк теперь на все `crates/**/*.rs`, включая тесты и тестовые крейты
    (проверено пробным файлом на 401 строку — ловит).

- `ce53dfa` — жел. 2 ревью 97: подсказка ушедшего ключа (`prompt` у `[[step]]`) указывает строку самого ключа, а не
  заголовок таблицы (`files.rs::key_at`); тест `a_retired_key_is_one_line…` проверяет `line 5, column 1`.
- `ada2772` — WP-83, опыт (subcoder, поддельные провайдеры, без модели): `docs/design/experiments.md`, раздел
  «Background work after the outcome». codex — `thread/backgroundTerminals/clean`, claude — управляющий `stop_task`;
  ловушка в `aae-claude/src/stream.rs` (`notified_since_result`). Лог — `.loop/cycles/98/notes/wp83-experiment.md`.
- `046842f` — WP-80, конфиг (subcoder): `max_attempts`, `step_check_script_path` у шага любого вида, проверка
  исполняемого файла, «объясни конфиг», `config.md`, два теста.
- `336cb8a` — WP-80, машина: `run/repeat.rs` (`check`, `check_done`, `repeat`), `enter(.., again)`, миграция 22
  (`step_attempt`, `step_remarks`), `RunView.step_attempt`, запрос `entering` (повтор продолжает сессию), вход `drain`,
  замечание повтора (`remarks` + дописывание), `ExecDone.check`, `stop_exec` для скрипта, восстановление `finishing`.

## Разбор застоя (с expert)
`.loop/cycles/98/notes/stagnation.md`, запись в журнале 23:30. Вывод цикла 97 в силе: ступень шкалы, не застой работы.
`SCORE` ≤ минимум компонентов + 1; на 6 девять компонентов; до 8 их поднимет только закрытие WP-80…83 и один живой
прогон строк 1, 2, 4a, 8 на замороженном коде. Порядок: 98 — WP-80 + опыт WP-83; 99 — WP-81 + код WP-83; 100 — WP-82,
заморозка, живой прогон. Мерило до тех пор — `OPEN` (было 5).

## Не получилось и почему
## Начато и не закончено (файлы, точка остановки, следующий шаг)
## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыт (`da89828`), обяз. 2 — закрыт (`da89828`) и закреплён проверкой в `budget.sh`.
## Контекст на момент отчёта: ~60k
