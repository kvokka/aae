# WP-103 · условие шага `if` и корень `back_from` — план реализации (expert, цикл 115)

Источник: `.loop/PLAN.md:432-452`, решение 128 (и 126 — ряд проверок), contracts §1.3, §2.1–2.3, §6.1–6.2, config.md
«Formula», invariants.md «Recovery». Номера строк — на HEAD `2f1d7a2`.

## 0. Суть в шести строках

- `if` — тот же `Filter` (`aae-contract/src/template.rs:84-118`), что у `where` триггера: новых матчеров и языка нет.
  Хранится в `Step.condition: Filter` (пустой = совпадает всегда, `Option` не нужен).
- Пропуск решается в одной точке — `render` (`aae-core/src/run/render.rs:22`), единственный вызов которого стоит в
  `render_and_prepare` (`machine.rs:42`): туда приходят и первый шаг, и следующий, и цель `back_to`. Пропуск — не
  состояние: `skip → advance → enter → render_and_prepare` следующего шага идут в той же транзакции.
- Две новые колонки, версия схемы 23 → 24 (миграций до релиза нет, `store.rs:4-6`): `runs.back_from` (JSON-массив имён
  проверок, пишется тем же `UPDATE`, что `round`/`remarks`) и `attempts.skipped` (старый исход шага больше не его).
- Попытки у пропущенного шага нет (`runs.attempt` не растёт), поэтому «исход прогона — исход последнего выполненного»
  получается сам: `terminal` читает исход `row.attempt` (`finish.rs:89`).
- Правок существующей логики пять, все мелкие: тип результата `render`, два `AND skipped = 0`, запись `back_from`,
  охрана `run.step_finished` в `enter`, охрана последней ветки `advance`.
- Объём: контракт ~+60 строк, ядро ~+40, фронт +1; тесты ~+330; документы ~+45. Бюджет строк не риск; риск — лимит
  400 на файл: `machine.rs` 397 (+1), `items.rs` 385 (+8), `cli/mod.rs` 380 (+1) — см. §7.

## 1. Модель формулы и загрузка (`aae-contract`)

| Файл:строка | Правка |
|---|---|
| `config/formula_file.rs:49-102` | в каждый из пяти вариантов `StepFile`: `#[serde(rename = "if", default)] condition: serde_json::Map<String, serde_json::Value>` (как `filter` у `TriggerFile`, `files.rs:320`). `deny_unknown_fields` остаётся |
| `formula_file.rs` (после enum) | `impl StepFile { pub(super) fn condition(&self) -> &Map<..> }` — один исчерпывающий `match` с `\|` по пяти вариантам. Нужен, чтобы не трогать большой `match` в `items.rs:122-150` (лимит строк) |
| `formula_file.rs:144-156` | `Step` + `pub condition: Filter` с rustdoc: «`if`: the step runs only when every entry matches; empty: always» |
| `template.rs:107` | `Filter::is_empty(&self) -> bool` и `Filter::accepted(&self, path: &str) -> impl Iterator<Item = &Value>` (значения матчера по пути; для проверки имён `back_from`) |
| `config/prompt.rs:122` | `path(path, inputs, before, optional: bool)`: ветка `"steps"` — `field && before.iter().any(\|s\| s.name == first && (optional \|\| s.condition.is_empty()))`. Вызовы: шаблон (`prompt.rs:63`) и `step_inputs` (`:153`) — `false`; `step_if` — `true`. Это и есть «ссылка на шаг с `if` из шаблона или `inputs` шага `formula` — ошибка загрузки»: существующие тексты ошибок (`is not a value of the runs of`, `names nothing a run has`) |
| `config/prompt.rs` (новое, ~14 строк) | `pub(super) fn step_if(raw: &Map, inputs, before: &[Step]) -> Result<Filter, String>`: `Filter::parse(raw)` (синтаксис пути, типы матчеров — отрицания и выражений там нет); затем каждый путь: `p == "back_from"` либо корень из `["inputs", "bead", "rig", "run", "project", "steps"]` и `path(p, inputs, before, true)`; иначе ``Err(format!("`if`: a run has no `{bad}`"))``. `remarks`, `agent`, чужой корень, `back_from.x` — ошибка. Путь, кончающийся на объекте (`run`, `bead`, `inputs`, `steps.a`), `path` отвергает уже сейчас |
| `config/prompt.rs` (новое, ~10 строк) | `pub(super) fn strangers(steps: &[Step]) -> impl Iterator<Item = String>`: для каждого шага — значения `condition.accepted("back_from")`, которые не равны имени ни одного шага с `back_to`: ``step `fix`: `if` waits for `back_from` "reviw", which is no check of the formula``. Проверки могут стоять позже шага, поэтому проверка — после цикла |
| `config/items.rs:180` | перед `self.step(..)`: `let condition = super::prompt::step_if(step.condition(), &raw.inputs, &steps).map_err(\|e\| self.fail(file, format!("step `{name}`: {e}"))).ok()?;` и поле в `Step { .. }` (`:181`) |
| `config/items.rs:188` | после цикла: `for bad in super::prompt::strangers(&steps) { self.fail(file, bad); }` (собрать в `Vec` до вызова `fail`, если мешает заём) |
| `config/schema.rs:113` | `"run.step_skipped" => &[("run", RUN), ("step", Leaf)],` |

`steps` в `if` — только более ранние шаги (`before = &steps` на момент разбора шага), включая шаги со своим `if`:
у пропущенного `steps.<имя>` нет → запись не совпадает (`Filter::matches`, `None => false`). `back_from` — массив,
матчер по массиву уже «содержит» (`template.rs:113`): `if = { back_from = "review" }` и `back_from = ["a", "b"]`
работают без кода. Пути с точкой в TOML — в кавычках, как у `where`: `if = { "inputs.deploy" = true }`; без кавычек
получится вложенная таблица и ошибка `a matcher is a string, integer or boolean…` — годится. Входы приводятся к типу
на старте (`queue.rs:128-129`), так что `true`/`3` из CLI совпадают с булевым/целым матчером.

`aae config explain` не трогать (не просили). Конструктор `Step { .. }` один — `items.rs:181`.

## 2. `back_from`: откуда и как переживает рестарт

Сейчас от возврата в БД остаются только `runs.round` и `runs.remarks` (склеенный текст); имён проверок нет, из
`attempts` их не вывести (в новом раунде проверки получают новые исходы), журнал событий — не состояние.

- `store.rs:78-81`: `back_from TEXT` в `runs` (+ фраза в комментарии `:43`), `skipped INTEGER NOT NULL DEFAULT 0` в
  `attempts` (+ фраза в комментарии `:85-87`), `VERSION = 24`.
- `finish.rs:139-168` (`advance`): `sent_back` несёт ещё имя проверки; `UPDATE runs SET round = round + 1, remarks = ?2,
  back_from = ?3` с `json!(names).to_string()`. Порядок имён — порядок формулы. Каждый возврат перезаписывает
  колонку: в раунде видны только те проверки, что вернули именно в него. В первом раунде `NULL` → `[]`.
- `render.rs` (рядом с `remarks()`, `:159`): `fn back_from(conn, run) -> rusqlite::Result<Value>` —
  `SELECT COALESCE(back_from, '[]')`, `serde_json::from_str(..).unwrap_or_default()`.

Recovery: `back_from` коммитится той же транзакцией, что `round` и вход в целевой шаг; `if` вычисляется один раз на
входе в шаг в транзакции входа, результат (пропуск или `run.preparing`) коммитится с ней же. После рестарта `render`
не вызывается ни для какого состояния (`recover.rs`: `preparing` ждёт клейм или решение хуков, `rearm` только двигает
дедлайн), значит `if` не пересчитывается, а прогон никогда не стоит «на пропущенном шаге» в нетерминальном состоянии.
Текст invariants.md «Recovery» остаётся верным, править не надо.

## 3. Ядро: точка пропуска и что вокруг

`render.rs:22-130` (170 → ~185 строк):
- результат `Result<Option<(Option<String>, i64, Value)>, String>`; `Ok(None)` — шаг пропущен; пять `return Ok((..))`
  → `Ok(Some((..)))`;
- `step` — весь `&Step` (сейчас `&s.kind`), `rig_view` (`:106-109`) поднять наверх; контекст строится один раз до
  `match`: `inputs`, `rig`, `run` (`row.view()`), `bead`, `project`, `steps` (`step_results`), `back_from`. Ветка
  `Formula` (`:72-77`) берёт его же (лишние ключи безвредны: корни ограничивает `parse_step`), ветка `ai` дописывает
  `context["remarks"]` и `context["agent"]`. Дублирование двух `json!` уходит;
- сразу после контекста: `if row.step_attempt == 1 && !step.condition.matches(&context) { return Ok(None); }` с
  комментарием «a repeat of the step does not ask again». Проверка стоит до рендера вида шага: пропущенный ai-шаг не
  падает на неразрешённом шаблоне;
- `step_results` (`:147`): в `WHERE` добавить `AND skipped = 0`. Этой одной функцией закрыты `if`, шаблон, `inputs`
  шага `formula`, `AAE_STEP_<NAME>_*` у exec и check-скрипта (`exec_step.rs:65`) и `steps` в `GET /v1/runs/{id}`
  (`read.rs:70`).

`machine.rs:43-46` (397 → 398, больше туда ничего): `Ok(Some(p)) => p, Ok(None) => return self.skip(tx, row),`.

`finish.rs` (268 → ~290), новая функция рядом с `enter`:

```rust
/// The `if` of the step the run entered does not hold (docs/design/config.md): no hook is asked, no attempt starts,
/// and what the step left in an earlier round is no longer its outcome.
pub(super) fn skip(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
    tx.sql().execute("UPDATE attempts SET skipped = 1 WHERE run = ?1 AND step = ?2", params![row.id, row.step])?;
    row.emit(tx, "run.step_skipped", json!({ "run": row.view(), "step": row.step }))?;
    self.advance(tx, row, None)
}
```

Правки в `advance`/`enter`:

| Файл:строка | Правка | Зачем |
|---|---|---|
| `finish.rs:144` | запрос исхода проверки: `AND skipped = 0` | пропущенная проверка со старым `fail` из прошлого раунда никого не возвращает |
| `finish.rs:152,166` | имя проверки в `sent_back`, `back_from = ?3` | §2 |
| `finish.rs:172-175` | перед `None => passed`: `None if !Self::outcome(tx, &row)?.is_some_and(\|o\| o.result == Verdict::Pass) => self.terminal(tx, row, "failed", Some("infrastructure"), "every step was skipped"),` | пропущены все шаги (`row.attempt == 0`, исхода нет) — см. ниже |
| `finish.rs:192-198` | `run.step_finished` — только `if row.state != "preparing"` (комментарий: «a skipped step, still `preparing`, finished nothing») | из выполненного шага `enter` зовётся всегда в `finishing` (`check`, `check_done`, `repeat`, `recover`), из пропущенного — в `preparing` |
| `finish.rs:208` | `row.set_state(tx, "preparing", None)` — под тем же условием | иначе событие `run.state_changed {preparing → preparing}` |

Поведение, которое получается без кода (проверено чтением):
- Хуки `run.preparing` не зовутся: `skip` возвращается раньше `deciding` (`machine.rs:56`). Попытки нет, слот агента
  не занимается, `run.awaiting`/ребёнок `formula`/раздача `drain` не начинаются.
- Исход прогона при пропущенном последнем шаге: `terminal(passed)` → `Self::outcome` по `row.attempt` = последняя
  выполненная попытка. `RunView.step` в `run.passed` — имя пропущенного последнего шага (прогон дошёл до него).
- Первый шаг: `run.started` уже вышел в `prepare`; пропуск идёт через тот же `enter`.
- `back_to` на шаг с `if`: `enter(цель)` → `render` → при несовпадении `skip` → дальше по порядку. Неявного пропуска
  цели нет, прыжков `if` не даёт.
- Ряд проверок (решение 126) — статический, по тексту формулы: `row_ends` (`finish.rs:137`) не смотрит на `if`.
  Пропущенный `await` всё равно закрывает ряд перед собой; пропущенная проверка в середине ряда остаётся его членом
  без исхода; пропущенный шаг без `back_to` ряд заканчивает. `advance` от пропущенной проверки собирает ряд как
  обычно (`checks` включает её саму) и, если ряд кончился, возвращает прогон по упавшим выполненным.
- Конечность: возврат требует непропущенного `fail` и тратит раунд; пропуск снимает старый `fail`. Рекурсия
  `render_and_prepare → skip → advance → enter` ограничена `rounds × число шагов`, всё в одной транзакции.
- Повтор под `max_attempts` (`enter(.., Some(again))`, `step_attempt ≥ 2`) `if` не спрашивает: иначе
  `if = { "run.step_attempt" = 1 }` молча пропускал бы упавший шаг при повторе.

**Пропущены все шаги** — выбор: `failed{infrastructure}`, причина `every step was skipped`, исход `null`. Обоснование:
`run.passed` обещает `Outcome` (contracts §1.3), а `passed` закрыл бы бид без единого выполненного шага (тихое
«готово» — худший класс ошибки оркестратора); прецедент класса — `prompt: empty after substitution`. Охрана шире, чем
`attempt == 0`: прогон проходит только с исходом `pass` последнего выполненного шага — это закрывает и вырожденный
случай «проверка упала, вернула, а во втором раунде пропущено всё, включая её саму» (`if` по счётчику `run.attempt`).

## 4. Коммиты и объём

1. **`WP-103: a step with an if that does not hold is skipped; back_from names the checks that sent the run back`** —
   контракт (§1), ядро (§2–3), тесты контракта и ядра, документы (§6, кроме frontends). Отдельным коммитом контракт не
   выделять: `if`, который грузится и игнорируется ядром, хуже отсутствия ключа.
2. **`WP-103: run watch names the skipped step`** — `cli/mod.rs`, e2e, `frontends.md`.

Оценка контекста кодера: чтение по плану ~35k, правки и итерации тестов ~70k, документы ~15k, гейт ~10k — **~130k**.
Точка разреза при > 200k после коммита 1: коммит 2 — отдельный хвост (одна строка кода и один e2e). Гейт
`scripts/loop/checks 115` — один раз в конце.

## 5. Тесты

Контракт. `crates/aae-contract/src/tests/config_steps.rs` — 379 строк, туда нельзя: новый
`src/tests/config_if.rs` (+ `mod config_if;` в `tests/mod.rs`; хелперы `load_with`/`fails` из `tests/config.rs:97,120`,
образец пяти видов шага — `a_step_of_any_kind_takes_max_attempts_and_a_check_script`, `config_steps.rs:~215`):

- `a_step_of_any_kind_takes_an_if_table` — все пять видов с `if = { back_from = "t" }` грузятся; у шага без ключа
  `condition.is_empty()`; у шага с ключом `condition.matches(&json!({ "back_from": ["t"] }))` и не совпадает с `[]`.
- `the_paths_of_an_if_are_checked_at_load` — таблица. Ошибки: `"inputs.nope"`; `"steps.later.result"` (поздний шаг);
  `"steps.pre.nope"`; `remarks`; `"agent.x"`; `"bead.title"` без входа `task`; `run` (объект); `"nowhere.x"`;
  `"back_from.x"`; `inputs.deploy = true` без кавычек (матчер-таблица); `back_from = "nope"` (не проверка). Проходят:
  `"inputs.deploy" = true`, `"run.formula"`, `"rig.name"`, `"project.name"`, `"steps.pre.data.exit_code" = 0`,
  `back_from = ["t"]`, `"steps.cond.result" = "pass"`, где у `cond` свой `if`.
- `a_template_or_step_inputs_cannot_name_a_step_with_an_if` — шаблон `{{ steps.pre.summary }}`: у `pre` есть `if` →
  `is not a value`; без `if` — грузится (контроль). То же для `inputs = { x = "{{ steps.pre.summary }}" }` шага
  `formula` → `names nothing a run has`, и контроль.
- `src/tests/template.rs:161` (`event_paths_follow_the_payload_schemas`): `event_path("run.step_skipped", "step", true)
  == Some(true)` и `("run.step_skipped", "run.formula", true) == Some(true)` — закрепляет запись схемы (урок ревью 114).

Ядро. Новый `crates/aae-testkit/tests/core/step_if.rs` (+ `mod step_if;` в `core/main.rs`), реальное время
(`world_with(.., Arc::new(ExecHooks::default()))`, хелперы-образцы `returns.rs:17-35`: `exec` с параметром лишних
ключей, счётчик в файле, `log`; запуск со входами — `Create { inputs, .. }`, `returns.rs:38-52`; события —
`w.events`, `w.all`, `w.until`, `common/mod.rs:197,235,276`):

| «Готово, когда» / охрана | Тест | Что утверждает |
|---|---|---|
| `back_from`: первый раунд, названная проверка, другая проверка | `a_step_waiting_for_a_check_runs_only_in_the_round_that_check_sent_back` | `[work, fix(if back_from = "review"), review(back_to fix, падает в 1-й раз), lint(back_to work, падает только во 2-й раз)]`. Лог `work review lint fix review lint work review lint`; `run.passed`; два `run.step_skipped`, оба `step == "fix"`, ключи `data` — ровно `run` и `step`, `data.run.step == "fix"`; `run.preparing` с `run.step == "fix"` — один; `run.step_finished` с `run.step == "fix"` — один; ни одного `run.state_changed` с `from == to`. Заодно — возврат на цель с `if` |
| `if` по `inputs` и по `steps`; нет `AAE_STEP_<NAME>_*` | `a_step_runs_by_an_input_and_by_the_outcome_of_an_earlier_step` | вход `deploy: boolean`; `[probe, ship(if "inputs.deploy" = true), after(if "steps.ship.result" = "pass"), env(пишет ${AAE_STEP_SHIP_RESULT:-none})]`. `deploy = true`: лог `probe ship after pass`; без входа: `probe none`, пропуски `ship`, `after` |
| пропущенная проверка не возвращает; старый исход снят | `a_skipped_check_sends_nothing_back_and_loses_its_old_outcome` | `[work(printf номера запуска), gate(exit 1, back_to work, if "steps.work.data.stdout" = "1"), tail(пишет ${AAE_STEP_GATE_RESULT:-none})]`: раунд 1 — `gate` падает и возвращает; раунд 2 — пропущен; `run.passed`, `round == 2`, `tail` записал `none` |
| исход последнего выполненного | `a_skipped_last_step_leaves_the_outcome_of_the_last_step_that_ran` | `[work, note(if "inputs.deploy" = true)]` без входа: `run.passed.outcome.summary == "exit 0"`, `run.passed.run.step == "note"`, попытка одна |
| пропущены все | `a_run_whose_every_step_is_skipped_fails` | один шаг с несовпавшим `if`: `run.failed {class: infrastructure, reason: "every step was skipped", outcome: null}`, `run.started` один, `run.preparing` нет |
| повтор не спрашивает `if` | `a_repeat_of_a_step_does_not_ask_its_if_again` | `[flaky(падает 1 раз, max_attempts = 2, if "run.step_attempt" = 1)]`: в логе два запуска, `run.passed`, `run.step_skipped` нет |
| рестарт (Recovery) | `a_restart_keeps_the_checks_that_sent_the_run_back` | формула из `await`: `[a, gate(back_to a), hold, fix(if back_from = "gate")]`; `gate` — `fail`, во втором раунде прогон ждёт на `hold`, `w.restart()`, `hold` — `pass` → `run.awaiting` с `run.step == "fix"`. Образец — `await_step.rs` (`resolve`, рестарт). Срезается первым при нехватке контекста |

e2e, новый `crates/aae/tests/e2e/step_if.rs` (+ `mod` в `e2e/main.rs`; `cli.rs` 393 — туда нельзя; хелпер `cli` и
`Daemon::start(|dir| ..)` — образец `await_step.rs:12-30,109-114`):
- `a_skipped_step_shows_in_run_watch` — формула `[work(exec true), notify(exec true, if "inputs.notify" = true)]`,
  вход объявлен и не дан; `until_state(passed)`; `aae run watch <run>` (без `--json`; поток закрывается сам — прогон
  завершён, `api/live.rs:79-86`) печатает строку с `run.step_skipped` и `step=notify`; `run show`: в `steps` нет
  `notify`, `outcome.summary == "exit 0"`.

Охрана → мутация → тест, который краснеет (мутировать каждую до коммита):

| Охрана | Мутация | Краснеет |
|---|---|---|
| `!step.condition.matches(&context)` в `render` | убрать условие | оба первых теста ядра |
| `row.step_attempt == 1 &&` | убрать | `a_repeat_of_a_step_does_not_ask_its_if_again` |
| `UPDATE attempts SET skipped = 1` в `skip` | убрать | `a_skipped_check_sends_nothing_back…` (прогон падает `still failing after 3 rounds`) |
| `AND skipped = 0` в запросе `advance` | убрать | тот же тест |
| `AND skipped = 0` в `step_results` | убрать | тот же тест (`tail` пишет `fail`) |
| `back_from = ?3` в `UPDATE` | писать `'[]'` | `a_step_waiting_for_a_check…` (`fix` не выполняется) |
| `if row.state != "preparing"` у `run.step_finished` и `set_state` | убрать любое | `a_step_waiting_for_a_check…` (счёт `run.step_finished`, `from == to`) |
| ветка `None if !…Pass` в `advance` | убрать | `a_run_whose_every_step_is_skipped_fails` (`run.passed`) |
| `optional` в `prompt::path` | `true` для шаблона/`step_inputs`; `false` для `step_if` | `a_template_or_step_inputs_cannot_name…`; строка `steps.cond.result` в `the_paths_of_an_if…` |
| список корней в `step_if` | убрать проверку корня | `remarks`, `agent.x`, `nowhere.x` в `the_paths_of_an_if…` |
| `strangers` | не звать | `back_from = "nope"` там же |
| запись `run.step_skipped` в `schema.rs` | убрать | `event_paths_follow_the_payload_schemas` |
| `step=` в `follow` | убрать | `a_skipped_step_shows_in_run_watch` |

## 6. Документы (английский, «как сейчас», без номеров пакетов)

- `docs/design/config.md` «Formula»: в пример — строка `if = { back_from = "test" }  # optional, any step: a `where`
  table; the step is skipped unless it matches` на одном из шагов; `:593-595` «the run's outcome is that of its last
  step» → «of the last step that ran». Новый абзац после абзаца о проверках (`:658-668`): таблица как у `where`
  (contracts 2.2), корни `inputs`, `bead`, `rig`, `run`, `project`, `steps` (только более ранние) и `back_from` — имена
  проверок, вернувших прогон в текущий раунд (в первом пусто; значение должно быть именем проверки формулы); пути
  проверяются при загрузке; спрашивается на входе в шаг (в начале, после шага перед ним, при возврате на него), не при
  повторе под `max_attempts`; пропуск — `run.step_skipped`, без хуков `run.preparing`, без попытки и исхода; пропущенная
  проверка не возвращает, ряд проверок считается по тексту формулы; `back_to` на шаг с `if` пропуска не отменяет и сам
  ничего не пропускает, шаг «только после возврата» пишется `if = { back_from = "<check>" }`; все шаги пропущены →
  `failed{infrastructure}`, `every step was skipped`. Абзац `:688-694` («A step sees the outcomes…»): у пропущенного
  шага исхода нет, пока он не выполнится снова — нет `AAE_STEP_<NAME>_*`; ссылка на шаг с `if` из шаблона или из
  `inputs` шага `formula` — ошибка конфига.
- `docs/design/contracts.md`: §1.3 — строка `run.step_skipped | {run, step}` в наблюдаемых событиях; в `RunView`
  (`steps`, `:74`) — «a skipped step has none». §2.1 — пути также в `if` шага. §2.2 — та же таблица служит `if` шага
  над контекстом прогона и `back_from` (config.md). §2.3 — `steps`: путь к шагу с `if` — ошибка конфига. §6.1,
  пункт «A run goes through the steps…» (`:687-693`): пропуск на входе, в той же транзакции, `run.preparing` не
  решается; исход прогона — последнего выполненного шага; все пропущены → `failed{infrastructure}`. §6.2 — строка
  `preparing` | the `if` of the step does not hold | `preparing` of the next step, or the end of the run |
  `run.step_skipped`; no hooks, no attempt.
- `docs/overview.md:61`: в правой ячейке — «…a bounded return to an earlier step, and a step may be skipped by its
  `if`» (графа шагов по-прежнему нет).
- `docs/api/frontends.md:64` (коммит 2): строка события в `run watch` называет шаг (`step=<name>`), если событие его
  несёт.
- `aae run watch` (`cli/mod.rs:345-366`) печатает любое событие строкой `seq  type  run`, так что пропуск виден и без
  кода, но без имени шага: три условных шага дали бы три одинаковые строки. Одна строка кода после `:357`:
  `line.extend(inner["step"].as_str().map(|s| format!("step={s}")));` (ключ `step` в `data` есть только у этого
  события). OpenAPI не меняется.

## 7. Ловушки

- **400 строк на файл** (`scripts/budget.sh:39`): `machine.rs` 397 — ровно +1 строка, пропуск туда не писать;
  `items.rs` 385 — после правок проверить `wc -l` (rustfmt может развернуть цепочку `map_err`); `cli/mod.rs` 380;
  `config_steps.rs` 379, `e2e/cli.rs` 393 — новые файлы.
- `enter` различает пропуск по `row.state == "preparing"`. Это держится на том, что из выполненного шага в `enter`
  приходят только из `finishing`; при правках рядом это свойство не ломать, комментарий в коде обязателен.
- `render` строит контекст до ветки вида шага: ошибка `step_results` теперь возможна и для exec/await/drain — это
  ошибка БД, а не конфига, поведение прежнее (`failed{infrastructure}`).
- `step_agent` следующего шага (`finish.rs:187`) считается до его `if`: пропускаемый ai-шаг с исчезнувшим агентом
  всё равно уронит прогон. Не чинить.
- Инвариант 12 (исход объявляет шаг): ядро исход пропущенному шагу не выдумывает — у него его нет вовсе.
- Сообщение об ошибке «ссылка на шаг с `if`» общее (`is not a value…`), причину объясняет config.md. Отдельный текст
  — лишний код в двух местах; если ревью попросит, это одна ветка в `prompt.rs`.
- `VERSION` схемы: старое состояние демона после сборки отвергается (`not migrated`) — так и задумано до релиза;
  тест `tests/mod.rs:328` от номера не зависит.

## 8. Допущения (в журнале, запись expert цикла 115)

1. Все шаги пропущены (и вообще нет исхода `pass` у последнего выполненного шага) → `failed{infrastructure}`,
   `every step was skipped`, а не `passed` без исхода.
2. `if` спрашивается на входе в шаг и не спрашивается при повторе под `max_attempts`; `run` в контексте — `RunView`
   на входе (`state = preparing`).
3. Пропуск снимает исход шага из прошлых раундов (`attempts.skipped`), пока шаг не выполнится снова; схема БД 24.
4. Значения матчера `back_from` обязаны быть именами проверок формулы — ошибка загрузки сверх буквы пакета.
5. Ряд проверок статический: `if` границы ряда не двигает.
6. `run.step_skipped` = `{run: RunView, step}`; `aae run watch` дописывает `step=<имя>`.

Не делается: `if` в `aae config explain`, `back_from` в `GET /v1/runs/{id}`, отдельный текст ошибки для ссылки на
шаг с `if`, сценарии chaos/acceptance, `demos/`.
