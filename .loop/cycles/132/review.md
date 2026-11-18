SCORE: 9.3/10
VERDICT: complete
BLOCKERS: 0

# Ревью цикла 132

Дифф `8f2dba4..2e9278c`: 4 коммита, 32 файла, +843/−14.
- Обязательный пункт ревью 131 (мутации правила `max_writers`): `533f6e7`.
- Желательные пункты ревью 131 и 130: `eb429bc`, `af6934f`.
- WP-134 (пак `devloop/`): `2e9278c`.

Вне `.loop` незакоммиченного нет. Неотслеживаемый `.loop/research/2026-11-19-claude-traffic-switches/README.md` —
исследование вне цикла, это норма. `.claude/`, `scripts/loop/`, `CLAUDE.md`, `docs/agents/loop.md`, `demos/`,
`decisions.md` и `assumptions.md` дифф не трогает.

`checks.md` снят на текущем HEAD `2e9278c`: RESULT pass, 43 s, nextest 673/673. Гейт я не повторял. Время
упало с 54 s (цикл 131, там была полная пересборка) до 43 s. Сам nextest — 28,5 s против 29,3 s. Две новые строки
`mise run ci` стоят около секунды: selftest — 0,6 s, `config check` — бинарь уже собран.

Обязательных пунктов нет. Все пакеты плана `done`, и статусы правдивы.

## Обязательно до продолжения

Нет.

## Желательно

1. **`check-report.sh` не проверяет вторую строку отчёта (`BEAD:`).**
   - Проверяются только строки 1 и 3. При этом `devloop/README.md` («wants the report's first three lines») и промпт
     кодера («checks the first three lines») говорят о трёх строках.
   - Строка `BEAD:` — единственный способ найти отчёты прошлых прогонов на том же биде: в обоих промптах это
     `grep -l '^BEAD: <id>$'`. Ошибка в ней тихо обрывает преемственность между прогонами.
   - Скрипт получает `LOOP_MANAGER_BEAD`, так что сверка — одна строка плюс один случай в `selftest.sh`.
   - Это правка `devloop/`. Делать её до переезда или после — решает владелец.
2. **`docs/agents/rules-tests.md:87-93`.**
   - Суженное правило («every run of loop-manager or br a test starts … goes through `common::program`») всё ещё
     чуть шире кода. Мост `sh bridge.sh` в `crates/loop-manager/tests/e2e/review_gate.rs:144` запускает
     loop-manager мимо `program`. Вреда нет: мост сам задаёт сокет.
   - Строка 87 длиной 121 символ, перенос после `without_outer_session`: — рваный.
3. **Проверка «review older than the report» в `check-review.sh` слабая.**
   - Она ловит старое ревью, только если кодер в новом раунде переписал `report.md`. Это промпт требует, но скрипт
     не проверяет.
   - Если раунд ушёл на красный `ci`, а отчёт не тронут, старое ревью пройдёт проверку.
   - Мелочь: промпт ревьюера требует писать ревью заново каждый раунд.
4. **Хвосты ревью 130** (пункты «Желательно» 2–6) не тронуты, как кодер и отметил: очистка `BEADS_DIR`, фолбэки
   таймаута, `install.md`, артикль в `retired.rs`, `checks.rs:319`.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-133 | done | — (прошлые циклы) |
| WP-135 | done | Ничего. Дыра из ревью 131 закрыта: четыре мутации правила подсчёта теперь краснеют, каждая своим тестом |
| WP-134 | done | Ничего обязательного. Пак есть, `config check` — строкой в `mise run ci`. `config explain` показывает модель, субагентов (`devloop.helper/subcoder/expert`), аддон и `env`. Самопроверка скриптов идёт в гейте. Pin на этот же репозиторий невозможен — это находка с вариантами в журнале и README. Три выбора expert (метка `auto` у триггера, числа, `inherit_env`) записаны вопросом владельцу |

## Проверено

- **Гейт:** `checks.md` на HEAD `2e9278c` — pass, 43 s, 673 теста. Не повторял.
- **Строки** (`scripts/budget.sh`): ядро 11991/18000, всего 18908/30000. Затронутые файлы ≤ 400 строк: `queue.rs` 385,
  `writers.rs` (тесты) 308. `todo!`/`unimplemented!` нет, новых зависимостей нет.
- **Мутации `max_writers`** (`crates/loop-manager-core/src/run/writers.rs`). Каждую вносил отдельно, после каждой
  откат через `git checkout --`, `git status` чистый. Каждая убита своим тестом:
  - `:18` `return true` → `false` → `a_run_whose_formula_is_gone_keeps_its_place`;
  - `:22` `Exec => true` → `false` → `a_run_of_an_exec_step_takes_a_place`;
  - `:27` `is_none_or` → `is_some_and` → `a_run_whose_agent_is_gone_keeps_its_place`;
  - `:52` добавлен `'interrupted'` в исключённые → `an_interrupted_run_keeps_its_place`.
- **Мутация `waiting_for`:** убрал `UPDATE … waiting_for = NULL` в `queue.rs:362` → краснеет
  `a_rig_at_its_max_writers_queues_the_next_run_until_one_ends`.
- **Мутации скриптов пака.** Каждая валит `selftest.sh` с понятным сообщением:
  - `gate.sh` пропускает 8.9;
  - `check-report.sh` принимает `blocked`;
  - `check-review.sh` теряет сравнение с отчётом.
  Откат после каждой.
- **Флейки:** `writers::|queue` — 3×3 параллельных прогона, 17/17 во всех девяти. Висящих `sleep 60` после
  exec-теста нет.
- **Вживую `waiting_for`.** Демон в `/tmp/rv132w.*` с изолированными `XDG_*`, без модели. Риг с `max_writers = 1`,
  exec-формула `sleep 4`, два прогона. В `run list --json`:
  - сначала `run_2` — `queued`, `waiting_for: "writers"`;
  - затем `running`, `waiting_for: null`;
  - в конце оба `passed`.
  Демон остановлен.
- **Пак `devloop/` без запуска.** Запуск пакет запрещает. Прочитаны все 27 файлов.
  - `config check --config devloop/config.toml` зелёный.
  - `config explain --project devloop --formula task --rig repo`:
    - `rounds = 4`, `max_writers = 1  # rig`;
    - у coder и reviewer — `opus`/`high`, аддон `context-budget` (300000/4, `autoCompactWindow`), три `env` проекта;
    - `BEADS_DIR` и `BD_ISSUE_PREFIX = "lm"`;
    - у `gate` и `ci` — `exec.inherit_env = true`.
  - `doctor --config devloop/config.toml` в изолированном XDG — ok, только известное предупреждение о лицензии br.
  - Сверено с документами:
    - `where` со скаляром по массиву `bead.labels` — «contains» (contracts 2.2);
    - бид корня (`dev-…`) на риге `repo` не отклоняется (`beads.rs:22-26`, `:172`);
    - exit 75 у exec — повтор по `unknown_retries` (3 раза, 30 s), как пишет README;
    - строки после упавшего `ci` не выполняются, `gate` в этом раунде не идёт;
    - шаблоны промптов проверяются при загрузке;
    - имена типов субагентов `devloop.<agent>` совпадают с таблицей в промпте кодера;
    - команды CLI из README существуют;
    - клонирование пака из git берёт локальный путь (`git_cache.rs:111-125`).
- **Язык:** кириллицы в диффе продукта и в сообщениях коммитов нет.
- **Документы:**
  - Неточности `max_writers`, добавленные в config.md, совпадают с кодом: `writes()` считает по формуле самого
    прогона.
  - Абзац `explain` и строка примера перенесены.
  - `docs/README.md` и `development.md` ссылаются на пак.
  - Ослаблений нет. Сужение правила в rules-tests.md приводит документ к коду, это вариант из ревью 130.

## Нарушения правил

- Нет. Отчёт кодера правдив, статусы в плане верны.
- Метка `auto` у триггера — отступление от текста пакета, а не от решения владельца. Причина весомая: иначе бид,
  который агент заводит с префиксом `lm`, сам запускал бы прогон. Записано вопросом в журнал 2026-11-19T05:25.

## Следующий шаг

План закрыт. Дальше — решение владельца:
- ответы на вопросы expert в журнале: метка `auto`, `rounds = 4`, окно 300k/4 сжатия, `inherit_env`;
- сам переезд по `devloop/README.md`, «Moving over»;
- новые пакеты с WP-136.

Если цикл продолжится без новых пакетов, у кодера есть мелочи на ~30k: «Желательно» 2 и 4. Пункты 1 и 3 правят
`devloop/` и ждут владельца.
