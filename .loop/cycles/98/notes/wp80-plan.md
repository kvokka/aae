# WP-80 · Повтор шага и скрипт-проверка — план реализации

expert, цикл 98. Спека: §9.1 («Повтор шага и скрипт-проверка»), §5.5, §5.6, §14 (правила тестов), §15 критерий 29.
Код читал на `58f6c6d` + `da89828`. Код продукта не менял.

## 1. Суть одним абзацем

Повтор шага — это **вход в тот же шаг** существующим `enter()` (`run/finish.rs`), как при `back_to`, только без
`round + 1`. Счётчик — столбец `runs.step_attempt`. Скрипт-проверка — **та же `exec`-команда** (эффект `op: "exec"`,
тот же раннер, маркер, таймаут, `stop_exec`), запущенная из `finished()` после того, как хуки `run.finishing` приняли
`pass`; прогон на это время остаётся в `finishing` без ожидаемого решения. Новых состояний, типов событий и строк
`attempts` для скрипта нет. Новый файл ядра один — `run/repeat.rs` (~80 строк).

## 2. Три счётчика и почему они не пересекаются

| что | где | кто расходует |
|---|---|---|
| попытка сессии (`attempt`, потолок 20, инвариант 16) | `runs.attempt`, строки `attempts`; считает `decide.rs::retry` по `failure IS NOT NULL AND waited = 0` | сбой инфраструктуры |
| замечания хода (10 на попытку) | `attempts.prompts` | `remark` / `continue` хуков |
| раунд возврата (`rounds`) | `runs.round`, `runs.remarks` | провал проверки с `back_to` |
| **исполнение шага (`max_attempts`)** | **`runs.step_attempt`, `runs.step_remarks`** | финишер `fail`, ненулевой код команды, скрипт ≠ 0 |

- Повтор шага заводит новую строку `attempts` **без `failure`** — запрос потолка в `retry()` её не видит. Менять
  `retry()` не нужно; тест это закрепляет.
- Новая попытка — новый счёт `prompts`: потолок замечаний не расходуется.
- `round` при повторе не трогается. Вход в шаг из другого шага (следующий шаг или возврат) ставит `step_attempt = 1`:
  каждый раунд даёт шагу полный `max_attempts` (§9.1 [Д] «повтор идёт внутри раунда»).
- Сбой инфраструктуры внутри исполнения (`daemon_crash`, `stall`) `step_attempt` не меняет (§9.1 [Д]).

Имя: в состоянии, `RunView` и доках — `step_attempt` («execution of a step»); `attempt` остаётся попыткой сессии.
Ключ конфига — `max_attempts` (имя владельца, [Р]).

## 3. Состояние

Миграция 22 в `store.rs` (обычный `ALTER`, старые строки получают умолчание):

```sql
ALTER TABLE runs ADD COLUMN step_attempt INTEGER NOT NULL DEFAULT 1;
ALTER TABLE runs ADD COLUMN step_remarks TEXT;
```

- `Row` (`run/row.rs`): поля `step_attempt: u32`, `step_remarks: Option<String>` в `COLUMNS`; в `view()` —
  `"step_attempt"`. Так номер исполнения попадает в `run` каждого события и в `run show` без отдельного кода.
- Счётчик и замечание пишутся одним `UPDATE` в `enter()`, в той же транзакции, что `run.step_finished` и старт
  следующей попытки. `kill -9` либо до фиксации (ничего не записано), либо после (записано всё) — отдельной логики
  «пережить рестарт» нет.

## 4. Машина

### 4.1 `finish.rs`

`finished()`:

- `(Pass, None)` → `self.check(tx, row)` (было `advance(None)`);
- `(Pass, Some(veto))` → как было, `failed{check}`; вето хука повтором не лечится;
- `(Fail, _)` → `self.repeat(tx, row, &outcome, "agent", false)` (было `advance(Some(summary))`).

`advance(tx, row, failed: Option<(&str, &str)>)` — пара `(класс, причина)` вместо одной причины; в `terminal`
уходит класс из пары. `advance` и `enter` — `pub(super)`.

`enter(tx, row, next, again: Option<&str>)`:

```sql
UPDATE runs SET step = ?2, agent = ?3, step_kind = ?4,
  step_attempt = CASE WHEN ?5 IS NULL THEN 1 ELSE step_attempt + 1 END, step_remarks = ?5 WHERE id = ?1
```

- `run.step_finished` выходит **до** `UPDATE` — в нём `run.step_attempt` закончившегося исполнения.
- После `UPDATE` поправить `row.step_attempt` и `row.step_remarks` в памяти: `render_and_prepare` берёт их из `row`.
- Вызовы из `advance` — `None`.

### 4.2 `run/repeat.rs` (новый)

```rust
/// pass принят: решает скрипт шага, если он есть (прогон ждёт его в `finishing` без решения)
pub(super) fn check(&mut self, tx, row) {
    match шаг(row).and_then(|s| s.check.clone()) {
        None => self.advance(tx, row, None),
        Some(script) => self.command(tx, row, json!([script]), true),
    }
}
pub(super) fn check_done(&mut self, tx, row, exit, stdout, stderr) {
    clear_deadline(state);
    Code(0)        => advance(None)
    SpawnFailed(t) => terminal(failed, infrastructure, "check script: {t}")
    Code(75)       => fatal;   Code(n) | Signal(n) | TimedOut => не прошёл
    // слово скрипта — исход шага:
    UPDATE attempts SET outcome = {fail, "check script: exit N", {exit_code, stdout, stderr}}
    self.repeat(tx, row, &outcome, "check", fatal)
}
pub(super) fn repeat(&mut self, tx, row, outcome, class, fatal) {
    match шаг(row).cloned() {
        Some(step) if !fatal && row.step_attempt < step.max_attempts =>
            self.enter(tx, row, step, Some(&remark(&row.step, outcome))),
        _ => self.advance(tx, row, Some((class, &outcome.summary))),
    }
}
```

- `remark()` — существующая функция `finish.rs` (имя шага, резюме, stdout, stderr): формат замечания тот же, что у
  `back_to`.
- Исход переписан в `fail` — поэтому без правок работают: `advance` для шага с `back_to` (читает исход из
  `attempts`), `terminal` (исход в `run.failed`), `steps.<имя>` следующих шагов, инвариант S2 хаоса.
- Класс провала из-за скрипта — `check` (исход был `pass`, его отверг проверяющий — как вето). `agent` нельзя:
  инвариант 13 хаоса требует, чтобы последний финишер сказал `fail`.
- Собственный провал: скрипт не запускается, попытка неудачна сразу [Д].
- Шаг пропал из конфига — `advance` уже даёт `infrastructure`.

### 4.3 Запуск скрипта — `exec_step.rs`

Тело `start_exec` после `run_attempt` вынести в
`fn command(&mut self, tx, row, command: Value, check: bool)`: окружение, маркер, эффект, дедлайн. Отличия проверки:

- `"check": true` в payload; ключ `exec:<run>` тот же, dedupe — `check:<attempt>` (у команды шага — `<attempt>`);
- окружение — то же, плюс исход самого шага: `step_results` получает вторым аргументом исключаемый шаг (для
  проверки — `""`), скрипт видит `AAE_STEP_<ИМЯ>_RESULT/_SUMMARY/_DATA` своего шага [Д];
- `env` берётся из `row.config["env"]` — у `ai`-шага там та же форма `{value, from}`. Для `drain` в `render.rs`
  положить в снимок тот же `env` трекера, что у `exec` (одна строка);
- каталог — `row.workdir`; таймаут — таймаут шага заново [Д]; дедлайн `state` = таймаут + `LOST_SLACK_MS`.

`Msg::ExecDone` получает `check: bool` (из payload). В `exec_done`: `check` → принять только при
`row.attempt == attempt && state == "finishing" && waiting IS NULL`, иначе выбросить; дальше `check_done`.

Прочее:

- `events.rs::on_deadline`: `("state", "finishing") => check_done(row, Exit::TimedOut, "", "")` — потерянный скрипт.
- `machine.rs::end_attempt`: `stop_exec` ставить ещё и когда в `effects` есть ключ `exec:<run>` (отмена во время
  скрипта у `ai`/`drain`-шага). **Ловушка:** dedupe обязан отличаться от `stop:<n>` — им же ставится остановка
  сессии, `INSERT OR IGNORE` молча выбросит вторую. Взять `stop_exec:<n>`.

### 4.4 Повтор продолжает сессию — `machine.rs::start_attempt`

Сейчас «вход в шаг» = «прошлая попытка была другого шага». При повторе шаг тот же → сессия не продолжилась бы.
Заменить запрос `previous`:

```sql
SELECT NOT EXISTS (SELECT 1 FROM attempts WHERE run = ?1 AND n = ?2 AND step = ?3
                   AND (outcome IS NULL OR failure IS NOT NULL))
```

`entering` истинно, если прошлая попытка другого шага **или довела этот шаг до исхода без сбоя**. Дальше ничего не
менять: ветка `_ if entering => own…` даёт `SessionPlan::Resume` и полный промпт, не `RECONCILE`. Файл — 383 строки:
замена не длиннее старого запроса.

То же для входа в `drain` (`drain.rs`, подзапрос `kids`): `step != ?2` →
`(step != ?2 OR (outcome IS NOT NULL AND failure IS NULL))`. Повтор `drain`-шага — новый вход: открытые биды выдаются
снова [Д].

### 4.5 Замечание повтора — `render.rs`

- `remarks()` — оба столбца: `TRIM(COALESCE(remarks,'') || char(10,10) || COALESCE(step_remarks,''), char(10))`.
  Идёт в `{{ remarks }}` и `AAE_REMARKS`.
- В шаблоне нет `{{ remarks }}` (`prompt.template.paths()`), а `row.step_remarks` есть — дописать его к промпту через
  пустую строку. Иначе критерий «получает вывод скрипта замечанием» зависел бы от шаблона чужого пака. Только для
  `step_remarks`: поведение `back_to` не меняется [Д].

### 4.6 Восстановление — `recover.rs`

`finishing` без `waiting` теперь устойчивое состояние «идёт скрипт» (раньше было только внутри транзакции). Сейчас
такой прогон получил бы `daemon_crash` и повтор хода — нарушение §5.6 («ход не повторяется»). В цикле `caught`,
после общей чистки эффектов `exec:<run>` (она уже убивает скрипт по маркеру):

```rust
if *waits || row.state == "finishing" {
    // handle = NULL, дедлайны state/silence сняты — как сейчас
    if !*waits { self.check(tx, row)?; }   // скрипт заново; исполнение не засчитано, пока его слово не записано
    continue;
}
```

Итог границы: скрипт отработал, слово не записано → после рестарта он идёт ещё раз, счётчик не тронут; слово
записано → счётчик и замечание уже в состоянии, скрипт не повторяется.

### 4.7 Шов для стендов — `hooks.rs`

Хаос-стенд идёт на остановленных часах и процессов не запускает. В `HookRunner`:

```rust
/// How the command of an `exec` effect ended, when a test stand plays it instead of running it.
fn played(&self, _payload: &Value) -> Option<Exit> { None }
```

`exec_step::run` получает `&dyn HookRunner` вместо `socket` и при `Some(exit)` возвращает `ExecDone` без процесса.
`hooks.rs` — 384 строки, добавка ≤ 5.

## 5. События на каждое исполнение

Новых типов нет. В `RunView` всех событий — `step_attempt`.

| что произошло | события |
|---|---|
| шаг отработал сам | `run.finishing {run, outcome}` — как сейчас |
| не прошёл, повтор | `run.step_finished {run{step_attempt: k}, outcome: fail}` → `run.state_changed → preparing` → `run.preparing {run{step_attempt: k+1}}` |
| скрипт 0 | `run.step_finished {outcome: pass}` или `run.passed` |
| предел исчерпан или код 75 | `run.failed {class: check \| agent, reason, outcome}`; у шага с `back_to` — `run.step_finished` и новый раунд |

`config/schema.rs`: `("step_attempt", Leaf)` в `RUN`. Вывод прошедшего скрипта в события не попадает.

## 6. Конфиг

- `formula_file.rs`: в каждом варианте `StepFile` — `max_attempts: Option<NonZeroU32>`,
  `step_check_script_path: Option<String>` (`flatten` с `deny_unknown_fields` не дружит — дублировать, как
  `timeout`). `Step` получает `max_attempts: u32` (умолчание 1) и `check: Option<PathBuf>`.
- `items.rs::formula`: пустая строка — нет скрипта; путь — относительно каталога файла формулы (`parent(file)`),
  абсолютный допустим. При загрузке: обычный файл с битом исполнения, иначе ошибка одной строкой:
  `` step `build`: the check script `<путь>` is not an executable file ``.
- `max_attempts = 0` и не число отвергает serde с местом в файле; тест закрепляет, что строка одна.
- `aae-front/src/api/config.rs` («объясни конфиг»), рядом со строкой про `back_to`:
  `# runs at most 3 times; passes when <script> exits 0` (части — по наличию).
- Демо не меняются: ключи необязательны.

## 7. Тесты

### 7.1 Ядро, конфиг — `aae-core/src/tests/config_steps.rs` (147 строк)

1. `a_step_of_any_kind_takes_max_attempts_and_a_check_script` — три вида шага; пустая строка = нет; путь от файла
   формулы.
2. `a_bad_max_attempts_or_check_script_is_one_line` — `0`, нет файла, файл без бита исполнения.

### 7.2 Testkit — новый `tests/core/step_repeat.rs` (≤ 350 строк)

`common/mod.rs`: файлы `*.sh` писать с режимом `0o755` (2 строки). Тестовый раннер `Played`: очередь `Exit`, список
payload'ов, необязательный `AbortHandle` писателя; очередь пуста → `Code(0)`. Остановленные часы, сценарный драйвер:

1. `a_step_runs_again_until_its_check_script_passes` — `max_attempts = 3`, выходы 1, 1, 0 → `passed`. Вторая и
   третья попытки — `Resume` той же сессии; `driver.sent` = шаблон + замечание (шаблон `FILES` без
   `{{ remarks }}` — проверяет дописывание); `round == 1`; в `attempts` нет `failure`; `step_attempt` в
   `run.step_finished` — 1, 2.
2. `past_max_attempts_the_step_fails` — `max_attempts = 2`, выходы 1, 1 → `failed{check}`, причина
   `check script: exit 1`, скрипт вызван ровно дважды.
3. `exit_75_of_the_check_script_fails_the_step_at_once` — `max_attempts = 3`, выход 75 → один вызов, повтора нет.
4. `a_step_that_fails_on_its_own_runs_again_without_its_script` — финишер `fail`, потом `pass`: скрипт вызван один
   раз; замечание — резюме финишера; при `max_attempts = 1` — `failed{agent}`.
5. `a_repeat_stays_in_its_round_and_a_round_starts_the_count_again` — шаг 2 с `back_to` возвращает на шаг 1:
   `round == 2`, `step_attempt` снова 1, `step_remarks` пуст.
6. `a_check_script_cut_by_a_crash_runs_again_and_counts_once` — `Played` на первом вызове рвёт писателя; рестарт;
   `max_attempts = 2`, выходы 1 (потерян), 1, 0 → `passed`, три вызова. Засчитай потерянный — прогон бы упал.
7. `a_crash_between_attempts_keeps_the_count` — `max_attempts = 2`; после первого `run.step_finished` рестарт
   посреди хода второго исполнения, решение `resume` (`POLICY` + `decide`), `pass`, выход 1 → `failed{check}`,
   ровно два вызова. Сбросься счётчик — третий вызов дал бы 0 и `passed`.
8. `a_repeat_of_a_drain_step_is_a_new_entry` — `drain`, `max_attempts = 2`, выходы 1, 0; дочерний прогон бида в
   первом входе провален → во втором бид выдан снова (помощники `common/drain.rs`).

Настоящее время, настоящий процесс (как `exec.rs`):

9. `a_check_script_gets_the_step_and_its_output_is_the_remark` — `exec`-шаг + `check.sh` рядом с формулой: скрипт
   видит `AAE_RUN`, `AAE_STEP_<ИМЯ>_RESULT=pass`, каталог прогона; первый раз печатает текст и выходит с 1; вторая
   команда шага получает `AAE_REMARKS` с этим текстом.
10. `a_cancel_stops_the_check_script_and_the_session` — `ai`-шаг, скрипт `sleep 30`; отмена в `finishing`: процесс
    скрипта исчез, драйвер увидел закрытие сессии (ловит ошибку с dedupe из 4.3).

### 7.3 Приёмка — критерий 29, две строки `Launch::Ci`

- Сценарий `scenarios/c29_repeat.rs` (по образцу `c25_return.rs`, ≤ 130 строк), настоящий демон, `exec`-шаги:
  - `check.sh` считает вызовы в файле и проходит с третьего. Формулы `third` (`max_attempts = 3`) и `second`
    (`= 2`): первая — `passed`, три вызова; вторая — `failed`, два;
  - формула `fatal`: скрипт `exit 75`, `max_attempts = 3` → `failed`, один вызов;
  - `kill -9`: команда шага на втором исполнении создаёт `reached` и ждёт файла `go`; сценарий ждёт `reached`,
    `d.restart()`, создаёт `go`. `third` доходит до `passed` ровно за три вызова скрипта, `second` — до `failed`
    за два; `view["step_attempt"]` в конце — 3 и 2.
- Строка `tests(…)`: тесты 1, 3, 4, 6, 7 из 7.2 и оба из 7.1 — часть про `ai`-шаг и сессию.
- `"29"` убрать из `OPEN` (`readiness.rs`). Фильтр обязан выбрать ровно названное число тестов.

### 7.4 Хаос-стенд — subcoder, после шага 3 (файлы только `tests/chaos/*`)

Основание — §14: `kill -9` на каждой границе транзакции. Файлы у предела (`steps.rs` 387, `main.rs` 369,
`check.rs` 366): новое — в новый `tests/chaos/repeat.rs`.

- Второму шагу `SHIP`: `max_attempts = 2`, `step_check_script_path = "ok.sh"`; файл `shop/formulas/ok.sh`.
- `Hooks::played`: выход по `seed ^ hash(run, attempt)` — чаще 0, иногда 1, редко 75. Один и тот же при повторе.
- `undone()`: эффект `op = "exec"` → вид `check`; `KINDS` + `"check"`: каждая полная доля сидов убивает демона,
  когда скрипт записан и не отработал.
- S1: `run.step_finished` с `outcome.result == "fail"` — повтор, шаг в `done` не кладётся (в `step_finished`
  передать данные события).
- S5 (новый): повторов шага в раунде ≤ `max_attempts − 1`; `runs.step_attempt ≤ max_attempts`; после слова 75
  шаг больше не исполняется.
- `a_seed_replays_the_same` остаётся зелёным.

Не влезло в цикл — запись в журнал и первый пункт цикла 99; статус WP-80 тогда `in progress`, а не `done`.

## 8. Доки (английский)

- `docs/design/config.md`, «Formula»: два ключа в образце; абзац после абзаца о `back_to` — что считается неудачным
  исполнением, коды 0 / 75 / иное, **разница с кодом 75 команды `exec`-шага** (там — сбой попытки, здесь —
  неизлечимо), путь, окружение, таймаут, замечание и `{{ remarks }}`, счёт внутри раунда, повтор `drain` — новый вход.
- `docs/design/contracts.md`: 1.3 `RunView` (`step_attempt`); `run.failed` — класс `check` включает скрипт; 6.1 —
  строка `finishing` (идёт скрипт, дедлайн — таймаут шага); 6.2 — переходы из `finishing`; потолки — повтор их не
  расходует; 6.4 — `finishing` со скриптом после рестарта.
- `docs/design/invariants.md`: инвариант 16 — исполнения шага ограничены `max_attempts` и лежат в состоянии; абзац о
  стенде (S5, вид `check`); «Recovery».
- `docs/design/event-model.md`, «State»: два столбца.
- `docs/guide/first-formula.md`: короткий пример рядом с `back_to`.
- `docs/api/openapi.json`: `step_attempt` в схеме прогона, рядом с `round`.

## 9. Порядок коммитов

1. `WP-80: a step takes max_attempts and step_check_script_path` — `formula_file.rs`, `items.rs`, «объясни
   конфиг», тесты 7.1, `*.sh` в `common/mod.rs`, образец и ключи в `config.md`.
2. `WP-80: a step that did not pass runs again within max_attempts` — миграция 22, `row.rs`, `schema.rs`,
   `finish.rs`, `repeat.rs` (`repeat`), `render.rs`, `machine.rs` (`entering`), `drain.rs`; тесты 4, 5, 8.
3. `WP-80: the check script of a step decides whether it passed` — `exec_step.rs` (`command`), `msg.rs`, `mod.rs`,
   `exec.rs`, `hooks.rs` (`played`), `repeat.rs` (`check`, `check_done`), `events.rs`, `machine.rs` (`stop_exec`),
   `recover.rs`; тесты 1–3, 6, 7, 9, 10.
4. `WP-80: acceptance row 29` — `c29_repeat.rs`, строка `tests`, `OPEN`.
5. `WP-80: document step repeats and the check script` — раздел 8.
6. `WP-80: the chaos stand kills the daemon around a check script` — 7.4 (subcoder).
7. READINESS одним коммитом, статус в `PLAN.md`.

После шага 3 — `helper` с мутациями до отчёта: `<` → `<=` в `repeat`; `fatal` не учтён; `step_attempt` не
сброшен в `enter`; `round + 1` при повторе; `recover` без повторного запуска; старый запрос `entering`; `stop_exec`
убран; класс `agent` вместо `check`; скрипт при собственном провале; подзапрос `drain`; дописывание замечания.

## 10. Файлы и бюджет

| файл | сейчас | после |
|---|---|---|
| `run/repeat.rs` (новый) | — | ~80 |
| `run/finish.rs` | 229 | ~240 |
| `run/exec_step.rs` | 221 | ~240 |
| `run/machine.rs` | 383 | ≤ 388 |
| `run/hooks.rs` | 384 | ≤ 389 |
| `run/events.rs` | 376 | 377 |
| `run/recover.rs` | 134 | ~140 |
| `run/render.rs` | 104 | ~115 |
| `run/row.rs`, `store.rs`, `msg.rs`, `mod.rs`, `exec.rs`, `drain.rs`, `config/schema.rs` | | по 1–8 строк |
| `config/formula_file.rs` | 135 | ~150 |
| `config/items.rs` | 315 | ~340 |
| `aae-front/src/api/config.rs` | 286 | ~292 |

Ядро: 10 555 → ~10 730 из 18 000. `budget.sh` теперь считает и тесты: каждый новый тестовый файл ≤ 400.
`machine.rs` и `hooks.rs` — самые тесные: если не лезет, остановку скрипта вынести в `repeat.rs` функцией.

## 11. Допущения [Д] — одной записью в журнал (делает кодер)

1. Собственный провал шага — неудачное исполнение без запуска скрипта.
2. Вето хука на `run.finishing` повтором не лечится; `hollow_done` и сбои инфраструктуры `max_attempts` не расходуют.
3. Скрипт идёт после хуков `run.finishing`; его таймаут — таймаут шага; таймаут и сигнал — «не прошёл», невозможность
   запустить — `failed{infrastructure}`.
4. Провал из-за скрипта — класс `check`.
5. Скрипт видит исход своего шага в `AAE_STEP_<ИМЯ>_*`.
6. Замечание повтора дописывается к промпту, если в шаблоне нет `{{ remarks }}`.
7. Повтор `drain`-шага — новый вход.
8. Шов `HookRunner::played` — код продукта ради стенда; прецедент — сам трейт `HookRunner`.
9. Исход исполнения, отвергнутого скриптом, переписывается: собственное резюме шага в нём не хранится.
10. Отдельного события «скрипт начал работу» нет.

## 12. Не проверено

- Ничего не собиралось и не запускалось: план по чтению кода.
- Форма `env` в снимке `ai`-шага (`{value, from}`) — по `assemble.rs` и `machine.rs::prepared`, не прогоном.
- Порядок «`abort` писателя раньше приёма `ExecDone`» в тесте 6 — по устройству `current_thread`; если гонка есть,
  `Played` рвёт писателя и не отвечает (возврат через `pending` недоступен синхронной функции — тогда тест 6 перенести
  на настоящий скрипт с файлом-воротами).
- Вне пакета: жел. 1 ревью 96 и 97 (метки бида и шаблон агента при выдаче) — WP-81, где правится выдача.
