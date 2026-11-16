SCORE: 3.4/10
VERDICT: continue
BLOCKERS: 0

# Ревью цикла 107

Дифф `e92ddba..HEAD`: 3 коммита, 95 файлов, +471/−361. Почти весь объём — перенос модулей из `aae-core` в новый
крейт `aae-contract` без правок тел. Вне `.loop` дерево чистое. Незакоммичены только `JOURNAL.md`, `PLAN.md`,
`state.json` и `cycles/107/` — штатные файлы цикла.

`checks.md`: HEAD `2ccc4fb` совпадает с текущим, RESULT pass, 30 s (в цикле 106 — 28 s), nextest 538/538. Полный
гейт я не повторял.

## Обязательно до продолжения

Нет.

## Желательно

1. **Выгоду WP-90 ничто не сторожит** (находка кодера в журнале).
   - Если `aae-core` снова попадёт в зависимости адаптера, `aae-br`, `aae-plugins` или в normal-зависимости
     библиотеки `aae-testkit`, гейт останется зелёным, а пересборка молча вернётся.
   - Проверка — одна строка `cargo tree -e normal,dev` в `scripts/core-boundary.sh` или в `mise run ci`.
   - Пакет такой проверки не просил, поэтому решает владелец.
2. **`aae_contract::envelope` (`crates/aae-contract/src/event.rs:8`) стала `pub`, но без rustdoc.** У соседних
   `rfc3339_ms` и `parse_rfc3339_ms` он есть. Мелочь.
3. **Тесты, которые зовут git продукта, читают глобальный git config машины.** Это пункт Жел. 3 ревью 106, он
   по-прежнему открыт.
4. **Ждут владельца:**
   - имя `aae-contract` и текст решения 43 (`docs/agents/decisions.md:20`): решение называет только `aae-core`.
     Вопросы — в записи expert в журнале, цикл 107. Скрипт уже проверяет оба крейта, и решение кодер не трогал —
     так и должно быть;
   - вопрос о командах `CLAUDE.md` и `.claude/` после `default-members` (журнал, цикл 106);
   - открытые пункты «Желательно» ревью 105.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-89 | done | — (проверено в ревью 106) |
| WP-91 | done | — Жел. 1–2 ревью 106 закрыты: тест на 6 свежих репозиториях, довод про `hold` в `remove` — в rustdoc `hold` |
| WP-90 | done | — Все условия «готово» проверены мной (см. ниже) |
| WP-92…WP-101 | todo | — |

Статусы в `PLAN.md` правдивы.

## Проверено

- **Гейт.** `checks.md`: HEAD `2ccc4fb`, pass, 30 s, nextest 538/538.
- **Пересборка после правки движка.** Сделал `touch crates/aae-core/src/run/mod.rs`, затем
  `cargo nextest run --locked --no-run`:
  - компилируются только `aae-core`, `aae-front`, `aae-testkit`, `aae`;
  - записано 324 МБ — совпадает с отчётом;
  - `aae-contract`, `aae-claude`, `aae-codex`, `aae-br` и `aae-plugins` не пересобираются.
- **`cargo tree`.** Зависимости `normal,dev` у `aae-claude`, `aae-codex`, `aae-br`, `aae-plugins` и `normal` у
  `aae-testkit` — `aae-core` нет ни у одного.
- **Список тестов.** `cargo nextest list --locked --workspace` даёт 548 тестов. Я заменил пакет `aae-contract` на
  `aae-core` и сравнил через `diff` с `notes/tests-before.txt` — разницы нет.
- **Фильтры `checks.rs`.** Прогнал все восемь фильтров с `-p aae-co…` (и критерий 11) через `cargo nextest list`.
  Ожидаемое число совпадает с выбранным: 41/41, 12/12, 12/12, 7/7, 8/8, 15/15, 7/7, 14/14.
- **Мутации** (после каждой — откат через `git checkout`, `git status` чистый):
  - `// codex` в `crates/aae-contract/src/template.rs`: `scripts/core-boundary.sh` выходит с кодом 1 и называет
    файл и строку;
  - инвариант 16, потолок попыток: `let ceiling = u32::MAX` в `crates/aae-contract/src/config/items.rs:174`
    вместо `super::MAX_ATTEMPTS`. Тест `aae-contract tests::config_steps::a_bad_max_attempts_or_check_script_is_one_line`
    краснеет;
  - WP-91: убрал `hold` в `create` (`crates/aae-plugins/src/worktree.rs:75`) и прогнал тест
    `runs_of_one_repository_prepared_at_once_each_get_a_worktree` 8 раз — красный в 7 из 8 (в ревью 106 было 11 из
    20).
- **Перенесённый тест.** `a_project_trigger_without_a_rig_runs_in_the_root` остался в `aae-core` со своей
  фикстурой. Сверил со старой версией: три `assert_eq!` те же, и ожидаемые значения те же.
- **Флейки под нагрузкой.** 9 процессов `sha256sum` занимали 9 ядер.
  - `-p aae-contract -p aae-plugins -p aae-core` (134 теста) прогнал 10 раз — все прогоны зелёные;
  - `test(/worktree/)` по `aae-testkit` и `aae-plugins` прогнал 15 раз — все зелёные.
- **Вживую**, в `mktemp -d` с изолированными `HOME` и `XDG_*`, без модели:
  - `aae daemon` поднялся, `aae status` ответил;
  - `aae trigger list` показал cron-триггер с верным `next_at` в зоне системы;
  - `aae run start hello check` → `run_1 passed`;
  - `aae events --from 1` выдал полную цепочку событий от `queued` до `passed`, с конвертом и `seq`;
  - `aae drain` остановил демон.
- **Документы сверены с кодом:** `architecture.md` (границы, таблица крейтов), `adapters.md`, `contracts.md:865`,
  `development.md`, `rules-code.md`, `clippy.toml`.
  - Устаревших путей `crates/aae-core/src/{config,driver,orphans,tracker,template,cron}` в `docs/` и в коде нет.
  - Оставшиеся упоминания `aae-core` указывают на файлы движка, и это верно.
  - `decisions.md` и `assumptions.md` не тронуты.
- **Код, язык, зависимости:**
  - в корне `aae-contract` есть `#![deny(clippy::disallowed_methods)]`;
  - `cargo machete` ничего не нашёл;
  - внешних зависимостей не добавлено: в `Cargo.lock` только новый пакет `aae-contract`;
  - русского в `crates/`, `docs/`, `scripts/` и в сообщениях коммитов нет, `todo!`, `unimplemented!`, `dbg!` не
    добавлено.
- **Строки** (`scripts/budget.sh`): ядро 11034 / 18000, всего 17644 / 30000. Самый большой файл —
  `aae-contract/src/config/files.rs`, 398 строк, перенесён без правок.

## Нарушения правил

- Размер цикла: один M (WP-90) и S-доделка WP-91 на ~10k. Ревью 106 явно предложило эту доделку отдельным коммитом
  в начале цикла, контекст ~120k. Нарушением не считаю.

## Следующий шаг

Блок «меньше пересборки» (WP-87…WP-91) закрыт. Дальше — блок watchdog, по порядку плана: WP-92 и WP-93 (оба S),
вдвоём в один цикл. Затем WP-94 (M, критичный, начинать с плана expert) — один в цикле.
