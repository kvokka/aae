SCORE: 6.6/10
VERDICT: continue
BLOCKERS: 1

# Ревью цикла 113

Дифф `7be3d92..f73ab51`: 2 коммита кодера, 9 файлов, +289/−2. Это добивка WP-99 (Жел. 1 и 2 ревью 112) и WP-101.
Вне `.loop` дерево чистое. В индексе застейджено удаление кода стенда WP-99 из `.loop/cycles/112/notes/` (Жел. 3
ревью 112), его закоммитит `loopctl` вместе с `.loop`, так что это нормально.

`checks.md` снят на текущем HEAD `f73ab51`: pass, 35 s, nextest 559/559. Гейт я не перезапускал.

## Обязательно до продолжения

1. **Мутация пережила охрану `!self.open` в новой ветке адаптера claude** (`crates/aae-claude/src/stream.rs:146`).
   Если условие заменить на `if self.agent && !out.is_empty() && …`, весь `aae-claude` остаётся зелёным (41/41).
   Без этой охраны `api_retry`, `rate_limit_event` или `permission_denied` **внутри хода** при живом `local_agent`
   выдают `State{Idle, background:true}`. Такое бывает в ходе пробуждения после `init` или в следующем промпте, пока
   субагент жив, потому что `self.agent` при `result` не сбрасывается. Ядро на это вызывает `stop_background` и
   взводит `background_silence` (120 min) вместо `silence` (15 min) посреди хода
   (`crates/aae-core/src/run/events.rs`, ветка `State{Idle, background:true}`). Это задевает инвариант 15 (бюджеты
   watchdog). Сам код верен, но охрана ничем не закреплена. Нужно дописать в
   `a_background_subagent_keeps_the_session_idle_with_background…` (`crates/aae-claude/src/stream/tests.rs`) шаг:
   после `init` (ход открыт) при живом `local_agent` строка `api_retry` даёт ровно `[Retrying]`, без `State`.
   Проверка: с мутацией `!self.open` → `true` тест должен покраснеть.

## Желательно

1. **Мутация, удаляющая `&& !matches!(out.last(), Some(Normalized::State { .. }))`, тоже выживает.** Тогда после
   `result` при живом `local_agent` уходит второй `State{Idle, background:true}` подряд. Вред почти нулевой:
   `stop_background` дедуплицируется ключом, таймер тот же. Но строгий тест на число событий после `result`
   (`events.len()`) закрепил бы охрану одной строкой.
2. **В подсказках скилла по видам отказа нет `adapter_crash`** (`harness-plugin/skills/recover/SKILL.md`, раздел 1;
   enum — `crates/aae-contract/src/driver/failure.rs`). Вид `hollow_done` покрыт строкой про класс. По смыслу
   `adapter_crash` подходит к группе «the session may be fine to continue».
3. **Проверка команд в тексте скилла сверяет только имя команды, но не флаги** (`harness_plugin.rs`, второй цикл).
   Например, флаг `--parent` в «`aae run list --parent`» не проверяется. Сейчас все такие флаги есть и в
   `sh`-блоках, где разбор полный, так что это не дыра. Но со скиллом WP-108 вероятность ошибки растёт.
4. **Флейк вне диффа.** `aae-testkit::chaos a_seed_replays_the_same` упал один раз под нагрузкой: я по ошибке
   запустил полный `cargo nextest run --workspace` вместе с chaos, а рядом крутились точечные прогоны. Сообщение:
   «seed 4458174239967525156: the steps differ», шаг `decide run_5@101` против `@131`. Сид «повторяется
   одинаково» только без нагрузки, то есть детерминизм стенда зависит от времени. Chaos в гейт не входит, этот цикл
   его не трогал. Записано в журнал, чтобы план решил, брать ли это в работу.
5. Перенесено из ревью 112: Жел. 4 (путь `held` в codex без теста) и Жел. 5 (`class` в текстовом `run list`).

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-98 | done | — |
| WP-99 | done | Жел. 1–3 ревью 112 закрыты: внеходовые `Retrying`/`Usage`/`PermissionDenied` дополнены `Idle{background}`, мутация `local_agent → !tasks.is_empty()` краснит тест, стенд удалён из `notes/`. Но охрана `!self.open` новой ветки не закреплена тестом (Обяз. 1) |
| WP-100 | done | — |
| WP-101 | done | Плагин `harness-plugin/` и маркетплейс `.claude-plugin/marketplace.json`, скилл `recover`, 2 теста в гейте, раздел в `guide/recovery.md` и абзац в `architecture.md`. Codex читает манифесты Claude (совместимый формат), это записано в журнал. Жел. 2–3 |
| WP-102…WP-108 | todo | — |

Закрыто 15 пакетов из 22.

## Проверено

- **Гейт.** Взят из `checks.md`: HEAD `f73ab51` совпадает с текущим, pass, 35 s (в цикле 112 было 36 s), nextest
  559/559 (было 557, прибавились 2 теста плагина). `scripts/budget.sh`: ядро 10930 / 18000, claude 973 / 3500,
  frontends 2669 / 4000, всего 17534 / 30000. Самые большие файлы диффа: `stream.rs` 300 строк, `stream/tests.rs`
  304, оба ≤ 400. `todo!`/`unimplemented!` нет, новых зависимостей нет. Русского нет ни в диффе продукта, ни в
  сообщениях коммитов.
- **Мутации.** Каждую откатывал через `git checkout`, после каждой проверял `git status`.
  - Новая ветка выключена (`if false && …`): краснеет `a_background_subagent_keeps…`.
  - `self.agent` → `self.background` в новой ветке: краснеет.
  - `self.agent = !tasks.is_empty()`: краснеет (Жел. 2 ревью 112 закрыт).
  - Без `!self.open`: **выжила** (Обяз. 1).
  - Без охраны «последнее — не `State`»: выжила (Жел. 1).
  - Мутации скилла и манифестов краснят тесты плагина:
    - `--continue-from` → `--continue`;
    - «`aae run list --parent`» → «`aae run children`»;
    - `--timeout` → `--deadline`;
    - имя в маркетплейсе;
    - `version` 0.1.0 → 0.1.1.
- **Флейки.** Затронутые тесты (subagent и 2 теста `harness_plugin`) прогнаны 10 раз подряд рядом с полным
  `cargo nextest run --workspace`: 10/10 зелёные. Флейк chaos вне диффа описан в Жел. 4.
- **Вживую, без модели.** В изолированных `HOME`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME` (`/tmp/aae-rev113.*`):
  - `claude plugin validate .` и `claude plugin validate harness-plugin` проходят («Validation passed»);
  - `claude plugin marketplace add <repo>` + `plugin install aae@aae` ставят плагин, в кэше лежат
    `aae/aae/0.1.0/skills/recover/SKILL.md`;
  - `codex plugin marketplace add <repo>` + `codex plugin add aae@aae` ставят плагин; в кэше codex только
    `.claude-plugin` и `skills`, без всего репозитория.

  Отдельно поднял демон с изолированными `XDG_*` (`/tmp/aae-rev113d.*`) и прошёл команды чтения из скилла:
  `run list --state failed [--limit 20] --json`, `gate list --json`, `formula list --json`,
  `run list --parent … --json`, `run show … --json`, `run cancel … --reason`. Все разбираются CLI и отвечают демону
  ожидаемо (`[]` и 404 на несуществующий прогон). Путь восстановления на упавшем прогоне ревью 112 уже проходило.
  Демон остановлен.
- **Документы и код.** Строка в contracts 4.6 совпадает с кодом адаптера. Раздел установки в `guide/recovery.md` и
  абзац в `architecture.md` совпадают с файлами. Тексты скилла сверены с `guide/recovery.md`: состояния для
  `run cancel`, `bead_failures` = 3 (`design/config.md:303`), классы отказа. Ни одно утверждение `docs/` не
  ослаблено, истории нет. Ссылка `MEASURE` в `experiments.md` указывает на `wp99-stand.md`, а он не удалён.
- Решения владельца (122, 125, 54) соблюдены, допущения не менялись.

## Нарушения правил

В продукте нарушений нет. Одна выжившая мутация на инварианте 15 (Обяз. 1). С моей стороны: chaos-стенд я запустил
по ошибке, вне гейта (см. Жел. 4).

## Следующий шаг

Сначала закрыть Обяз. 1. Это одна-две строки теста в `crates/aae-claude/src/stream/tests.rs` и проверка мутацией,
около 10k контекста. Заодно можно закрыть Жел. 1 (`events.len()` после `result`) и Жел. 2 (`adapter_crash` в
скилле). После этого брать WP-102 (`await`, M, критичный, с планом expert до кода) отдельным циклом с полным
бюджетом: он задевает конфиг, движок, API, CLI и документы.
