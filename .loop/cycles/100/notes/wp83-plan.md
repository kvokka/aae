# WP-83 · «Фон после исхода» · план для subcoder (цикл 100, expert)

Спека: §9.4 (абзац «Фон после исхода» [Р]), решение 104, критерий 34 (CI, поддельный провайдер); критерии 4, 4а, 8
остаются зелёными. Опыт сделан в цикле 98: `docs/design/experiments.md`, раздел «Background work after the outcome
(experiment 8, criterion 34)» — прочитай его целиком (80 строк), остальную спеку читать не нужно.

Рядом в том же дереве coder делает WP-81. **Его файлы не трогать**: `crates/aae-core/src/run/{drain,decide,beads,
machine,recover}.rs`, `crates/aae-core/src/config/{items,formula_file}.rs`, тесты `drain*`. Ты не коммитишь.

## 1. Что сейчас и что должно стать

Сейчас (`crates/aae-core/src/run/events.rs`, `attempt_event`): адаптер после хода шлёт `TurnEnded`, затем
`State{Idle, background}`. При `background: true` ядро только взводит дедлайн `silence` на `background_silence_ms`
(30 мин) и ждёт `State{Idle, background:false}`; на нём `turn_settled` переводит прогон в `finishing` (исход есть) или
поднимает `run.turn_ended_without_outcome`. Оставленный `http.server` держит прогон до 30 минут.

Должно стать:

- Исход заявлен **и** ход закончен **и** пришло `Idle{background:true}` → ядро ставит эффект `stop_background`;
  исполнитель зовёт новый метод драйвера `stop_background(instance, session)`; адаптер останавливает оставленные
  **команды** средствами харнесса; харнесс сам сообщает об их конце, поток адаптера даёт
  `State{Idle, background:false}`; дальше работает существующий `turn_settled` → `finishing`. Сессия жива: замечание
  вето уходит в неё следующим промптом.
- Переход в `finishing` остаётся только по `Idle{background:false}` — машина состояний не меняется. Не смог адаптер
  остановить (ошибка RPC, субагент) — прогон ждёт бюджет фона, как раньше: это запасной путь, а не новый сбой.
- Исхода нет → эффект не ставится, фон ждётся как раньше.
- Фоновый **субагент** claude не останавливается (критерий 4а): адаптер стопит только задачи `local_bash`. У claude
  при живом фоновом агенте состояние остаётся `running` (опыт 1b), так что `Idle{true}` в сценарии 4а и не приходит;
  фильтр по типу — страховка.
- Исход заявлен уже после конца хода (фоновой командой или субагентом), пока сессия «idle с фоном» → тот же эффект
  из `finish()`.

Допущения записаны в журнал (2026-11-16T15:45, expert). Событий журнала, полей API и OpenAPI это не меняет.

### Рестарт демона (§5.6)

- Окно «эффект поставлен — `Idle{false}` не пришёл» длится миллисекунды. Прогон в нём `running` без `waiting`:
  восстановление завершает попытку `daemon_crash`, как любую идущую (`recover.rs`, не меняется). Экземпляр харнесса
  убит со всеми потомками, оставленная команда — тоже.
- Незавершённый эффект `stop_background` переживает рестарт (ключ эффекта — id прогона, `recover.rs` его не
  удаляет) и исполняется на мёртвом handle: адаптер не знает экземпляра и молча выходит. Поэтому метод возвращает
  `()`, а не `Result`: его сбой не должен ронять попытку.
- `finishing` и далее — без изменений (исход хранится, хуки запускаются заново).

## 2. Изменения по файлам

### 2.1 Ядро (crate `aae-core`), ~35 строк кода

`crates/aae-core/src/driver.rs` (309 строк) — новый обязательный метод трейта `Driver`, после `respond`:

```rust
/// Stops the commands the session left running after its turn (spec §9.4); the session stays and takes a next
/// prompt. Work the agent may still wait for (a background subagent) is left alone. The harness reports the end
/// of what was stopped, and the session comes to `State{Idle, background: false}` by the usual rule. Best effort.
fn stop_background<'a>(&'a self, instance: &'a InstanceHandle, session: &'a SessionRef) -> BoxFuture<'a, ()>;
```

Без реализации по умолчанию: реализаций три (claude, codex, `ScriptedDriver`), молчаливая дыра хуже.

`crates/aae-core/src/run/finish.rs` (239) — `pub(super) fn stop_background(tx: &mut Tx<'_>, row: &Row) -> Result<(), Error>`:
один запрос `SELECT harness, handle, session FROM attempts WHERE run = ?1 AND n = ?2 AND turn_ended != 0 AND
outcome IS NOT NULL AND handle IS NOT NULL AND session IS NOT NULL` (`.optional()`); нет строки — `Ok(())`; иначе
`tx.effect(&row.id, &format!("stop_background:{}", row.attempt), &json!({ "op": "stop_background", "harness": …,
"handle": …, "session": … }))` (handle и session разбираются из текста в `Value`, как в `next_prompt`). Повторный
эффект с тем же dedupe поглощается, пока первый не исполнен; повтор после исполнения безвреден.
В `finish()` после `UPDATE attempts SET outcome` — вызов `Self::stop_background(tx, &row)?` (условие `turn_ended != 0`
внутри запроса: посреди хода ничего не ставится).

`crates/aae-core/src/run/events.rs` (**381 строка, лимит 400**) — только ветка `State{Idle, background:true}`:
перед `silence(...)` вызов `Self::stop_background(tx, &row)?` (+3 строки). Больше в этот файл ничего не добавлять.

`crates/aae-core/src/run/exec.rs` (324) — в `match (payload["op"].as_str(), handle)` ветка
`(Some("stop_background"), Ok(handle)) => { driver.stop_background(&handle, &session).await; Ok(()) }`.

### 2.2 Сценарный драйвер (`aae-testkit`), ~30 строк

- `src/lib.rs` (334): в `State` поля `background_stops: usize` и `stoppable: bool`; `impl Driver`:
  `stop_background` увеличивает счётчик и, если `stoppable` и `live_session` жив, шлёт
  `self.emit(&instance.id, Some(session), Normalized::State { state: Idle, background: false })`.
- `src/control.rs` (137): `pub fn stoppable_background(&self)` (ставит флаг: «фон сессии — команды, которые харнесс
  умеет остановить») и `pub fn background_stops(&self) -> usize`.
- По умолчанию флаг снят: драйвер ведёт себя как сессия с субагентом. Существующий тест
  `the_last_outcome_declared_during_the_turn_passes_after_idle_without_background` (finisher.rs) остаётся как есть
  и теперь означает «фон, который остановить нельзя, ждётся» (ядро 4а).

### 2.3 claude (`aae-claude`), ~25 строк

`src/stream.rs` (290):
- рядом с `background: bool` хранить `commands: Vec<String>` — `task_id` задач с `task_type == "local_bash"` из
  последнего `background_tasks_changed` (`tasks[]` — полный текущий набор, элемент:
  `{"task_id","task_type":"local_bash"|"local_agent",…}`); `pub(crate) fn commands(&self) -> Vec<String>`;
- `task_notification`: `notified_since_result` ставится только при `msg["status"] != "stopped"`. Причина (опыт 8):
  после `stop_task` приходят `task_notification{status:"stopped"}`, `background_tasks_changed{tasks:[]}`,
  `control_response`, `session_state_changed{idle}`, а `result` не приходит — иначе сессия навсегда «idle с фоном».
  Остановленная задача модель не будит.

`src/lib.rs` (**356**): `stop_background` — взять `commands()` из потока экземпляра и на каждый id записать строку
`{"type":"control_request","request_id":"<uuid v4>","request":{"subtype":"stop_task","task_id":"<id>"}}`
(через `self.write`, ошибки игнорировать; экземпляр неизвестен — выйти). Не `interrupt`, не `end_session`, не сигналы:
убийство процесса задачи снаружи будит модель лишним ходом.

Запасной ход, разрешён заранее: если на живом claude после `stop_task` завершающий `session_state_changed{idle}` не
приходит (в опыте приходил, n=2), поток сам отдаёт `State{Idle, background:false}` на `background_tasks_changed` с
пустым списком, когда последнее состояние `idle`, ход закрыт и `notified_since_result == false`. Без нужды не делать.

### 2.4 codex (`aae-codex`), ~10 строк

`src/lib.rs` (**366**): `stop_background` — `known.call("thread/backgroundTerminals/clean",
json!({ "threadId": thread_id(session) }), None)`, результат игнорируется; экземпляр неизвестен — выйти. Больше ничего:
на `clean` app-server шлёт `item/completed` команды, а `src/stream.rs` (строка «The last background command of an ended
turn is done») уже превращает его в `State{Idle, background:false}`. Не `thread/unsubscribe` (закрывает тред и не даёт
`item/completed`), не `turn/interrupt` (ошибка −32600 на законченном ходе). API экспериментальное, адаптер уже
инициализируется с `experimentalApi: true`.

### 2.5 Хвост (режется первым): отвязавшиеся процессы у codex, ~25 строк

Команда вида `setsid … >/dev/null 2>&1 &` фоном харнесса не считается и прогон не держит, но живёт, пока жив общий
app-server (у claude процесс — это сессия, его остатки уходят с экземпляром по `AAE_INSTANCE`). Чтобы «сирот после
прогона нет» выполнялось и на общем app-server:

- `crates/aae-core/src/orphans.rs` (**347**): обобщить `marker(pid)` до чтения переменной по имени и добавить
  `pub fn sweep_var(name: &str, value: &str) -> usize` (убить каждый процесс, кроме своего, у которого в окружении
  `name=value`); тест рядом по образцу `the_sweep_kills_every_descendant_with_the_marker` с приманкой другого
  значения. Файл после этого ≤ 395 строк; не влезает — сначала вынести `mod tests` в `orphans/tests.rs`.
- `crates/aae-codex/src/rpc.rs`: в `Instance` — `tokens: Mutex<HashMap<String, String>>` (тред → токен попытки);
  заполнять в `start_session`/`resume_session` из `part.0["config"]["shell_environment_policy"]["set"]
  ["AAE_ATTEMPT_TOKEN"]`; в `close_session` после `thread/unsubscribe` — снять токен треда и
  `spawn_blocking(|| orphans::sweep_var("AAE_ATTEMPT_TOKEN", &token))`. app-server токена не несёт, чужие треды — с
  другими токенами.

Не успеваешь — не начинай: напиши в ответе «хвост 2.5 не сделан», coder запишет предел в журнал.

## 3. Тесты

### 3.1 Ядро на сценарном драйвере — новый файл `crates/aae-testkit/tests/core/background.rs` (~120 строк)

Подключить строкой `mod background;` в `crates/aae-testkit/tests/core/main.rs` (единственная правка этого файла; coder
может добавлять туда свои `mod` — перечитай файл перед правкой). В `finisher.rs` сделать `World::finish`
`pub(crate)` (одно слово) и звать его. Часы приостановлены (`start_paused = true`), образец — `finisher.rs:48`.

1. `a_declared_outcome_stops_the_commands_left_in_the_background` — драйвер `stoppable_background()`; сценарий
   `TurnStarted, Wait(10 s), TurnEnded{Completed}, State{Idle, background:true}` и дальше тишина; финишер `pass` на
   1-й секунде; через ~11 с состояния кончаются на `["finishing", "passed"]`, `background_stops() == 1`.
2. `without_an_outcome_the_background_is_waited_for_and_a_late_outcome_stops_it` — тот же сценарий без финишера:
   через 60 с прогон `running`, `background_stops() == 0`; затем финишер `pass` → `accepted`, счётчик 1, прогон
   `passed` (ветка из `finish()`).
3. `idle_with_background_inside_a_turn_stops_nothing` — `TurnStarted, Wait(10), State{Idle,true}, Wait(10),
   TurnEnded, State{Idle,false}`; финишер на 1-й секунде; на 15-й `background_stops() == 0`; в конце `passed`.
4. `a_restart_while_the_background_stops_fails_the_attempt_with_daemon_crash` — мир с `POLICY`, драйвер **без**
   `stoppable_background()`; сценарий 1, финишер; после `Idle{true}` — `w.restart().await`; последний
   `run.attempt_failed` — `daemon_crash`, состояние `interrupted`, ничего не падает (образец — `recovery.rs:21`).
   Режется вторым после хвоста 2.5.

### 3.2 claude

- `src/stream/tests.rs` (298), рядом с `idle_counts_only_without_background_work_and_late_notifications`:
  `a_stopped_task_is_no_wake_up_and_only_commands_are_stopped` — список из `local_bash` и `local_agent` →
  `commands()` отдаёт только id первой; после `result`, `task_notification{status:"stopped"}`, пустого списка и
  `idle` последнее событие — `idle(false)`; то же с `status:"completed"` — `idle(true)` (уже проверено соседним тестом).
- `tests/claude.rs` (184; **не** `contract.rs` — он 399 строк, и его 15 сценариев посчитаны в документах и в строке
  приёмки): `a_command_left_in_the_background_is_stopped_and_the_session_takes_the_next_prompt` — план
  `[Reply::Background("sh -c 'echo $$ > bg.pid; exec sleep 300'"), Reply::Text("Started."), Reply::Text("Again.")]`,
  `permission_mode = bypassPermissions` (как в `contract.rs` s14); после `TurnEnded` и `State{Idle,true}` —
  `driver.stop_background(...)`; ждать `State{Idle,false}` (до 30 с); процесс из `bg.pid` исчез
  (`aae_core::orphans::start_time(pid) == None`, опрос до 5 с); 2 с нет `TurnStarted`; второй промпт завершается
  `Completed`; `provider.requests().len() == 3` (остановка не стоила запроса к модели).

### 3.3 codex — `tests/codex.rs` (212; не `contract.rs`, причина та же)

`the_commands_a_thread_left_are_stopped_and_other_threads_keep_theirs` — один app-server, два треда (образец двух
сессий — `a_thread_resumed_on_its_live_app_server_gets_the_new_attempt_token` там же), план
`Reply::Turn(vec![Reply::Command("sh -c 'echo $$ > bg.pid; exec sleep 300'"), Reply::Text("Started.")])`, у тредов
разные рабочие каталоги; оба хода кончились с `State{Idle,true}` (команда уступает ход примерно через 10 с);
`stop_background` на треде A → его `State{Idle,false}`, pid A исчез, pid B жив; второй промпт в A завершается без
resume. Если два треда в этом файле обходятся дороже ~60 строк — один тред здесь, а изоляцию оставь строке приёмки
(3.4 её проверяет на общем app-server).
С хвостом 2.5: `a_detached_command_goes_with_its_thread` — команда
`setsid sh -c 'echo $$ > det.pid; exec sleep 300' >/dev/null 2>&1 &`; после `close_session` pid исчез.

### 3.4 Приёмка — новый файл `crates/aae-acceptance/src/scenarios/c34_background.rs` (~120 строк)

Без модели: настоящие `claude` и `codex` на поддельных провайдерах testkit, образец — `c04_finisher.rs`
(`claude_agent`, `codex_agent`, `Provider`, `Daemon::start`, `until_state`, `events_of`). Одна функция на харнесс,
обе через `tokio::join!`. На харнесс — один демон, проект `shop` с двумя ригами (`config/aae/shop/aae.toml`:
`[rigs.web]`, `[rigs.api]`; в каталоге `web` заранее лежит файл `declare`), один агент, один провайдер с планом на
любое число сессий:

```
Reply::Turn(vec![
    <команда>("[ -e declare ] && {aae} finish pass --summary served; true"),
    <фоновая команда>("sh -c 'echo $$ > bg.pid; exec sleep 20'; touch bg-done"),
    Reply::Text("Started."),
])
```

claude: `Reply::Bash` и `Reply::Background`; codex: `Reply::Command` дважды (вторая уступает ход сама). Два прогона
сразу: A на `web` (исход заявлен), B на `api` (не заявлен; создать через `d.post("/v1/runs", …"rig":"api"…)`).

- A: `passed` в пределах 60 с; в момент `passed` файла `web/bg-done` нет; процесс из `web/bg.pid` исчезает за 10 с;
  в событиях есть `run.finishing`.
- B: в событиях `run.turn_ended_without_outcome`, конец — `run.failed`; к этому моменту `api/bg-done` **есть** (фон
  дождались, команду B остановка A не задела — у codex оба треда на одном app-server).
- После B: `web/bg-done` так и не появился.
- С хвостом 2.5 — третья команда плана, отвязанная (`setsid … det.pid …`), и проверка, что оба `det.pid` исчезли
  за 15 с после конца своего прогона.

Регистрация: `mod c34_background;` и элемент `Scenario { criterion: "34", title: "an agent that declared its outcome
and left a background command: the step ends at once and the command is stopped; without an outcome the background is
waited for (claude and codex on fake providers)", launch: Launch::Ci, run: |env| Box::pin(c34_background::run(env)) }`
в `scenarios/mod.rs` (**373 строки**, +7). Запись `"34"` из `OPEN` в `readiness.rs`, `docs/READINESS.md` и строку в
`checks.rs` **не трогать** — это делает coder в конце цикла вместе с критерием 30.

Прогон строки (отчёт — в свой каталог, общий `target/acceptance/report.md` не затирать):
`cargo build -q --locked -p aae && R=$(mktemp -d) && timeout 600 cargo run -q --locked -p aae-acceptance -- --ci
--report $R/report.md 34; grep -n '| 34 ' $R/report.md`. Смотри отметку строки 34, а не код выхода (строка 22 о
готовности может быть красной из-за `OPEN`).

### 3.5 Мутации, которые тесты обязаны убивать (проверь руками 1, 2, 4, 6; остальные — по смыслу тестов)

| # | Мутация | Кто убивает |
|---|---|---|
| 1 | в `events.rs` убран вызов `stop_background` | 3.1 №1, 3.4 A |
| 2 | из запроса убрано `outcome IS NOT NULL` | 3.1 №2, 3.4 B (`bg-done` нет) |
| 3 | из запроса убрано `turn_ended != 0` | 3.1 №3 |
| 4 | claude: `stopped` снова ставит `notified_since_result` | 3.2 оба, 3.4 A claude |
| 5 | claude: стопятся и `local_agent` | 3.2 юнит |
| 6 | codex: `thread/unsubscribe` вместо `clean` | 3.3 (нет `Idle{false}`, второй промпт без resume падает) |
| 7 | codex: `clean` без `threadId` треда / на все треды | 3.3 или 3.4 B codex |
| 8 | `exec.rs`: ветка есть, вызова драйвера нет | 3.1 №1 |
| 9 | убран вызов из `finish()` | 3.1 №2 |
| 10 | claude: сигнал процессу задачи вместо `stop_task` | 3.2 интеграционный (`requests().len()`, `TurnStarted`) |

## 4. Документы (английский), ~40 строк

- `docs/design/contracts.md`:
  - 4.1 — метод `stop_background` в листинге трейта;
  - 4.3, строка `state` — при `idle` с `background` после конца хода и заявленном исходе ядро зовёт
    `stop_background`;
  - 4.6 — строка таблицы «Stop of what a session left»: claude — `stop_task` на каждую задачу `local_bash`,
    `task_notification{stopped}` не считается пробуждением; codex — `thread/backgroundTerminals/clean{threadId}`;
  - 6.2 — строка перехода: `running` | turn ended, idle with background, outcome declared | `running` | the adapter
    stops the commands the session left; `finishing` follows on idle without background;
  - 6.3 — строка «turn ended, background work (subagents) still running» делится на две: «outcome declared, commands
    left running → the daemon stops them, then `finishing`» и «background the daemon does not stop (a subagent), or no
    outcome → stays `running` until idle without background, deadline — background silence budget»; плюс «finisher
    called after the turn ended, while background runs → stored, and the commands are stopped»;
  - 6.4 — одно предложение о рестарте в окне остановки (раздел 1 выше).
- `docs/design/adapters.md`, «What the core expects of a driver» — пункт про `stop_background`: что останавливать, что
  нет, что сессия должна остаться и сама прийти к `State{Idle, background:false}`.
- `docs/design/experiments.md`, раздел опыта 8: в конце «Recommended mechanism» — абзац «What the product does»:
  шаг 1 как рекомендовано; фоновый субагент не останавливается; правило (c) опыта 1b — с оговоркой про `stopped`
  (поправить и формулировку в разделе «Idle without background»); шаг 2 — по токену при закрытии треда codex (или
  «not done» с причиной, если хвост 2.5 срезан). Новых замеров нет — маркер `MEASURE` не нужен.
- Спеку (`docs/init/AAE-SPEC.md`), `docs/READINESS.md`, `harness-updates.md` не трогать.

## 5. Порядок работы и группы для коммитов (коммитит coder)

1. **Ядро + testkit** (2.1, 2.2, 3.1): `driver.rs`, `run/finish.rs`, `run/events.rs`, `run/exec.rs`,
   `aae-testkit/src/{lib,control}.rs`, `tests/core/{background,main,finisher}.rs`. Чтобы дерево собиралось, в этом же
   шаге добавь в оба адаптера `stop_background` (сразу настоящие, из 2.3 и 2.4 — они по 10 строк).
   Коммит: `WP-83: a declared outcome stops the commands a session left in the background`.
2. **claude** (2.3, 3.2): `aae-claude/src/{lib,stream}.rs`, `src/stream/tests.rs`, `tests/claude.rs`.
   Коммит: `WP-83: claude stops the shell tasks a session left`.
3. **codex** (2.4, 3.3): `aae-codex/src/lib.rs`, `tests/codex.rs`. Коммит: `WP-83: codex cleans the background
   terminals of a thread`.
4. **Приёмка и документы** (3.4, 4): `scenarios/{c34_background,mod}.rs`, три файла `docs/design/`.
   Коммит: `WP-83: acceptance row 34 and the contracts of the background stop`.
5. **Хвост** (2.5): `aae-core/src/orphans.rs`, `aae-codex/src/{rpc,lib}.rs`, тесты.
   Коммит: `WP-83: a closed codex thread takes its detached commands with it`.

Шаги 1–4 обязательны. К началу шага 5 контекст больше ~170k — шаг 5 не начинать.

Проверки своего куска (вывод всегда обрезан; каждая команда — с `timeout`):

```
cargo build -q --message-format short 2>&1 | head -60
cargo clippy --all-targets -q --message-format short -- -D warnings 2>&1 | head -60
cargo nextest run -p aae-testkit -E 'test(/background|finisher|watchdog|recovery/)' --no-fail-fast --status-level fail --final-status-level fail 2>&1 | tail -40
cargo nextest run -p aae-claude -p aae-codex --no-fail-fast --status-level fail --final-status-level fail 2>&1 | tail -40
scripts/budget.sh 2>&1 | tail -12
```

Общее дерево: ошибка компиляции в файле из списка WP-81 — чужая, не чинить; подожди 2 минуты и повтори, до трёх раз,
затем проверяй то, что собирается, и напиши об этом в ответе. `cargo fmt` — только по своим файлам
(`rustfmt --edition 2024 <файлы>`), не по всему дереву.

## 6. Размер, лимит 400 строк, бюджеты

Код продукта ≈ +100 строк с хвостом (ядро 35, claude 25, codex 10 + 15, orphans 10); тесты ≈ 400; документы ≈ 40.
Бюджеты §14 далеко (ядро 10 746 / 18 000, claude 877 / 3 500, codex 1 154 / 3 500).

Файлы у лимита — что можно добавить:

| Файл | Сейчас | После |
|---|---|---|
| `aae-core/src/run/events.rs` | 381 | 384 — только три строки |
| `aae-codex/src/lib.rs` | 366 | ~376, с хвостом ~386 |
| `aae-claude/src/lib.rs` | 356 | ~372 |
| `aae-core/src/orphans.rs` (хвост) | 347 | ≤ 395, иначе вынести тесты |
| `aae-acceptance/src/scenarios/mod.rs` | 373 | 381 |
| `aae-testkit/src/lib.rs` | 334 | ~352 |
| `aae-claude/tests/contract.rs` (399), `aae-codex/src/tests.rs` (371), `aae-acceptance/src/main.rs` (390), `tests/common/mod.rs` testkit (368) | — | не трогать |

## 7. Пересечения с WP-81

Общих файлов кода нет. Возможные касания, все однострочные:
`crates/aae-testkit/tests/core/main.rs` (строка `mod background;`), `crates/aae-acceptance/src/scenarios/mod.rs`
(строка `mod` и элемент `ALL` в конце массива — coder может добавлять рядом строку 30). `readiness.rs`, `checks.rs`,
`READINESS.md` — за coder. `machine.rs` и `recover.rs` план не трогает.

## 8. Чего здесь нет

- События журнала «фон остановлен» и поля в API — критерий не требует.
- Остановки фоновых субагентов — противоречит критерию 4а.
- Перехода в `finishing` на восстановлении для окна остановки.
- Запасного `thread/unsubscribe` у codex: пропадёт `clean` — упадёт тест 3.3 при обновлении харнесса.
- Живых моделей: всё на поддельных провайдерах. Строка 4а («запрос») после этого пакета должна пройти живьём один раз
  — это в общем живом прогоне coder на замороженном коде, не здесь.
