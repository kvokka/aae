# WP-90 · план: движок отдельно от контрактов ядра

Автор — expert, цикл 107, HEAD `ffa27dc`. Код не менялся. Всё ниже проверено grep и сборкой на этом HEAD.

## 0. Решения

- Имя и роли по умолчанию остаются: нижний крейт `aae-contract` (контракты), `aae-core` — движок и реэкспорт.
  Имя — вопрос владельцу, запись в журнале уже сделана (expert, 2026-11-17).
- Граница шире, чем в тексте пакета, на три вещи (без них не собирается или рвётся тест):
  1. `cron` и `template` уходят вниз: на них стоит `config` (`config/mod.rs:18,212`, `config/items.rs:11`).
  2. `rfc3339_ms` и `parse_rfc3339_ms` уходят вниз: их берёт `aae-plugins` (`crates/aae-plugins/src/policy.rs:5`).
     В тексте пакета этого потребителя нет.
  3. `envelope` и `SCHEMA` уходят вниз вместе с ними: это раздел 1.1 `docs/design/contracts.md`, и тест
     `tests::template::a_loaded_where_on_the_run_view_matches_a_journal_envelope` тогда переезжает целиком.
- Два коммита. Тестов 548, как сейчас; имена отличаются только пакетом, без исключений.
- Замер «до» снят (раздел 6), повторять его не нужно.

## 1. Граница

| Уходит в `crates/aae-contract/src/` | Остаётся в `crates/aae-core/src/` |
|---|---|
| `config/` (13 файлов) | `run/` (целиком, с `run/exec_step/tests.rs`) |
| `driver.rs`, `driver/` (с `child/tests.rs`) | `writer.rs`, `store.rs`, `effects.rs`, `clock.rs` |
| `tracker.rs` | `journal.rs` без четырёх элементов из строки ниже |
| `orphans.rs`, `orphans/tests.rs` | `process.rs`, `process/tests.rs` (нужен только `run/`) |
| `cron.rs`, `template.rs` | `version.rs` (нужен только `aae`: `daemon.rs`, `doctor.rs`) |
| новый `event.rs`: `SCHEMA`, `envelope`, `rfc3339_ms`, `parse_rfc3339_ms` из `journal.rs:15-16,232-238,248-288` | |
| `BoxFuture` (из `writer.rs:27`) — в `lib.rs` | |
| `MAX_ATTEMPTS` (из `run/mod.rs:46-47`) — в `config/mod.rs` | |

Что берут нижние потребители (grep `aae_core` по `src` и `tests`), всё попадает вниз:

| Крейт | Берёт |
|---|---|
| `aae-claude` | `BoxFuture`, `config::{EffectiveConfig, Hook, Layer, Mcp, McpScope, Skill, Sourced, duration}`, `driver::*`, `driver::child`, `orphans::{start_time, waited}` |
| `aae-codex` | то же плюс `orphans::sweep_var` |
| `aae-br` | `BoxFuture`, `tracker::*`, `orphans::{output, output_std}` |
| `aae-plugins` | `orphans::spawn_std`, `parse_rfc3339_ms`, `rfc3339_ms`; `config::BUNDLED` — только в комментарии |
| `aae-testkit/src` | `BoxFuture`, `config::EffectiveConfig`, `driver::*`, `tracker::*` |

Обратные ссылки из уходящих модулей в движок — их три, других нет (grep `crate::` по уходящим файлам):

| Место | Ссылка | Как разрешается |
|---|---|---|
| `tracker.rs:5` | `crate::writer::BoxFuture` | `use crate::BoxFuture;` — тип теперь в корне `aae-contract` |
| `driver.rs:5` | `crate::BoxFuture` | не меняется |
| `config/items.rs:174` | `crate::run::MAX_ATTEMPTS` | `super::MAX_ATTEMPTS`; константа с её doc-комментарием — в `config/mod.rs`, в `run/mod.rs` вместо неё `pub use crate::config::MAX_ATTEMPTS;` |

Ссылки тестов на движок — раздел 3.

## 2. Механика

### Перенос

```bash
mkdir -p crates/aae-contract/src/tests
for p in config driver.rs driver tracker.rs orphans.rs orphans cron.rs template.rs; do
  git mv crates/aae-core/src/$p crates/aae-contract/src/$p
done
for t in config config_builtin config_launch config_layers config_prompt config_root config_services \
         config_sources config_steps config_strict config_triggers cron template; do
  git mv crates/aae-core/src/tests/$t.rs crates/aae-contract/src/tests/$t.rs
done
```

### Новые файлы

`crates/aae-contract/Cargo.toml` — шапка как у `aae-core` (без `publish = false`: от него зависят публикуемые
крейты), `description = "Contracts of the core of aae: the config, the driver and tracker interfaces, the event envelope"`,
`[lints] workspace = true`. Строки зависимостей копируются из `aae-core/Cargo.toml` дословно (версии те же,
`Cargo.lock` получает только запись о новом пакете):

- `[dependencies]`: `jiff`, `rustix`, `serde`, `serde_json`, `sha2`, `toml`, `uuid` — как есть;
  `tokio` с `features = ["rt", "sync", "time", "process", "io-util"]`.
- `[dev-dependencies]`: `tempfile`; `tokio` с `features = ["rt", "macros", "time", "net"]` (`net` — только
  `orphans/tests.rs:61`).
- Не нужны: `getrandom`, `rusqlite`.

Набор features проверить один раз: `cargo check -p aae-contract --all-targets` (собирает крейт в одиночку, без
объединения features с остальными). Не хватает — добавить недостающее, не весь список.

`crates/aae-contract/src/lib.rs`:

```rust
//! The contracts of the core of aae (docs/design/contracts.md): the config, the driver and tracker interfaces, the
//! event envelope and the registry of child processes. The engine over them is `aae-core`.

#![deny(clippy::disallowed_methods)]

pub mod config;
pub mod cron;
pub mod driver;
mod event;
pub mod orphans;
pub mod template;
pub mod tracker;

pub use event::{SCHEMA, envelope, parse_rfc3339_ms, rfc3339_ms};

pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(test)]
mod tests;
```

`crates/aae-contract/src/event.rs` — четыре элемента из `journal.rs` дословно, с их комментариями; `envelope`
становится `pub`. Заголовок: `//! The event envelope (docs/design/contracts.md 1.1): its schema version and the form
of its timestamps.`

`crates/aae-contract/src/tests/mod.rs` — 14 строк `mod`: 13 перенесённых и `journal`.

### `aae-core`

`Cargo.toml`: добавить `aae-contract = { version = "0.1.0", path = "../aae-contract" }`, убрать `toml` (в движке
не используется, `cargo machete` упадёт). Остальное не трогать.

`src/lib.rs`:

```rust
mod clock;
mod effects;
mod journal;
mod process;
pub mod run;
mod store;
mod version;
mod writer;

pub use aae_contract::{BoxFuture, config, driver, orphans, template, tracker};
pub use clock::Clock;
pub use journal::{Journal, JournalHealth, ReadError, SCHEMA, parse_rfc3339_ms, rfc3339_ms};
pub use store::open;
pub use version::tested_version;
pub use writer::{CoreHandle, Error, Event, Executor, Handler, Tx, start};
```

`cron` не реэкспортируется: `aae_core::cron` и `crate::cron` никто не берёт (проверено grep). Пути `crate::config::…`,
`crate::driver::…`, `crate::orphans::…`, `crate::template::…`, `crate::tracker::…` в `run/` продолжают работать
через реэкспорт — файлы `run/` импортов не меняют.

Правки в движке, все точечные:

| Файл | Правка |
|---|---|
| `journal.rs` | убрать четыре элемента; добавить `pub use aae_contract::{SCHEMA, parse_rfc3339_ms, rfc3339_ms};` и `use aae_contract::envelope;`. Девять мест `crate::journal::rfc3339_ms` в `run/` остаются как есть |
| `writer.rs:27` | убрать `pub type BoxFuture`, добавить `BoxFuture` в `use crate::{…}`; лишние `future::Future`, `pin::Pin` убрать, если clippy назовёт |
| `writer.rs:106` | `crate::journal::envelope(…)` → `aae_contract::envelope(…)` |
| `run/mod.rs:46-47` | константа → `pub use crate::config::MAX_ATTEMPTS;` |
| `tests/mod.rs` | остаются `mod config_triggers; mod effect_retry; mod idempotency; mod journal; mod process;` |

### Видимость: что стало пересекать границу

`pub(crate)` → `pub`, ровно пять функций и одна новая:

| Элемент | Кто берёт из движка |
|---|---|
| `config::input_variable` (`config/mod.rs:314`) | `run/exec_step.rs:60` |
| `config::upper` (`config/mod.rs:319`) | `run/exec_step.rs:66` |
| `orphans::sweep` (`orphans.rs:181`) | `process.rs:213`, `run/exec.rs:116`, `run/service_proc.rs:248` |
| `orphans::sweep_all` (`orphans.rs:186`) | `run/exec.rs:83`, `run/recover.rs:39` |
| `orphans::listens_alone` (`orphans.rs:232` и `:255`, обе cfg-ветки) | `run/service_proc.rs:198` |
| `envelope` (было `pub(crate)` в `journal.rs:232`) | `journal.rs:128`, `writer.rs:106` |

Остаются `pub(crate)`: `config::schema` с `event_path`, `bead_path`, `run_path` (берут только `config/` и
переехавший `tests/template.rs`), `orphans::reap`. `Clock::instant_at` остаётся в движке. `unused-pub.sh` ищет по
имени — все шесть имён используются в `aae-core`.

### Кто меняет зависимость и импорты

`aae-claude`, `aae-codex`, `aae-br`, `aae-plugins`: в `Cargo.toml` строка `aae-core` → `aae-contract`.
`aae-testkit`: `[dependencies]` `aae-core` → `aae-contract`, в `[dev-dependencies]` добавить `aae-core` (его берут
`tests/core`, `tests/chaos`, `tests/common`).

```bash
grep -rl aae_core crates/aae-claude crates/aae-codex crates/aae-br crates/aae-plugins crates/aae-testkit/src \
  | xargs sed -i 's/aae_core/aae_contract/g'
cargo fmt --all
```

`crates/aae-testkit/tests/` не трогать. Не меняются вовсе: `aae`, `aae-front`, `aae-acceptance` (кроме фильтров
`checks.rs`).

## 3. Тесты

Сейчас: 548 тестов, 15 бинарей, в `aae-core` — 128 (`cargo nextest list --locked --workspace -T oneline`; тот же
список у кодера в `notes/tests-before.txt`). После: `aae-contract` — 93, `aae-core` — 35, остальные без изменений.

| Куда | Файлы | Тестов |
|---|---|---|
| `aae-contract` | `tests/config*.rs` (11 файлов, без одного теста), `tests/cron.rs`, `tests/template.rs`, `orphans/tests.rs`, `driver/child/tests.rs`, новый `tests/journal.rs` | 70 + 6 + 9 + 6 + 1 + 1 = 93 |
| `aae-core` | `tests/mod.rs`, `tests/journal.rs` (7 из 8), `tests/effect_retry.rs`, `tests/idempotency.rs`, `tests/process.rs`, `process/tests.rs`, `run/exec_step/tests.rs`, `version.rs`, новый `tests/config_triggers.rs` | 6 + 7 + 4 + 4 + 1 + 9 + 2 + 1 + 1 = 35 |

Три теста стоят на границе:

1. `tests::journal::timestamps_are_rfc3339_utc_with_milliseconds` (`tests/journal.rs:79-101`) проверяет функции,
   ушедшие вниз. Переезжает в новый `crates/aae-contract/src/tests/journal.rs` — путь модуля тот же. `T0` там своя
   константа (`1_800_000_000_000`), импорт — `use crate::{parse_rfc3339_ms as parse, rfc3339_ms};`.
2. `tests::template::a_loaded_where_on_the_run_view_matches_a_journal_envelope` (`tests/template.rs:136`) переезжает
   с файлом; одна правка: `crate::journal::envelope` → `crate::envelope`.
3. `tests::config_triggers::a_project_trigger_without_a_rig_runs_in_the_root` (`tests/config_triggers.rs:85-112`)
   зовёт `crate::open` и `crate::run::Reader` — это тест движка. Он остаётся в `aae-core`, в новом
   `src/tests/config_triggers.rs`, под тем же именем. Фикстура `BASE` уехала вниз и из другого крейта недоступна
   (`#[cfg(test)]`), поэтому тест получает свою, из пяти файлов. Оба варианта ниже загружены
   `aae config check` без ошибок:

   ```rust
   /// `shop` with a rig that lists the pack's schedule and a project trigger; `root` stands before the rig, `rig`
   /// ends the trigger.
   fn places(root: &str, rig: &str) -> Vec<String> {
       let project = format!(
           "packs = [\"../kit\"]\n{root}[rigs.backend]\npath = \"backend\"\ntriggers = [\"nightly\"]\n\
            [[trigger]]\nname = \"implement-next\"\non = \"bead.ready\"\nrun = \"audit\"\n{rig}"
       );
       let files = [
           ("config.toml", "projects = [\"shop\"]\n"),
           ("shop/aae.toml", project.as_str()),
           ("shop/backend/.keep", ""),
           ("kit/pack.toml", "name = \"kit\"\nversion = \"0.1.0\"\n[[trigger]]\nname = \"nightly\"\ncron = \"0 3 * * *\"\nrun = \"audit\"\n"),
           ("kit/formulas/audit.toml", "name = \"audit\"\n[[step]]\nkind = \"exec\"\ncommand = [\"true\"]\n"),
       ];
       // write the files, `crate::config::load_for(.., &[], Default::default())`, then as in the old test:
       // `crate::open`, `crate::run::Reader::open(..).triggers(&config)`, "{trigger} {rig}" sorted
   }
   ```

   Три проверки прежние: `places("", "")` → корень `root`; `places("[root]\nname = \"main\"\n", "")` → `main`;
   то же с `rig = "main"\n` → `main`. Ожидаемые строки — `"kit.nightly" "backend"` и `"shop.implement-next" "<корень>"`.
   Из переехавшего `tests/config_triggers.rs` этот тест удалить (там остаются 2).

### Фильтры `crates/aae-acceptance/src/checks.rs`

Все семь фильтров с `-p aae-core` выбирают из него тесты `tests::config_*`, которые уходят вниз:

| Критерий | Строка | Из `aae-core` сейчас | Правка |
|---|---|---|---|
| 27 | 133 | `config_launch` 5, `config_root` 3, `config_triggers` 1 (остаётся в движке) | добавить `-p aae-contract`, `-p aae-core` оставить |
| 32 | 151 | `config_layers` 1, `config_root` 5 | `-p aae-core` → `-p aae-contract` |
| 28 | 171 | `config_steps` 1 | то же |
| 33 | 188 | `config_launch` 1, `config_prompt` 5 | то же |
| 29 | 206 | `config_steps` 2 | то же |
| 30 | 228 | `config_steps` 1 | то же |
| 31 | 247 | `config_steps` 1 | то же |

Проверка без запуска сценариев (скрипт — во временный файл, не в репозиторий; сейчас печатает `12 12`, `12 12`,
`7 7`, `8 8`, `15 15`, `7 7`, `14 14`):

```bash
#!/usr/bin/env bash
# wanted count, listed count, head of the filter
set -euo pipefail; set -f
perl -0ne 'while (/"(-p aae-co(?:re|ntract)[^"]*)",\s*(\d+)/g) { ($f,$n)=($1,$2); $f =~ s/\\\n\s*//g; print "$n\t$f\n" }' \
  crates/aae-acceptance/src/checks.rs |
while IFS=$'\t' read -r want filter; do
  echo "$want $(cargo nextest list --locked $filter -T oneline 2>/dev/null | wc -l) ${filter:0:70}"
done
```

Запускать через `bash <файл>`: в zsh `$filter` не делится на слова.

## 4. Скрипты

| Файл | Правка |
|---|---|
| `scripts/core-boundary.sh:15` | `find crates/aae-contract/src crates/aae-core/src …`; комментарий в шапке и сообщение — «the core (`aae-contract`, `aae-core`)» |
| `scripts/select-biased.sh:7` | тот же `find` по двум каталогам, комментарий и сообщение — так же. Сейчас проверка покрывает `driver/`, `orphans.rs`; без правки они из-под неё выпадут |
| `scripts/budget.sh:21` | `crates/aae-core/* \| crates/aae-contract/*) part=core ;;` — иначе «belongs to no budget part» |
| `clippy.toml` | в комментарии и шести `reason` `aae_core::orphans` → `aae_contract::orphans`: у адаптеров пути `aae_core` больше нет |
| `Cargo.toml` (корень) | `"crates/aae-contract"` в `default-members` |

`scripts/unused-pub.sh`, `scripts/budget-selftest.sh`, `mise.toml`, `.github/` правок не требуют.

## 5. Документы

| Место | Что поменять |
|---|---|
| `docs/architecture.md:25-27` | ядро — два крейта: `aae-contract` (на нём пишутся адаптеры, адаптер трекера, плагины, тестовые двойники) и `aae-core` (движок над ним); ни один не называет харнесс и трекер, `scripts/core-boundary.sh` проверяет оба |
| `docs/architecture.md:39` | новая строка `aae-contract`: config assembly, the driver and tracker interfaces, the event envelope, templates and schedules, the registry of child processes; ссылки — contracts, config. Из строки `aae-core` убрать «config assembly», дописать, что он реэкспортирует `aae-contract`. В строках адаптеров, `aae-br`, `aae-plugins` зависимость от `aae-contract` назвать в строке `aae-contract`, не в каждой |
| `docs/design/adapters.md:16` | «depends on `aae-contract`» |
| `docs/design/adapters.md:20,35,90` | `aae_core::driver…` → `aae_contract::driver…` |
| `docs/design/contracts.md:865` | `aae_core::orphans` → `aae_contract::orphans` |
| `docs/guide/development.md:39` | «in the code of `aae-core`» → «of the core (`aae-contract`, `aae-core`)» |
| `docs/agents/rules-code.md:16` | «`tokio::select!` in `aae-core`» → «in the core (`aae-contract`, `aae-core`)» |
| `docs/agents/rules-code.md:26` | «the core (`aae-core`)» → «the core (`aae-contract`, `aae-core`)» |

Остаются верными и не меняются: `docs/design/event-model.md:9,11,26,56,69,87`, `docs/design/invariants.md:18`,
`docs/design/tracker-br.md:5` (называют файлы движка), `docs/README.md` (крейты не перечисляет).
`docs/agents/decisions.md:20` (решение 43, «`aae-core` names no harness») не трогать — файл владельца; запись в
журнале есть.

## 6. Порядок

Перед шагом 1 — ничего: список тестов и замер «до» уже есть.

**Коммит 1 — «WP-90: the contracts of the core in a crate of their own».** Раздел 2 без «Кто меняет зависимость»,
раздел 3 целиком (включая `checks.rs`), раздел 4 без `clippy.toml`, из раздела 5 — `architecture.md`,
`development.md`, `rules-code.md`. `Cargo.lock` — в коммит. Все зависимые крейты ещё берут `aae_core::…` через
реэкспорт, поэтому workspace собирается. Проверки шага:

- `cargo check -p aae-contract --all-targets 2>&1 | tail -20`
- `cargo clippy --workspace --all-targets -q --message-format short -- -D warnings 2>&1 | head -60`
- `cargo nextest run -p aae-contract -p aae-core --no-fail-fast --status-level fail --final-status-level fail 2>&1 | tail -30` — 93 и 35
- сравнение списков: `diff <(sed -E 's/^aae-core /C /' .loop/cycles/107/notes/tests-before.txt | sort) <(cargo nextest list --locked --workspace -T oneline 2>/dev/null | sed -E 's/^aae-(core|contract) /C /' | sort)` — пусто
- скрипт фильтров из раздела 3 — семь пар равных чисел
- `scripts/core-boundary.sh && scripts/select-biased.sh && scripts/budget.sh | tail -12`

**Коммит 2 — «WP-90: adapters, the tracker adapter, plugins and the testkit library build on aae-contract».**
«Кто меняет зависимость» из раздела 2, `clippy.toml`, `adapters.md`, `contracts.md:865`. Проверки шага:

- `for c in aae-claude aae-codex aae-br aae-plugins; do cargo tree -p $c -e normal,dev | grep -c 'aae-core' ; done` — четыре нуля; `cargo tree -p aae-testkit -e normal | grep -c aae-core` — ноль
- `scripts/budget.sh` (лимит 400 строк: `crates/aae-claude/tests/harness/contract.rs` сейчас ровно 400; строки, где
  стоит `aae_core`, после замены не длиннее 120, перенос не ожидается)
- clippy и точечные тесты `-p aae-claude -p aae-codex -p aae-br -p aae-plugins -p aae-testkit -E 'not binary(chaos)'`

**«Готово» пакета, на закоммиченном HEAD:**

1. `cargo nextest run --locked --no-run`, затем `touch crates/aae-core/src/run/mod.rs` и снова:
   `… 2>&1 | grep -E '^\s*Compiling'`. Ожидается ровно `aae-core`, `aae-testkit` (его тесты движка), `aae-front`,
   `aae`. Нет `aae-contract`, `aae-claude`, `aae-codex`, `aae-br`, `aae-plugins`. Вывод — в отчёт.
2. Список тестов и фильтры — команды шага 1 ещё раз.
3. `echo '// codex' >> crates/aae-contract/src/cron.rs; scripts/core-boundary.sh` — код 1 и имя файла;
   `git checkout -- crates/aae-contract/src/cron.rs`. То же для `crates/aae-core/src/clock.rs`.
4. Замер записи той же командой из `.loop/PLAN.md:32-37` с `touch crates/aae-core/src/run/mod.rs` вместо правки.
   «До»: 527 МБ, компилируются все восемь крейтов (`aae-br`, `aae-claude`, `aae-codex`, `aae-core`, `aae-front`,
   `aae-plugins`, `aae-testkit`, `aae`), 2026-11-17, `ffa27dc`.
   <!-- MEASURE id=wp90-engine-edit-before date=2026-11-17 ver=ffa27dc env=linux-arm64-orbstack src=.loop/cycles/107/notes/wp90-plan.md note="dev profile, warm target; touch crates/aae-core/src/run/mod.rs, then cargo nextest run --locked --no-run; sum of files in target newer than the marker; measure again after WP-90" -->
5. `scripts/loop/checks 107` (гейт) зелёный.

## 7. Риски

- **Орфанные правила — чисто.** В движке нет `impl` чужого трейта для типа контрактов и нет inherent-`impl` для них
  (grep `^\s*impl` по `run/`, `writer.rs`, `journal.rs`, `process.rs`): все `impl` — для `RunHandler`, `Reader`,
  `Tx`, `Error`, `Journal` и других своих типов. В контрактах нет ссылок на типы движка.
- **Макросы, feature-флаги, `include_str!`** — в `aae-core/src` их нет. `env!("CARGO_PKG_VERSION")` в
  `config/load.rs:272,276` (проверка `min_aae`) теперь берёт версию `aae-contract`: она та же, `version.workspace`.
  У нового крейта версию от workspace не отвязывать.
- **Реестр детей `orphans` — `static OWN`.** Один экземпляр на процесс, пока `aae-contract` в графе один. Второй
  версии крейта в графе быть не должно (у всех `path`-зависимость, так и оставить).
- **`#[cfg(test)]`-хелперы через границу не ходят.** Это причина своей фикстуры у теста 3 (раздел 3). Не делать
  фикстуру `pub` в продуктовом крейте и не подключать чужой файл через `#[path]`.
- **Features tokio.** В общей сборке они объединяются, и нехватку в `aae-contract` видно только в одиночной
  `cargo check -p aae-contract --all-targets`.
- **`cargo machete`**: `toml` в `aae-core` лишний; в `aae-contract` не нужны `rusqlite` и `getrandom`.
- **Лимит 400 строк** после `cargo fmt`: `config/items.rs` — 392 (правка не меняет число строк), `aae-br/src/lib.rs` —
  393, `aae-claude/tests/harness/contract.rs` — 400.
- **Реэкспорт и идентичность типов.** `aae_core::driver::Driver` и `aae_contract::driver::Driver` — один трейт,
  поэтому `ScriptedDriver` из тесткита (на `aae-contract`) подходит тестам `tests/core` (на `aae_core`).
- **Регресс выгоды.** Стоит кому-то вернуть `aae-core` в зависимости адаптера — выгода молча пропадёт. Проверки в CI
  на это нет и в пакете её не просили; предложение — в отчёт.
- **Выгода ограничена.** Из последних 300 коммитов 170 трогали движок, из них 101 — только движок; 116 трогали
  уходящие вниз модули (81 — `config/`), и такие правки по-прежнему пересобирают всё.

## 8. Не делаем

- Дробление `config` (типы отдельно от загрузчика) — «не входит» по тексту пакета.
- Правку `aae`, `aae-front`, `aae-acceptance`, `crates/aae-testkit/tests/` под `aae_contract::` — реэкспорт для того и нужен.
- Чистку features в `aae-core/Cargo.toml` — на сборку не влияет.
- CI-проверку «адаптер не зависит от `aae-core`».
