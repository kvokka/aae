# WP-102 · шаг `await` — план реализации (expert, цикл 114)

Источник: `.loop/PLAN.md:393-430`, решения 126/127/120/116/62/88, contracts §1.3, §6.1–6.3, config.md «Formula»,
invariants.md «Recovery». Номера строк — на HEAD `ac68778`.

## 0. Суть в пяти строках

- Ожидание = `runs.state = 'running'` + `step_kind = 'await'` + строка `deadlines` `<run>:state`. Новых колонок,
  таблиц, состояний и таймеров нет. Попытка без процесса, `harness = 'await'` (как `drain`/`formula`).
- Таймаут — существующий механизм дедлайнов писателя (`Tx::set_deadline`, `crates/aae-core/src/writer.rs:116`):
  писатель просыпается ровно в `at_ms`, опроса нет (решение 62). Время стены: строка лежит в БД и переживает рестарт.
- Исход снаружи — `Msg::Resolve` → существующий `step_done` (`exec_step.rs:163`) → обычный `finishing`. Весь возврат
  на `back_to`, раунды, remarks, запрет remark-решения, `run.step_finished` работают без правок.
- Правок существующей логики четыре, по одной строке: `NO_PROCESS`, выборка `recover`, ветка `on_deadline`, условие
  конца ряда проверок в `advance`.
- Ядро: +~120 строк (10 930 → ~11 050 из 18 000). Фронт: +~110 (2 669 → ~2 780 из 4 000). Бюджет не риск; риск —
  лимит 400 строк на файл (см. §6).

## 1. Движок: что трогается

Новый файл `crates/aae-core/src/run/await_step.rs` (~55 строк; `mod await_step;` в `run/mod.rs:4-28`), три функции:

- `start_await(tx, row)`: `Self::run_attempt(tx, &mut row, "await")` (`exec_step.rs:148`); `at = now +
  row.timeout_ms.unwrap_or(AWAIT_TIMEOUT_MS)` (`saturating_add`, как `formula_step.rs:15-16`);
  `tx.set_deadline(&row.deadline("state"), at)`; `row.emit("run.awaiting", {run: row.view(), deadline:
  rfc3339_ms(at)})` (формат как у `deadline` решающих событий, `decide.rs:59`); в конце `self.schedule(tx)` —
  освобождает слот (как `drain.rs:44`; у `formula` это делает `create`).
- `resolve(tx, run, step, outcome)`: `Row::load` (нет прогона → `NotFound` → 404, как `cancel`, `events.rs:25`);
  охрана `row.state != "running" || row.step_kind != "await" || row.step != step` → `rejected(...)` с текстом, где
  прогон стоит; `summary > MAX_SUMMARY` или `data > MAX_DATA` → `rejected` (константы `finish.rs:15-16` сделать
  `pub(super)`, длина data — как `finish.rs:47`); иначе `self.step_done(tx, row, outcome)`.
- `await_timeout(tx, row)`: снимок `row.config`; `config["fail_on_timeout"] == false` → `step_done` с
  `Outcome { Pass, "timeout", None }`; иначе `self.terminal(tx, row, "failed", Some("check"), "await_timeout")`
  (напрямую, без `advance` — возврата на `back_to` нет).

Правки существующего:

| Файл:строка | Правка |
|---|---|
| `run/machine.rs:100-104` | ветка `"await" => return self.start_await(tx, row),` (файл 396 строк → 397, больше туда ничего) |
| `run/drain.rs:29` | `NO_PROCESS`: `step_kind IN ('drain', 'formula', 'await')` — этим закрыты слот (`queue.rs:374 started()`) и `active_runs` дрейна (`read.rs:114-120`); поправить комментарии `queue.rs:299,373`, `read.rs:112` |
| `run/recover.rs:65-70` | в `WHERE` выборки `caught` добавить `AND NOT (r.step_kind = 'await' AND r.state = 'running')`: ждущий прогон не получает `daemon_crash`, попытка и дедлайн остаются. `rearm` (`recover.rs:146`) двигает только `preparing` — дедлайн ожидания не трогает. Ждущий `await` в `finishing` (после исхода) идёт общим путём `waits` — править не надо |
| `run/events.rs:294` | перед общей веткой: `("state", "running") if row.step_kind == "await" => self.await_timeout(tx, row),` |
| `run/finish.rs:136` | конец ряда: `next.is_none_or(\|n\| n.back_to.is_none() \|\| matches!(n.kind, StepKind::Await { .. }))` — упавшие проверки возвращают прогон до входа в `await`. Сам `await` с `back_to` — проверка: `checks` (`finish.rs:127`) берёт его и прошедшие перед ним, remark `"<шаг>: <summary>"` даёт `remark()` (`finish.rs:258`) |
| `run/render.rs:36-85` | ветка `(StepKind::Await { timeout, fail_on_timeout }, "await")` → `Ok((None, timeout_ms(timeout, AWAIT_TIMEOUT_MS), json!({ "fail_on_timeout": fail_on_timeout })))`; `const AWAIT_TIMEOUT_MS = 7 * 24 * 3_600_000` рядом с `DRAIN_TIMEOUT_MS` (`render.rs:15`). `env` в снимке не нужен: процесса и check-скрипта нет (`machine.rs:76-86` это переживает) |
| `run/queue.rs:32` | `StepKind::Await { .. }` в ветку «без агента» (матч исчерпывающий — без этого не соберётся) |
| `run/msg.rs` (~165) | `Msg::Resolve { run: String, step: String, outcome: Outcome }` |
| `run/mod.rs:140` | `Msg::Resolve { run, step, outcome } => self.resolve(tx, &run, &step, &outcome),` (ответ `null`, как `Cancel`) |

Не трогается и почему работает: `fail` → `finished` (`finish.rs:110`) → `repeat` (`repeat.rs:69`, `max_attempts`
у `await` всегда 1) → `advance(Some(("agent", summary)))`: с `back_to` — возврат и `round + 1`, без — `failed{agent}`
с причиной `summary`. Remark-решение на `run.finishing` уже запрещено всем, кроме `ai` (`decide.rs:138,248`).
Check-скрипта нет (`repeat.rs:21`, `check = None`). Отмена — `cancel` → `terminal` → `end_attempt` чистит дедлайн
(`machine.rs:315`). Закрытый гейт или стоп демона на входе — `held` → `park` раньше `start_await` (`machine.rs:97`).

Рестарт: попытка `await` не прерывается, второго `run.awaiting` нет; дедлайн, истёкший при лежащем демоне, срабатывает
при старте (строка `deadlines`). Часы в тестах: `Clock::managed(0)` в `World` (`tests/common/mod.rs:142`) идёт по
часам tokio — `#[tokio::test(start_paused = true)]` + `tokio::time::sleep` / `w.restart_after(idle)`
(`common/mod.rs:212`). Ничего нового в testkit не нужно.

## 2. Модель данных

Конфиг (`aae-contract`):
- `config/formula_file.rs:49-95` `StepFile::Await { name: Option<String>, timeout: Option<String>,
  fail_on_timeout: Option<bool>, back_to: Option<String> }`. `deny_unknown_fields` на enum уже даёт ошибку загрузки
  `unknown field \`agent\`` для `agent`/`command`/`formula`/`max_attempts`/`step_check_script_path` — отдельной
  валидации не писать (образец теста — `tests/config_steps.rs:148,187`).
- `formula_file.rs:152-198` `StepKind::Await { timeout: Option<Duration>, fail_on_timeout: bool }`, `name()` → `"await"`.
- `config/items.rs:121-148` ветка `StepFile::Await { name, back_to, .. } => (name.clone(), back_to.clone(), None,
  None)`; `items.rs:230-285` → `StepKind::Await { timeout: self.timeout(..), fail_on_timeout:
  fail_on_timeout.unwrap_or(true) }`. Дефолт 168h — в ядре (`AWAIT_TIMEOUT_MS`), как у остальных видов.
- `config/check.rs:67` — `StepKind::Exec { .. } | StepKind::Await { .. } => {}`.
- `config/schema.rs:~113` — `"run.awaiting" => &[("run", RUN), ("deadline", Leaf)]` (этим событие доступно триггерам
  и хукам плагинов).

Событие: `run.awaiting {run: RunView, deadline}` — наблюдаемое, один раз на вход в шаг. `RunView.step_kind` получает
значение `"await"`, `agent` = null. Причина: `run.failed{class: "check", reason: "await_timeout"}`.

Исход: хранится там же, где у всех шагов, — `attempts.outcome` текущей попытки (`step_done`), `Outcome {result,
summary, data}`. Лимиты 4 KiB / 64 KiB — те же константы, что у финишера. Идемпотентность — общий механизм API:
заголовок `Idempotency-Key` → `send_request(key(&headers), msg)` (`api/runs.rs:149-153`, таблица `idempotency`,
ключи живут сутки, `writer.rs:19`); повтор с тем же ключом возвращает первый ответ. Без ключа повтор безопасен в
пределах раунда (прогон уже не `running` на этом шаге → `rejected`), но после возврата на тот же `await` запоздавший
дубль применился бы ко второму раунду — поэтому мост обязан слать ключ; это написать в `frontends.md`.

Состояние прогона для чтения (`run/read.rs`):
- `RunFilter.awaiting: Option<bool>`; в SQL `read.rs:77` добавить `AND (?5 IS NULL OR (step_kind = 'await' AND
  state = 'running') = ?5)`.
- `Reader::run` (`read.rs:57-68`): `view["steps"] = json!(super::render::step_results(&conn, id, &row.step)?)` —
  исходы остальных шагов по имени, последняя попытка каждого; та же функция и та же семантика, что `{{ steps.* }}` в
  шаблоне (`render.rs:133`). Сейчас исходов прошлых шагов в виде нет (`attempts` их не несёт, `outcome` — только
  текущей попытки).

## 3. API / CLI

- `POST /v1/runs/{id}/resolve`, тело `ResolveRun { step: String, outcome: Outcome }` (как `FinishRun`,
  `api/runs.rs:157-164`), заголовок `Idempotency-Key`. Ответы — точная копия `cancel` (`api/runs.rs:134-139`): 200
  `null`; 409 `rejected` (не на этом `await`, лимиты); 404 `not_found`; `Keyed`. Маршрут — `api/mod.rs:~92`.
  `crates/aae-front/tests/openapi.rs:49-80` требует 400/401/404/422/503 — копия атрибутов `cancel` их даёт.
- `GET /v1/runs?awaiting=true` — поле `awaiting: Option<bool>` в `ListQuery` (`api/runs.rs:87-97`).
- CLI (`cli/commands.rs`): `RunCommand::Resolve { id, step, result: Verdict, #[arg(long, default_value = "")] summary,
  #[arg(long)] data: Option<String>, #[arg(long = "key")] idempotency }` → `aae run resolve <run> <step> pass|fail
  [--summary <text>] [--data <json>|@<file>] [--key <key>]`; печатает как `cancel` (`commands.rs:196-209`). Разбор
  `--data` вынести из `cli/finish.rs:32-42` в `pub(super) fn data_arg(..) -> Result<Value, String>` и звать из обоих.
  `RunCommand::List` + `#[arg(long)] awaiting: bool` → `("awaiting", awaiting.then(|| "true".into()))`
  (`commands.rs:163-168`). В текстовую строку списка (`commands.rs:180-186`) добавить `text(&run["step"])`: без имени
  шага человек не соберёт команду `resolve`.
- `cli/mod.rs:20` `OPERATIONS` += `("run resolve", "POST", "/v1/runs/{id}/resolve")` (тест `openapi.rs:22`).
- `api/replies.rs:12-52` `Run`: поле `steps` (`#[schema(value_type = Object)]`), комментарии `step_kind`/`agent`
  (`replies.rs:19-21`) — добавить `await`. `api/config.rs:204-231` (explain) — ветка `StepKind::Await`: строка
  `# step \`{name}\` of \`{formula}\`: await an outcome from outside`.
- `docs/api/openapi.json`: `AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`.
- MCP не трогать; `steps` попадёт в ответ `get_run` MCP сам (тот же `Reader::run`) — проверить, что
  `crates/aae/tests/e2e/mcp.rs` и `api/tests.rs` не сравнивают вид прогона целиком.

## 4. Коммиты и объём

Два коммита (конфиг без движка дал бы шаг, который грузится и падает «changed its kind» — отдельным коммитом не
выделять):

1. **`WP-102: an await step waits for an outcome from outside`** — `aae-contract` (§2), `aae-core/src/run` (§1, без
   `read.rs`), минимальная ветка в `aae-front/src/api/config.rs` (иначе не соберётся), тест контракта, тесты ядра,
   документы: config.md «Formula», contracts §1.3 (событие, `step_kind`), §6.1 (строка `running`, слот), §6.2
   (переходы `preparing (await)`, `running (await)` × исход/дедлайн; «remark не разрешён» строка 743; строка 748),
   invariants.md «Recovery» п. 3, `overview.md:39`, `architecture.md:65`.
2. **`WP-102: aae run resolve and the list of waiting runs`** — `read.rs` (фильтр, `steps`), API, CLI, e2e,
   `openapi.json`, contracts §1.3 (чтение: `steps`) и §6.3 (абзац «исход снаружи»), `frontends.md` (поверхность,
   CLI, требование ключа для повторов), `overview.md:70` — строка «A human gate as a formula step» уходит.

Оценка: ~280 строк продукта, ~330 строк тестов, ~70 строк документов. Чтение по этому плану ~40k, правки и
итерации тестов ~60k, документы ~20k, `ci` ~10k — **~130k, в одну сессию влезает**, но без запаса на отладку.
Точка разреза, если после коммита 1 контекст кодера > 200k: **WP-102a** = коммит 1 («Готово, когда»: все тесты ядра),
**WP-102b** = коммит 2 («e2e на CLI», чтение, openapi, overview). Гейт `scripts/loop/checks 114` — один раз в конце.

## 5. Тесты

Контракт, `crates/aae-contract/src/tests/config_steps.rs` (335 строк; образец `:116-150`):
- `an_await_step_takes_timeout_fail_on_timeout_and_back_to` — дефолты (`timeout: None`, `fail_on_timeout: true`,
  `name() == "await"`), заданные значения, и таблица: каждый из `agent`, `command`, `formula`, `max_attempts`,
  `step_check_script_path` → `unknown field`.

Ядро, новый `crates/aae-testkit/tests/core/await_step.rs` (+ `mod await_step;` в `core/main.rs`). Формулы из одних
`await` — на паузе (`start_paused = true`, образец `formula_step.rs:101-119`); где нужен `AAE_REMARKS` — exec в
реальном времени (`#[tokio::test]`, хелперы-образцы `returns.rs:17-35,54`, `w.until(Some(&run), "run.awaiting", n)`
`common/mod.rs:235`). Исход — `w.core.send(None, Msg::Resolve { .. })`.

| «Готово, когда» | Тест | Что утверждает / какую охрану краснит |
|---|---|---|
| `pass` → следующий шаг | `a_pass_from_outside_moves_the_run_to_the_next_step` | `[gate, ship]` оба `await`: `run.awaiting.deadline` = вход + timeout; `run.step_finished.outcome`; второй `run.awaiting` на `ship`; `run.passed` |
| `fail` → `back_to` с remarks | `a_fail_sends_the_run_back_with_its_summary_as_remarks` | `[work(exec, печатает $AAE_REMARKS), gate(back_to work)]`: лог `work []\nwork [gate: redo]\n`, `round == 2`, затем `pass` → `passed` |
| `fail` без `back_to` | `a_fail_without_back_to_fails_the_run` | `run.failed{class: agent, reason: <summary>}` |
| `fail_on_timeout = true` | `a_wait_past_its_timeout_fails_the_run_with_no_return` | `[a, gate(back_to a, timeout 1h)]`: `run.failed{check, await_timeout}` ровно через 1h (сверка `ts`, образец `watchdog.rs:44-54`), третьего `run.awaiting` нет, `run.attempt_failed` нет (мутация ветки `on_deadline` даёт `runaway_turn`) |
| `fail_on_timeout = false` | `a_wait_past_its_timeout_passes_when_told_so` | `run.passed`, `outcome = {pass, "timeout"}` |
| исход для другого шага | `an_outcome_for_another_step_is_rejected_and_changes_nothing` | `Error::Rejected` для: чужого шага; своего шага прогона на `ai`-шаге в `running` (формула `build` из `FILES`, шаг `"1"`); повторного исхода после `passed`; `summary` 4 KiB + 1; `data` > 64 KiB. `Error::NotFound` для чужого id. Число событий прогона не изменилось. По одному утверждению на каждое условие охраны `resolve` |
| рестарт во время ожидания | `a_restart_leaves_the_wait_and_its_deadline_alone` | `POLICY`-мир, два прогона, `restart_after(20 min)`: `daemon.started.interrupted_runs == 0`, нет `run.attempt_failed`, по одному `run.awaiting`; первый после рестарта принимает `pass`; второй падает `await_timeout` в исходный дедлайн (образец `formula_step.rs:157-182`) |
| `max_runs = 1` | `a_waiting_run_leaves_its_slot_to_the_next_run` | конфиг `formula_step.rs:214-217`; второй прогон доходит до своего `run.awaiting`, пока первый ждёт |
| упавшая проверка перед `await` | `a_failed_check_before_an_await_sends_the_run_back_first` | `[work, test(exec, падает 1 раз, back_to work), gate(back_to work)]`: к первому `run.awaiting` лог `work\ntest\nwork\ntest\n`, `run.awaiting` один, `round == 2` (образец `returns.rs:24-28,100-118`) |

e2e, новый `crates/aae/tests/e2e/await_step.rs` (`cli.rs` 393 и `api.rs` 396 строк — туда нельзя; `mod` в
`e2e/main.rs`; образец `cli.rs:10-33,35-90`, `Daemon::start(|dir| ..)` пишет формулу `gate.toml`: `gate(await)`,
`done(exec true)`):
- `an_outcome_from_the_cli_moves_a_waiting_run_on` — `run start`; `until_state(running)`; `run list --awaiting --json`
  содержит прогон и не содержит параллельный `slow`; `run resolve <run> nope pass --json` → exit 1, код `rejected`,
  состояние прежнее; `run resolve <run> gate pass --summary ok --data '{"pr":1}' --key r1` дважды (повтор — тот же
  ответ); `until_state(passed)`; `run show` → `steps.gate == {pass, ok, {pr:1}}`; `run list --awaiting` пуст.
- Если влезает: в `e2e/replies.rs:117-160` два случая в таблицу ошибок (`/resolve` 404 и 409).

Правило цикла: каждую свою охрану мутировать до коммита — список охран в правом столбце таблицы плюс `NO_PROCESS`
(тест слота) и исключение в `recover` (тест рестарта).

## 6. Ловушки

- **400 строк на файл** (`scripts/budget.sh:39`, считает `wc -l`): `config/items.rs` 397 — правка даёт ~+5 → перенести
  свободные `string`/`agent_keys` (`items.rs:347-397`) к их второму пользователю `config/variants.rs` (184 строки) или
  ужать иначе; `run/machine.rs` 396 — только одна строка; `run/queue.rs` 384 — после rustfmt ветка может занять +4;
  `cli/mod.rs` 379 (+1); e2e `cli.rs`/`api.rs` — новый файл.
- **Инвариант 11 / recovery п. 3**: «`running` после рестарта → `daemon_crash`» перестаёт быть верным для `await`;
  текст invariants.md и шапку `recover.rs:1-5,61-64` править в коммите 1. D3 хаос-стенда про `drain` — не затронут
  (формул с `await` в стенде нет; добавлять не просили).
- **Инвариант 14**: у ожидания есть дедлайн и терминальный выход (`await_timeout`) — `set_deadline` обязателен на
  каждом входе, включая вход после `unpark`.
- **Инвариант 12**: `pass` по таймауту — явный исход, объявленный ядром (решение 127), идёт через `run.finishing`.
- **Инвариант 18**: исход при закрытом гейте оператора принимается, но следующий шаг паркуется в `start_attempt` —
  проверено чтением, отдельный тест не просили.
- **Инвариант 10**: см. §2 — дубль без ключа после возврата на тот же `await`.
- После исхода прогон в `finishing` снова считается занятым слотом, как после `drain`: `max_runs` может быть превышен
  на время хуков — существующее поведение, не чинить.
- Проверки ПОСЛЕ упавшего `await` (`[gate(back_to), lint(back_to)]`) выполняются и возвращают вместе с ним — «проверка
  как любая другая»; `await` закрывает только ряд перед собой. Написать одной фразой в config.md.
- config.md сейчас говорит «Any step may have `max_attempts`» и «`back_to` … ai and exec steps» — оба места править;
  тест `a_step_of_any_kind_takes_max_attempts_and_a_check_script` (`config_steps.rs:215`) оставить как есть.
- `docs/` — только по-английски и только «как сейчас», без номеров пакетов.

## 7. Допущения (в журнале, запись expert цикла 114)

1. Класс отказа по таймауту — `check` (по образцу `hook_deadline` и кода 75 check-скрипта: проверка не ответила,
   возврата нет), а не `infrastructure`.
2. Имена: `POST /v1/runs/{id}/resolve`, `aae run resolve`, фильтр `awaiting` / `--awaiting`, поле `steps`.
3. Превышение лимитов `summary`/`data` — `rejected` (409), а не отдельный код.
4. `fail_on_timeout` и `timeout` берутся из снимка на входе в шаг: перезагрузка конфига ждущий шаг не меняет.
5. `--summary` у `resolve` необязателен (пустая строка); remark тогда `"<шаг>: "`.

Не делается: срок ожидания в `GET /v1/runs/{id}` (он есть в `run.awaiting`), `await` в хаос-стенде, тест отмены
ждущего прогона, авто-ключ идемпотентности в CLI (одинаковый вердикт во втором раунде был бы проглочен).
