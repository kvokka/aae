# Цикл 132 — coder
STATUS: done

## Взято в работу
- Обязательный 1 ревью 131: мутации правила `max_writers`.
- Желательные 1, 3, 4 ревью 131 (мелочи).
- WP-134 (стартовый пак devloop/).

## Сделано (коммиты)
- `533f6e7` WP-135: четыре теста в `crates/loop-manager-testkit/tests/core/writers.rs` — exec-формула занимает место
  (реальное время, `sleep 60`), `interrupted` держит место (рестарт), формула ушла reload'ом — держит место, агент из
  метки `agent:` бида ушёл reload'ом — держит место. Все четыре мутации ревьюера (`writers.rs:18`, `:22`, `:27`, `:52`)
  теперь краснеют, каждая своим тестом; после каждой — `git checkout --`.
- `eb429bc` `waiting_for` обнуляется при выходе прогона из очереди (`queue.rs`, `schedule`); проверка в первом тесте
  writers через `Reader::run` (без правки тест краснеет). config.md: известные неточности `max_writers`
  (`continue_from` от worktree-прогона, две drain-семьи после reload, снизившего лимит до 1); две длинные строки
  перенесены.
- `af6934f` rules-tests.md: правило «процессы тестов идут через `common::program`» сужено до запусков loop-manager и br
  (так и есть в коде); убрано «this repository is developed with one» (переезд ещё не сделан) — желательный 1 ревью 130.

- `2e9278c` WP-134: каталог `devloop/` — проект loop-manager для разработки самого себя. План expert —
  `notes/wp134-plan.md` (выборы: `rounds = 4`; context-budget `window = 300000`, `max_compactions = 4` у coder и
  reviewer; `max_writers = 1`; `[exec] inherit_env = true` для `git push` и `mise run ci`; трекер `devloop/.beads`
  с префиксом `dev`, риг `repo` с префиксом `lm`; триггер `bead.ready` только для бидов с меткой `auto` — сверх
  текста пакета, причина в журнале). Реализация — subcoder дословно по плану (`notes/wp134-impl.md`).
  - Файлы: `config.toml`, `loop-manager.toml`, `formulas/task.toml` (code → ci → review → gate), пять агентов
    (`agent.toml`, `prompt.md`; у coder и reviewer ещё `prompt.template.md`), `scripts/check-report.sh`,
    `check-review.sh`, `gate.sh`, `selftest.sh`, `README.md`, `.gitignore`, `.beads/` (4 файла br).
  - `mise.toml` (`[tasks.ci]`): `devloop/scripts/selftest.sh` и `config check --config devloop/config.toml`.
    Ссылки в `docs/README.md`, строка ci в `docs/guide/development.md`.
  - Проверено мной: selftest зелёный (0,6 с; только во временных каталогах, пушит во временный bare-remote);
    `config check` зелёный, в том числе без `br` в PATH (значит, пройдёт в GitHub CI). Вывод `config explain` для
    coder (модель, субагенты, аддон, `env`) — в `notes/wp134-impl.md`, раздел «config explain».
  - Git-источник пака на этот же репозиторий невозможен без нового ключа (пак из git обязан лежать в корне репо):
    находка expert в журнале, принят вариант «правило ревьюера: правка `devloop/` ждёт владельца».
- Гейт: `scripts/loop/checks 132` на `2e9278c` — pass, 43 s, nextest 673/673.

## Не получилось и почему
- Пак вживую не запускался (запрещено пакетом): рендер шаблонов, доставка субагентов, хуки аддона и push не проверены.
- rumdl MD041 (нет H1) в 7 файлах промптов оставлен: это тексты промптов; rumdl в CI не гоняется.
- Пока в `.claude/settings.json` стоят хуки нынешнего лупа, они сработают и в сессиях пака — README называет переезд
  первым условием запуска.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
## Замечания предыдущего ревью: что закрыто, что нет
- Обязательный 1 — закрыт (`533f6e7`).
- Желательный 1 — закрыт (`eb429bc`). Желательные 3, 4 — закрыты (`eb429bc`). Желательный 2 (хвосты ревью 130): пункт 1 закрыт (`af6934f`), 2–6 — нет (малая ценность, вне пакетов).
## Контекст на момент отчёта: ~115k
