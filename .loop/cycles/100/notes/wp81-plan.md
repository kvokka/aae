# WP-81 · `drain`: `pool` и `on_item_failure` — план

expert, цикл 100. Спека §9.1 (`drain`), §9.3, решения 83, 100, 101, критерии 30 и 24. Читал: `run/drain.rs`,
`decide.rs`, `events.rs` (cancel), `finish.rs`, `repeat.rs`, `recover.rs`, `beads.rs` (`let_go_bead`, `ready_in`),
`render.rs`, `config/formula_file.rs`, `items.rs`, тесты drain, `c24_drain.rs`, `c29_repeat.rs`. Ничего не запускал.

## 1. Суть: нового состояния нет

Остановка **выводится**, а не хранится. Проход `RunHandler::pass` (`run/drain.rs:68`) уже читает из `runs` список
дочерних прогонов этого входа в шаг (`kids`: `bead, state, mine`). Остановлен шаг или нет — функция от него:

    stopped = policy != continue && kids.any(mine && state == "failed")

- Ни события, ни колонки, ни правки `store.rs`, `recover.rs`, `machine.rs`, `decide.rs`, `beads.rs`, `events.rs`.
- Рестарт (§5.6): `recover` роняет попытку шага с `daemon_crash`, политика даёт новую попытку, `start_drain` просит
  чтение трекера, `pass` снова видит проваленного ребёнка среди `mine` (вход в шаг — хвост попыток после последнего
  исполнения с исходом, рестарт входа не начинает: SQL в `drain.rs:88–92`) → `stopped` истинно → новых бидов нет.
- Настройки шага переживают рестарт в снимке `runs.config` (как `drain: <formula>` сейчас, `render.rs:41–43`).
- Повтор шага (`max_attempts`) и раунд возврата — новый вход: `mine` обнуляется, остановка снята. Так и надо.

## 2. Конфиг

`config/formula_file.rs` (147 строк):
- `StepFile::Drain` += `pool: Option<NonZeroU32>`, `#[serde(default)] on_item_failure: OnItemFailure`.
- `pub enum OnItemFailure { #[default] Continue, SoftStop, HardStop }` — `Clone, Copy, Debug, Default, PartialEq, Eq,
  Serialize, Deserialize`, `#[serde(rename_all = "snake_case")]`. Реэкспорт рядом с `StepKind`.
- `StepKind::Drain { formula, timeout, pool: u32, on_item_failure: OnItemFailure }`.

`config/items.rs:242` — `pool: pool.map_or(1, NonZeroU32::get), on_item_failure`. Потолка у `pool` нет: сверху его
держат `max_runs` и лимит агента (§9.3) — меньше кода, запись [Д] в журнал.

Ошибки (одна строка, даёт serde, своего кода ноль; место — заголовок таблицы шага, как у `max_attempts = 0`):
- `pool = 0` → `invalid value: integer `0`, expected a nonzero u32`;
- `on_item_failure = "stop"` → `unknown variant `stop`, expected one of `continue`, `soft_stop`, `hard_stop``;
- `pool` у шага `ai`/`exec` → `unknown field `pool`` (`deny_unknown_fields` уже стоит).

`run/render.rs:41–43` — снимок `{"drain", "pool", "on_item_failure", "env"}`.
`aae-front/src/api/config.rs:205–206` — строка explain: ``drain, a run of `<each>` per bead, <pool> at a time`` и
хвост ``; on a failed bead: soft_stop`` только при не-`continue`. Другие места с `StepKind::Drain { .. }`
(`check.rs:37`, `queue.rs:31`, `items.rs:181`) уже с `..`; `tests/config_steps.rs:81` — шаблон `timeout: None`
дополнить `..`.

## 3. `pass` в `run/drain.rs` (256 → ~290 строк)

1. В запрос `kids` добавить `r.id`: `(id, bead, state, mine)`.
2. После `kids`: `pool = config["pool"].as_u64().unwrap_or(1)`; `policy: OnItemFailure =
   serde_json::from_value(config["on_item_failure"].clone()).unwrap_or_default()` (снимок до обновления — умолчания).
3. `live(state)` = не `passed|failed|cancelled`. `stopped` — формула из раздела 1. Провал — только `failed`:
   ребёнок, отменённый оператором, шаг не останавливает ([Д], в журнал).
4. `hard_stop` и `stopped`: для каждого живого ребёнка `self.cancel(tx, &id, "drain_hard_stop")` — это
   `RunHandler::cancel` из `run/events.rs:25`, та же функция, что `Msg::Cancel` и отмена детей родителя (она сама
   рекурсивно отменяет внуков с `parent_cancelled`). Звать только для живых: неживому она отвечает `Rejected`, а это
   откат всей транзакции. `kids` после этого не перечитывать: устаревший список говорит «есть живые» → проход
   выходит; каждая отмена сама ставит в очередь чтение (`let_go_bead` → `sync:end:`/`sync:release:`), следующий
   проход видит всех терминальными и завершает шаг. Отмена — в той же транзакции, что и решение: после `kill -9`
   либо отменены все, либо проход повторится.
   Допущение [Д]: «сразу» = на первом чтении трекера после провала (его ставит конец проваленного прогона), не в
   самом `terminal`: второй путь выдачи/отмены не заводим.
5. Цикл выдачи: `let mut free = pool.saturating_sub(<число живых kids, любого входа>)`. Для бида, прошедшего фильтр
   `under`/`mine`: `if stopped || free == 0 { not_started.push(bead); continue; }`, дальше как сейчас; после
   `Ok(_)` — `free -= 1`. Отказ (`Rejected`, глубина) слот не тратит. Ребёнок в `queued` — «в работе»: считается
   живым ([Д]: `pool` — выданные, а не исполняемые; иначе предел зависел бы от занятости каталога).
6. Условие выхода `given > 0 || alive || reads > 1` не менять: конец ребёнка уже будит проход (чтение из
   `let_go_bead`), он выдаёт следующий бид — отдельного пути «освободился слот» не нужно.
7. Итог: `result` — `Fail`, если `stopped`, иначе `Pass`. `data` — прежние `{closed, failed, refused}` (форма при
   `continue` не меняется: проверки на неё в `drain_steps.rs`, `drain_rounds.rs`, c24 остаются), при остановке плюс
   `"stopped_on": of(&["failed"])` и `"not_started": not_started`. `summary` при остановке:
   `stopped on <id, ..>: <n> closed, <n> failed, <n> not started`. При `continue` в конце `not_started` пуст сам
   (`given == 0` ⇒ каждый готовый бид — `mine` или отказ).
   Предел [Д]: `not_started` — только готовые биды под корнем; заблокированный бид проходу не известен.
8. Дальше существующий путь: `step_done` → `run.finishing` → `finished` → `repeat(.., "agent", false)` →
   `advance(failed)` → `terminal(failed, class agent, reason = summary)`: причина прогона называет проваленный бид,
   `run.failed` несёт `outcome` с `data`. `run.step_finished` у остановленного шага **нет** (его шлёт `enter`), если
   нет повтора: тесты смотрят `run.failed`, а `World::settle` (ждёт `run.step_finished`) для них не годится.

## 4. Явный `pool` в фикстурах

| файл | правка | зачем |
|---|---|---|
| `aae-testkit/tests/common/drain.rs:34` (`SWEEP`) | `pool = 8` | `drain_steps.rs`, `drain_rounds.rs` ждут выдачу всех сразу (`["failed", "running", "queued"]`) |
| `aae-testkit/tests/core/drain_rounds.rs:16` (`AGAIN`) | `pool = 8` | то же |
| `aae-testkit/tests/chaos/main.rs:80` | `pool = 2`, в той же строке литерала | D1–D3: два ребёнка живы в момент `kill`; файл 383 строки |
| `aae-acceptance/.../c24_drain.rs:24` | `pool = 4` | F должен провалиться, пока A держится |
| `demos/1-planner-executor`, `demos/2-multirepo-qa`: `pack/formulas/deliver.toml` | `pool = 2` | текст WP; README демо — одна фраза |
| `docs/design/config.md:521`, `docs/guide/first-formula.md:211` | оба ключа с комментарием | фрагмент гайда прогнать через `aae config check` |

Без правки: `core/tests/config_steps.rs` (только загрузка), `core/step_repeat_more.rs:31` (один бид). `c20_demo2`
получает `pool = 2` через пак демо — проверить, что зелёный. Что ещё упадёт — покажет первый прогон
`cargo nextest run -p aae-core -p aae-testkit`; лечить строкой `pool`, не логикой теста.

## 5. Тесты

`aae-core/src/tests/config_steps.rs` (233 строки):
1. `a_drain_step_takes_pool_and_on_item_failure` — умолчания `(1, Continue)`, явные `(3, HardStop)`.
2. `a_bad_pool_or_on_item_failure_is_one_line` — три ошибки раздела 2, каждая одна и без `\n`.

Новый `aae-testkit/tests/core/drain_pool.rs` (+ `mod` в `core/main.rs`), формулы `one` (без `pool`), `two`
(`pool = 2`), `soft`, `hard` (`pool = 2` + политика) поверх `SWEEP`; `max_runs = 1` фикстуры оставить: первый
ребёнок `running`, второй `queued`, оба «в работе». Пять бидов `w.tracker.add_in("w", ..)`:
3. `without_pool_one_bead_is_in_work_at_a_time` — пока жив первый, `kids.len() == 1`; в конце 5 `closed`.
4. `pool_two_keeps_two_of_five_in_work_and_a_failed_bead_does_not_stop_the_rest` — на старте 2; после провала
   первого 3 всего, 2 живых; в конце `pass`, `closed` 4, `failed` 1, порядок выдачи = порядок готовых.
5. `soft_stop_gives_no_new_bead_and_lets_the_started_end` — первый `fail`; через 30 с бумажных `kids.len() == 2`,
   второй жив, прогон не кончен; второй `pass` → `run.failed`, `class` `agent`, `reason` называет бид,
   `outcome.data == {closed: [b2], failed: [b1], refused: {}, stopped_on: [b1], not_started: [b3, b4, b5]}`.
6. `hard_stop_cancels_the_started_beads` — первый `fail` → второй `cancelled` (причина `drain_hard_stop` в
   `run.cancelled`), `kids.len() == 2`, `run.failed`, `failed: [b1, b2]`, `stopped_on: [b1]`. Второго не
   заканчивать руками: `end_attempt` на отменённом прогоне паникует на `unwrap`.
7. `a_restart_mid_stop_gives_no_new_bead` — `soft`, первый `fail`, второй жив, `w.restart()` (конфиг с `POLICY`, как
   `a_restart_mid_drain_fails_the_attempt_and_gives_no_bead_twice`), 30 с, `kids.len() == 2`; второй `pass` на
   попытке 2 → `run.failed`, `not_started` из трёх.
8. `a_repeat_after_a_stop_is_a_new_entry` (`max_attempts = 2`, `soft`; если контекст < 150k) — второй вход выдаёт
   биды заново, включая проваленный.

Приёмка, две строки `criterion: "30"`, `Launch::Ci`, `"30"` убрать из `OPEN` (`readiness.rs:13–16`):
- `checks.rs` — `tests(env, "-p aae-core -p aae-testkit -E test(/<имена 1–8>/)", <N>)`;
- `scenarios/c30_drain.rs` (новый, ~130 строк) + `mod` и `Scenario` в `scenarios/mod.rs`. Настоящий br и демон, без
  модели и без провайдера: `tracker(root)`; формулы `one|two|soft|hard` = exec-шаг `plan`
  (`for i in 1 2 3 4 5; do br create "B$i" --parent "$AAE_BEAD" --silent; done`) + шаг `drain` на `work`; `work` —
  один exec-шаг `work.sh`:
  `rm "$D/fail" 2>/dev/null && exit 3; echo "$AAE_BEAD" >> "$D/started"; while [ ! -e "$D/go" ]; do sleep 0.05; done`.
  Прогон — `d.post("/v1/runs", ..)` с `inputs.task` и без рига (`d.create` входов не даёт): дети с префиксом
  базы идут в корне проекта. Фазы подряд, один демон; перед фазой убрать `go` и `started`, для `two|soft|hard` создать `fail` (первый запуск
  `work.sh` проваливается — id бидов знать не нужно). Дети — `GET /v1/runs?parent=<run>`:
  - `one`: в `started` одна строка → детей ровно 1 → `go` → `passed`, детей 5, все `passed`;
  - `two`: ждать «детей 3, живых 2» (при сломанном `pool` их сразу 5 — таймаут) → `go` → `passed`, 4 + 1 `failed`;
  - `soft`: первый `failed`, в `started` строка → `d.restart()` → `go` → `failed`; детей 2: `failed`, `passed`;
    `reason` содержит `3 not started`;
  - `hard`: `go` не создавать → `failed`; детей 2: `failed`, `cancelled`.

Мутации своего диффа (убить до отчёта; цель — тесты 1–8 и c30):
- M1 `free == 0` не проверяется → 3, 4, c30 `one`/`two`;
- M2 живыми для `pool` считать только `running` → 4;
- M3 `stopped` → `false` → 5, 6, 7;
- M4 убрать отмену в `hard_stop` → 6, c30 `hard`; M5 отменять и при `soft_stop` → 5, c30 `soft`;
- M6 `Verdict::Fail` → `Pass` после остановки → 5, 6;
- M7 `stopped` без `mine` → 8; M8 `mine` только по текущей попытке → 7;
- M9 умолчание `pool` 1 → 2 → 1, 3; M10 `pool` не попал в снимок `render` → 4; M11 `not_started` пуст → 5.

## 6. Порядок коммитов и оценка

| № | коммит | токены |
|---|---|---|
| 1 | `WP-81: a drain step keeps at most `pool` beads in work` — конфиг `pool`, снимок, предел в `pass`, фикстуры раздела 4 в `crates/`, explain, тесты 1 (часть), 3, 4 | ~40k |
| 2 | `WP-81: `on_item_failure` stops a drain step softly or hard` — enum, остановка, итог, тесты 1, 2, 5–8 | ~40k |
| 3 | `WP-81: acceptance rows of criterion 30` — `c30_drain.rs`, строка тестов, `OPEN` без 30; `aae-acceptance 24 30` | ~30k |
| 4 | `Demos: the drain step works on two beads at once` — два `deliver.toml`, README; `aae-acceptance 20` (CI-часть) | ~8k |
| 5 | `Docs: pool and on_item_failure of a drain step` — `config.md` («Formula»: образец, абзац про `drain`, строка explain :173–174), `contracts.md` 6.2 (строки `preparing (drain)`, `running (drain)` ×2), гайд, `invariants.md:42` | ~15k |
| 6 | мутации M1–M11; журнал ([Д] ниже); READINESS (только `docs/`), PLAN `done`; `scripts/loop/checks 100`; отчёт | ~35k |

Итого ~170k с базой. Каждый коммит зелёный: умолчание `pool = 1` меняет поведение, поэтому фикстуры — в коммите 1.

[Д] в журнал вместе с коммитами 1–2: остановка выводится из `runs`, без события; проваленный = `failed`, не
`cancelled`; `hard_stop` отменяет на следующем чтении трекера, причина `drain_hard_stop`; в `pool` считаются и
`queued`; `not_started` — только готовые; потолка у `pool` нет; класс провала прогона — `agent`, как у exec-шага.

Файлы и лимит 400: `scenarios/mod.rs` 373 → ~382, `chaos/main.rs` 383 → 383 (строк не добавлять), `items.rs` 358 →
~364, `checks.rs` 342 → ~356, `drain.rs` 256 → ~290, `config_steps.rs` 233 → ~265; новые `drain_pool.rs` ≤ 250,
`c30_drain.rs` ≤ 150. Не трогать: `core/beads.rs` (тест, 388), `run/hooks.rs` (389), `run/beads.rs` (381),
`run/events.rs` (381), `drain_steps.rs` (287 — новые тесты не сюда). Ядро: +~60 строк кода к 10746 / 18000.

## 7. Файлы WP-81 — `subcoder` (WP-83) их не трогает

- `crates/aae-core/src/config/formula_file.rs`, `config/items.rs`, `run/drain.rs`, `run/render.rs`,
  `tests/config_steps.rs`;
- `crates/aae-front/src/api/config.rs`;
- `crates/aae-testkit/tests/common/drain.rs`, `tests/core/main.rs`, `core/drain_pool.rs`, `core/drain_rounds.rs`,
  `tests/chaos/main.rs`;
- `crates/aae-acceptance/src/readiness.rs`, `checks.rs`, `scenarios/mod.rs`, `scenarios/c24_drain.rs`,
  `scenarios/c30_drain.rs`;
- `demos/*/pack/formulas/deliver.toml`, `demos/*/README.md`;
- `docs/design/config.md`, `contracts.md`, `invariants.md`, `docs/guide/first-formula.md`, `docs/READINESS.md`;
  `.loop/PLAN.md`.

Общие с WP-83 — `readiness.rs`, `checks.rs`, `scenarios/mod.rs`, `contracts.md`, READINESS, PLAN: их правит только
кодер, после коммитов WP-81 (см. `stagnation.md`, раздел 2). WP-83 в ядре работает в `run/finish.rs`, `machine.rs`,
`events.rs`, `driver.rs` — WP-81 их не касается.

## 8. Чего в плане нет (осознанно)

- Хаос-стенд не получает остановку: только `pool = 2`. Инвариант «после остановки нет новых бидов» под `kill -9`
  держат тест 7 и фаза `soft` c30. Добавить в стенд — если ревью попросит.
- Нет события `run.drain_stopped` и нет «первого проваленного» по времени: `stopped_on` — все проваленные входа.
- Реальная одновременность двух команд в c30 не меряется (дети одного каталога идут по одному, второй — `queued`);
  критерий меряется числом выданных.
