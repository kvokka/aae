SCORE: 9.0/10
VERDICT: complete
BLOCKERS: 0

# Ревью цикла 105

Дифф `ba6b363..HEAD`: 5 коммитов, 47 файлов, +1262/−1439. Почти весь объём — переснятые `-example` демо (`9d34e48`).
Что вошло:
- обяз. 3 ревью 104 (`e9cb8f0`);
- обяз. 1 ревью 104 (`dc5453a`);
- демо-конфиг `human_timeout = "2m"` (`b32c55c`);
- полный живой прогон на `b32c55c`, `-example` и READINESS (`9d34e48`, `04984d6`) — обяз. 2 ревью 104.

Общие проверки:
- спека, `Cargo.lock`, `mise.toml` не менялись. В `Cargo.toml` только workspace-lint `disallowed_methods = "allow"`;
  `clippy.toml` новый;
- русского в продукте и в сообщениях коммитов нет; `todo!`, `unimplemented!`, `dbg!` нет;
- абсолютных путей машины в `demos/` нет. `package.json` демо 2 — без зависимостей;
- вне `.loop` дерево чистое; незакоммичены только `JOURNAL.md`, `state.json` и `cycles/105/` — штатные файлы цикла.

`checks.md`: HEAD `04984d6` совпадает с текущим, RESULT pass, поэтому полный набор я не повторял.

## Обязательно до продолжения

Нет.

## Желательно

1. **Предел сценария не прерывает сценарий** (перенос жел. 1 ревью 104).
   - Где: `aae-acceptance/src/main.rs:153-158`. `timeout` поверх `JoinHandle` задачу не отменяет.
   - Нужно: `abort()` и убить группу `run.sh`.
   - В этом цикле не стрельнуло: оба демо уложились в пределы.
2. **`aae-acceptance/src/main.rs` и `scenarios/mod.rs` — по 398 строк из 400** (перенос жел. 2 ревью 104). Выносить
   до следующей правки.
3. **Находка «claude спрашивает разрешение под `bypassPermissions`» записана только в README демо 1 и в журнале.**
   - Поведение харнесса, на которое наткнётся любой, кто запускает агентов без присмотра. Его место —
     `docs/design/experiments.md` (строка «Permission request») или раздел claude в `contracts.md`: какой запрос был
     (heredoc в каталог вне рига), что делает `human_timeout` и как его укоротить.
   - Сейчас в документации есть только умолчание «1 h».
4. **`aae-acceptance` читает READINESS по `CARGO_MANIFEST_DIR` времени сборки** (журнал 16:50).
   - Сборка из worktree с общим `target/` читала чужое дерево, и кодер лечил это `touch`.
   - Лучше брать корень репозитория во время запуска (cwd или `--root`), а не зашивать при сборке.
5. **Ждут владельца:**
   - предложение по харнессу (журнал 02:20, цикл 104);
   - вопрос о наследовании `task` (жел. 6 ревью 103);
   - WP-07 (опыт 6, [НП] §7.5, песочница codex) — `blocked`.

## Готовность компонентов (§14)

Баллы READINESS (`04984d6`) сверены по букве шкалы. Десятки опираются на полный живой прогон на `b32c55c`: последняя
правка `crates/` — `dc5453a`, после неё менялись только конфиг демо, `-example` и READINESS.

| компонент | балл | чего не хватает до следующей точки |
|---|---|---|
| Event loop and state | 10 | — |
| Runs and finisher | 10 | — |
| Triggers and schedules | 8 | 10: демо без подписок и расписаний |
| Issue gates | 8 | 10: демо не задевают ворота; claude-половина критерия 10 — [НП] по самой спеке |
| Recovery | 10 | — (живые kill -9 на claude и codex, строка 8) |
| Config assembly | 10 | — |
| Instance and service registry | 10 | — |
| claude adapter | 10 | — |
| codex adapter | 10 | — |
| br adapter | 10 | — |
| Plugin hook execution | 10 | — |
| `default-policy` | 10 | — |
| `worktree` | 8 | 10: демо без worktree. ECHILD закрыт (обяз. 1 ревью 104), путь пройден мной вживую |
| Shared MCP | 8 | 10: демо без общего MCP-сервиса |
| HTTP API | 10 | — |
| CLI | 10 | — |
| MCP frontend | 8 | 10: демо его не используют |
| Acceptance spec | 10 | — (жел. 1 — качество раннера, не критерий) |
| Delivery and `doctor` | 10 | — (четыре цели — на теге, правило владельца 18:40 цикла 103) |

**`SCORE`.**
- Красных и не начатых критериев §15 нет. Минимум компонентов — 8, значит потолок 9.
- Ставлю 9,0: все обязательные пункты ревью 104 закрыты на деле. Мутации красные, флейков под нагрузкой нет.
  Полный живой прогон прошёл 48 из 50 строк: 22 упала только на устаревшем READINESS, на HEAD она pass; 1 —
  on a tag.

## Критерии §15

| № | статус | чем проверено |
|---|---|---|
| 1 | green | цель хоста — полный живой прогон `b32c55c` (release); четыре цели — на теге (решение владельца) |
| 2 | green | полный живой прогон: claude haiku, codex luna |
| 3 | green | acceptance CI (`checks 105`) |
| 4 | green | acceptance CI |
| 4a | green | полный живой прогон (haiku, 39 с) |
| 5a, 5b | green | acceptance CI |
| 6 | green | acceptance CI (p50 8 ms, p90 55 ms) |
| 7 | green | acceptance CI (RSS 30,4 MB, CPU 0,033 %) |
| 8 | green | acceptance CI (20 kill -9); живой прогон: 5+5 kill -9 на claude и codex |
| 9 | green | acceptance CI |
| 10 | green | codex — acceptance CI; claude — [НП] по формулировке самой спеки |
| 11 | green | контрактные тесты, 41/41 в живом прогоне |
| 12–16 | green | acceptance CI |
| 17 | green | живой прогон (haiku вызвал `list_formulas`), acceptance CI |
| 18, 19 | green | acceptance CI |
| 20 | green | живой прогон `b32c55c`: демо 1 — 286 с; демо 2 — два запуска подряд, 1260 с и 769 с, по 1 раунду, без `-dirty`; CI-строки демо зелёные; `-example` с датой и версиями |
| 22 | green | `checks 105` на HEAD: READINESS на `b32c55c` ≥ `dc5453a`. Бюджеты: ядро 11075 / 18000, всего 17688 / 30000 |
| 23 | green | acceptance CI |
| 24–29 | green | acceptance CI; 24 и 30 — мои 3 параллельных прогона `--ci 24 30`, все pass |
| 30 | green | acceptance CI; флейк ECHILD устранён (см. «Проверено») |
| 31–34 | green | acceptance CI |

## Проверено

- **Проверки.** Из `checks.md`, HEAD `04984d6`:
  - ci pass за 91 с; nextest 548/548 за 52,7 с (цикл 104 — 68,6 с, роста нет);
  - acceptance 40 pass / 0 FAIL / 9 not run / 1 on a tag / 0 open, 159 с.
- **Мутации.** Каждая откачена `git checkout`, после каждой `git status` вне `.loop` чист.
  - `orphans.rs:48`: `spawn_std` не регистрирует ребёнка в `OWN`. Тест
    `the_reaper_leaves_a_std_child_to_its_wait` красный (ECHILD), остальные 5 тестов `orphans` зелёные.
  - `worktree.rs`: вернул прямой `Command::spawn()` мимо `orphans`. `cargo clippy -p aae-plugins` даёт error
    `use of a disallowed method std::process::Command::spawn`, то есть обход ловит CI.
  - `run/exec.rs:90` (инв. 6/7): sweep без ожидания смерти. Тест recovery красный (recovery.rs:216). Через 10 с
    `pgrep -f 'while \[ ! -e halt'` пуст — обяз. 3 ревью 104 закрыт на деле.
- **Обход жнеца.** Чтением проверены все места запуска детей в крейтах демона:
  - `aae-core`, `aae-br`, `aae-claude`, `aae-codex`, `aae-front`, `aae-plugins`, `aae` — `#![deny]` в корнях;
  - `#[allow]` только на `orphans::spawn` и `spawn_std`, на модулях тестов и на `bridge.rs`. Мост — отдельный
    процесс без жнеца, причина записана;
  - `exec` в `main.rs` — замена процесса, не ребёнок.
- **Флейки.**
  - `orphans::`, recovery-тест, `worktree` (13–14 тестов): 5 прогонов подряд и ещё 6 параллельно с полным
    `aae-testkit` (257/257) — всё зелёное;
  - `aae-acceptance --ci 24 30`: 3 параллельных прогона, 3/3 строки pass в каждом.
- **Вживую.** `target/debug/aae` на HEAD, `/tmp/aae-rev105-DpQa`, изолированные `HOME` и `XDG_*`, путь по
  `docs/guide/first-project.md` и `first-formula.md`:
  - `project list`;
  - exec-формула с `worktree = true` → `run_1 passed`. Worktree под `.git/aae/worktrees/run_1` создан и убран, ветка
    `aae/run_1` удалена. Это хук `worktree` через `spawn_std`;
  - после `br init` — формула с входом `type = "task"`, `--input 'task=Add a dark mode'` → `run_2 passed`. Корневой
    эпик `hello-wmp` заведён и закрыт `passed: pass`. Это br через `orphans::output`;
  - `aae doctor`: br 0.7.6 найден, версия через `output_std`;
  - в логах демона нет «No child processes». Демон остановлен.
- **Документы и код.**
  - `contracts.md` (регистрация детей, `clippy.toml`) соответствует коду.
  - README демо 1 (абзац о `human_timeout`) соответствует `root/config.toml`.
  - README демо 2 («two in a row, the second in `RESULT-example.md`») соответствует `RESULT-example.md`.
  - `human_timeout` описан в `docs/design/config.md` и `contracts.md`.
- **READINESS.** Последняя правка `crates/` — действительно `dc5453a`. Маркеры `full-run` и `readiness` на
  `b32c55c`. Отчёт прогона `.loop/cycles/105/notes/full-run-b32c55c.md`: 48 of 50, FAIL только 22 (READINESS на
  `11839e9`).
- **Бюджеты.** `scripts/budget.sh`: ядро 11075 / 18000, всего 17688 / 30000. Тронутые файлы ≤ 400
  (`orphans.rs` — 283, тесты вынесены в `orphans/tests.rs` — 159).
- **Предыдущее ревью.**
  - Обяз. 1 закрыт: все дети через `orphans`, clippy ловит обход, тест и мутация есть.
  - Обяз. 2 закрыт: полный прогон, `-example` без `-dirty`, READINESS, `checks` pass, WP-85 `done` теперь правда.
  - Обяз. 3 закрыт.
  - Жел. 1 и 2 не сделаны (перенесены). Жел. 3 ждёт владельца. Жел. 4 закрыт. Жел. 6 ждёт владельца.
- **План.** WP-85 `done` — правда. План покрывает спеку; WP-07 `blocked` (опыт §16, [НП] §7.5), это не критерий §15.
- **Аудит.**
  - Цикл трогает п. 3 (инв. 6/7, ожидание смерти) — закрыт, мутация красная.
  - П. 21 (READINESS по букве) — снова закрыт.
  - П. 15 (время набора) — рост нет.
  - Остальные пункты закрыты ревью 67/68 и не затронуты.
- **Условия владельца для `complete`** (`OWNER-RULES` §8, §9):
  - 27–34 зелёные в `acceptance-ci`;
  - критерий 20 — оба демо нового вида вживую на коде цикла, демо 2 два раза подряд, `-example` с датой;
  - старых демо в дереве нет; старые ключи остались только в подсказках `config/files.rs` и `docs/design/config.md`.

## Нарушения правил

Нет.

## Следующий шаг

Критерии выпуска выполнены. Если цикл продолжится, это ~60–100k контекста, без живых моделей и без полного прогона:
1. жел. 2 — вынести части из `aae-acceptance/src/main.rs` и `scenarios/mod.rs`;
2. жел. 1 — `abort` и убийство группы `run.sh` по пределу сценария; тест на сценарий, превысивший предел;
3. жел. 3 и 4 — документация о запросах claude под `bypassPermissions`; корень репозитория для READINESS во время
   запуска.

Правка `crates/` (п. 2 и 4) сдвинет «последнюю правку `crates/`», и строка 22 потребует переоценки READINESS по новому
полному живому прогону. Поэтому брать эти пункты стоит только вместе с бюджетом на прогон (~45 мин).
