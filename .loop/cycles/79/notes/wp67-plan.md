# WP-67 «Корневой бид и `drain`» — план реализации (expert, цикл 79)

Основание: спека §9.1, §9.3, §10.1, §10.3, §5.5–5.6; решения 52, 76, 81–83; критерий 24. Код не собирался: строки —
по чтению. Проверено опытом на br 0.7.6 во временном трекере (п. 8): четыре факта, на которых стоит план.

## 0. Суть

`drain` — не второй диспетчер, а **третий вид попытки** рядом с `exec`: попытка без процесса (`attempts.harness =
'drain'`), прогон стоит в `running` и на каждое чтение трекера (`Msg::Ready`) делает один проход: для каждого готового
бида под корневым вызывает **обычный `create()`** — тот же, что у `aae run start` и триггеров. Выбор бида, владение,
claim, счётчик неудач, отмена детей — существующие, без правок по смыслу. Метки `agent:`/`harness:` разбирает
`create()` для **любого** прогона на биде (как уже сделано для `rig:`), а не `drain`. Конец шага — общий хвост
`finishing → advance()` из WP-66. Корневой бид создаётся эффектом трекера, пока прогон в `preparing`, сразу перед
claim.

## 1. `drain` на машине шагов

**Конфиг.** `[[step]] kind = "drain"`, `formula = "work"` (ссылка, как `run` у триггера), `timeout` необязателен.
`StepKind::Drain { formula: Name, timeout: Option<Duration> }` (`config/mod.rs:205`), `StepFile::Drain { name, formula,
timeout }` (`files.rs:357`; файл 395 строк — если не влезет в 400, перенести `StepFile` в `items.rs`). Проверки
загрузки в `check.rs:30` (рядом с разрешением агента шага, образец — триггер `:40-62`): ссылка разрешается в формулу;
у формулы-исполнителя ровно один вход типа `bead` и нет других обязательных; у формулы со шагом `drain` есть
обязательный вход типа `bead`.

**Миграция 19** (`store.rs`), одна:
`ALTER TABLE runs ADD COLUMN step_kind TEXT NOT NULL DEFAULT 'prompt'; UPDATE runs SET step_kind = 'exec' WHERE agent = '';
ALTER TABLE runs ADD COLUMN as_agent TEXT;`
`step_kind` пишут `create()` (`queue.rs:139`) и `advance()` (`finish.rs:116`) вместе с `step, agent`; `Row` получает
`step_kind`, `as_agent`. Зачем колонка, а не снимок `runs.config`, как намечал план WP-66: снимок пуст в `queued` и
на одно событие отстаёт в `advance()`, а вид нужен в SQL (ниже). `Row::exec()` остаётся «агента нет» (верно и для
drain: `gates.rs`, запрет `remark` в `decide.rs`); точный вид читают `view()` (`step_kind: self.step_kind`),
`start_attempt` (`machine.rs:103`: `match row.step_kind` → `start_exec | start_drain | дальше`), `end_attempt`
(`machine.rs:313`: `stop_exec` только при `harness == "exec"`), `render()` (`render.rs:26`: ветка `Drain` →
`(None, timeout или DRAIN_TIMEOUT_MS = 24 ч, json!({"drain": "<формула>"}))`; «changed its kind» — по `row.step_kind`).

Один SQL-фрагмент на четыре места — `const DRAINING: &str = "(step_kind = 'drain' AND state = 'running')"`:
- `recover.rs:44`: `AND NOT DRAINING` — у drain нет процесса, попытка **не** получает `daemon_crash`, прогон стоит;
- `queue.rs:230` (`blocker`, общий счётчик `all`): `AND NOT DRAINING` — иначе при `max_runs = 1` дети не стартуют;
- `queue.rs:206` (`workdir_busy`): `AND NOT (id = ?3 AND DRAINING)`, `?3 = row.parent_run` — родитель в drain не
  держит каталог **от своих детей** (иначе бид с `rig:` рига родителя ждёт вечно); чужие прогоны по-прежнему ждут;
- выборка прогонов для прохода: `SELECT id FROM runs WHERE project = ?1 AND DRAINING ORDER BY n`.

**Новый файл `run/drain.rs`** (~140 строк):

```
start_drain(tx, row):   нет row.bead или трекера -> terminal(failed, infrastructure, "a drain step needs the bead of the run")
                        attempt += 1; INSERT attempts (kind 'new_session', harness 'drain', step); state = running
                        tx.effect(tracker_key, "sync:drain:<run>:<attempt>", sync(..))   // без немедленного прохода
                        set_deadline(state, now + timeout_ms)
drained(tx, project):   из обработчика Msg::Ready (mod.rs:171) после Self::ready(): для каждого прогона DRAINING -> tick
tick(tx, row):
  kids  = SELECT id, bead, state, created_ms FROM runs WHERE parent_run = ?1 AND bead IS NOT NULL
  mine  = kids с created_ms >= attempts.started_ms текущей попытки          // выданные этим заходом в шаг
  for (id, _) in ready_in(tx, project, Some(root), None), кроме бидов из mine:
      match tx.unit(|tx| self.create(tx, Create { project, rig: None, formula: <из снимка config.drain>,
                 inputs: {<bead-вход исполнителя>: id}, correlation: Some(row.correlation), depth: row.depth + 1,
                 parent_run: Some(row.id), token: None, continue_from: None })):
          Ok(_) -> dispatched += 1;  Err(Rejected(why)) -> refused[id] = why;  Err(e) -> return Err(e)
  кончить, если: dispatched == 0 && среди kids нет живых && трекер прочитан свежо:
      SELECT COUNT(*) FROM effects WHERE key = 'tracker:<p>' AND dedupe LIKE 'sync:%'   <= 1
  конец: outcome = { result: pass, summary: "2 closed, 1 failed, 1 refused",
                     data: { closed: [..], failed: [..], refused: { id: why } } }  по mine
         -> общий хвост с exec_done (вынести из exec_step.rs:130-142 в step_done(tx, row, outcome))
```

- `ready_in()` — это цикл из `pick_bead` (`beads.rs:43-58`), возвращающий все подходящие (`rig: Option<&str>`);
  `pick_bead` берёт из него первый. Подбор один.
- `depth + 1 > MAX_DEPTH` — отказ всем кандидатам с причиной (проверка в `tick`, 3 строки).
- **Кто будит.** Только `Msg::Ready`: чтение трекера идёт после старта шага, после каждой записи br (наблюдатель →
  `TrackerChanged`), после close/release каждого прогона, раз в 5 минут (`RESYNC`) и на старте демона. Чтобы конец
  ребёнка будил всегда, в `let_go_bead` (`beads.rs:221-229`) эффект `sync` ставится и в ветке раннего выхода (прогон
  не стартовал, claim отвергнут) — сейчас там чтения нет.
- **Почему «свежо».** Ребёнок закрыл A, B был заблокирован A: в очереди `[close:A, sync]`. Чтение, начатое раньше,
  придёт со старым набором; за ним в очереди стоит ещё один `sync` → решение о конце откладывается до него. Эффект
  в полёте остаётся в таблице, пока его сообщение обрабатывается (`effects.rs:52`), отсюда `<= 1`.
- Бид выдаётся **один раз за попытку шага**: провалившийся остаётся «несделанным для следующего шага» (§9.1), отменённый
  оператором не перезапускается (инвариант 18), отвергнутый claim не крутит цикл. Счётчик неудач бида (`bead_failures`)
  работает между заходами (возвраты WP-68) и закрывает ворота как сейчас.
- Отмена родителя отменяет детей — уже есть (`events.rs:32`). Таймаут шага → `runaway_turn` → политика (новая
  попытка шага продолжит: живые дети считаются по всем `kids`).

## 2. Корневой бид

**Вход.** Значение входа типа `bead` получает третью форму: `{ request = "<текст>" }` (рядом с id и
`{ next_ready, epic }`). CLI уже принимает JSON: `--input 'bead={"request":"…"}'`. Риг при этом обязателен.
В `create()` (`queue.rs:96`): форма `request` (только этот ключ, непустая строка) → `bead = None`, вход остаётся как есть.

**Момент создания — `prepare()`** (`machine.rs:32`), как claim: `(None, Some(request))` → `create_root()`:
эффект `tracker:<project>` / `create:<run>` с `{op: tracker, call: create, run, key, title, description}` и дедлайн
`start_timeout` (существующая ветка `("state","preparing") → tracker_unavailable`). `key = "aae-" + 16 hex` из
`new_token()` — лежит в payload эффекта, поэтому повтор после `kill -9` идёт с тем же ключом. `title` — первая
непустая строка, ≤ 80 символов; `description` — текст целиком.

**Ответ** (`tracked()`, новая ветка перед общей ветвью ошибок, тело — `created()` в `drain.rs`):
`("create", Ok(bead))`, если прогон в `preparing` и `row.bead` пуст → `own_bead()`; `UPDATE runs SET bead, inputs`
(во вход подставлен id); `self.claim()` — дальше существующий путь до `render_and_prepare` (`{{ bead.description }}` —
текст запроса). Иначе ответ отбрасывается. `("create", Err(не Unavailable))` → `failed{infrastructure,
"bead_not_created: …"}`; `Unavailable` — общая ветка с повтором.

**Интерфейс** (`tracker.rs`, шестая операция):
`fn create<'a>(&'a self, actor: &'a str, key: &'a str, title: &'a str, description: &'a str) -> BoxFuture<'a, Result<Bead, TrackerError>>;`
«Один раз на `key`: повтор возвращает бид первого вызова».

**br** (`aae-br/src/lib.rs`): сначала `br list -a --limit 0` и поиск `external_ref == key` (обе формы JSON); нет —
`br create <title> -t epic -d <description> --external-ref <key> --silent`, затем `show`. Тип `epic`: открытый эпик с
открытыми детьми br в `ready` не отдаёт. Меток у корня нет (эпик шире рига).
**`close`**: br отказывается закрыть родителя с открытыми детьми кодом 3 (сейчас это читается как `NotFound`, бид
остаётся `in_progress` за мёртвым прогоном). Правка: `Err(Fail(Some(3), _)) => { self.one(id).await?; self.release(owner, id).await }`.
**MemoryTracker** (`aae-testkit/src/tracker.rs`): `create` с `keys: Vec<(String, String)>` в `State`.

## 3. Метки — в `create()`, для любого прогона на биде

`queue.rs`, после выбора бида (вычисление агента первого шага переезжает ниже `pick_bead`): метки берутся из
`ready_beads.body` выбранного бида (`rigs()` в `beads.rs:355` обобщить до `labels(bead, prefix)`).

- `rig:` — уже есть: `rig: None` → `rig_of_bead()` («no `rig:` label», «several rigs»), `other_rig`.
- `agent:<имя>` → `as_agent` (колонка; несколько меток — отказ). `Config::for_run_as(project, rig, formula, step,
  as_agent: Option<&str>)` в `compose.rs:47`; `for_run` — обёртка с `None`, чужие вызовы не трогаем. Имя — вариант
  (`p.variants`/`self.variants`) → путь варианта; иначе агент через `resolve(.., reference(name), has_agent, "agent")`
  и дальше как сейчас (вариант рига). `as_agent` передают три вызова о прогоне: `step_agent` (`queue.rs:24`, из
  `create` и `advance`), `render.rs:38`, `reload.rs:109`. Метки нет — агент шага формулы.
- `harness:<имя>` → для каждого `prompt`-шага формулы `for_run_as(..)?.1.harness` сравнивается с меткой; расхождение →
  `rejected("bead `b` is labelled `harness:codex`: the config gives `claude` to `dev` on `web`")`. Метки нет —
  проверки нет. У формулы без `prompt`-шагов метки `agent:`/`harness:` ни к чему не применяются.

Итог отказа: у `aae run start` — ответ `rejected`; у `drain` — прогон не создаётся, бид остаётся в трекере открытым,
причина в `data.refused` итога шага (журнал: `run.finishing`/`run.step_finished`; следующий шаг: `AAE_STEP_<NAME>_DATA`).
Это отказ по биду, шаг и остальные биды идут дальше. Ворота не закрываются: исправленную метку возьмёт следующий проход.

## 4. Восстановление (`kill -9` посреди drain)

| Где убит | Состояние | После старта |
|---|---|---|
| эффект `create` корня не исполнен / исполнен, ответ не записан | прогон `preparing`, эффект в таблице | эффект идёт снова с тем же `key` → адаптер находит бид; дубля нет |
| ответ `create` записан, эффект не удалён | `runs.bead` стоит | повторный ответ отброшен (`row.bead` не пуст) |
| drain `running`, дети живы/в очереди | попытка `drain` жива, `beads.owner` у детей | попытка drain не трогается (`NOT DRAINING`); дети — обычный recover; `sync:start` → `Ready` → `tick` |
| между `create()` ребёнка и его claim | владение записано в той же транзакции (инвариант 3) | `claim:<run>` в эффектах, идёт снова; `pick` пропускает бид с владельцем |
| drain `finishing` под хуком | исход записан, `waiting` | как у любого шага (WP-66) |

Почему ни один бид не выдан дважды: выбор и `own_bead` — одна транзакция (инварианты 2, 3); `ready_in` пропускает
биды с владельцем; закрытый бид удалён из `ready_beads` (`beads.rs:232`); выданный в этой попытке шага отсеян по
`mine`; попытка drain переживает рестарт, значит `mine` не обнуляется. Инвариант 14 — дедлайн `state` шага;
инвариант 20 — проход только по чтению трекера.

**Хаос-стенд** (`tests/chaos`; часы на паузе, exec нет): формула `sweep` = `drain` (исполнитель `fix`) → `prompt`;
в трекере корень и два ребёнка с `rig:web`; `create()` (`steps.rs:42`) выбирает и её. Проверки в `check.rs`:
D1 — у пары (родитель, бид) не больше одного ребёнка на попытку drain; D2 — у прогона с `run.step_finished` шага drain
нет живых детей с бидом; D3 — нет попытки `harness = 'drain'` с `failure.kind = 'daemon_crash'`. `undone()` получает
категорию `drain` (жив прогон `DRAINING`), `KINDS` (`main.rs:230`) — пятый элемент: каждая доля сидов убивает посреди drain.

## 5. Коммиты

| № | Коммит | Файлы | Тесты и мутации (должны краснеть) |
|---|---|---|---|
| 1 | The tracker creates a bead once per key; a parent with open children is released, not closed | `tracker.rs`, `aae-br/src/lib.rs`, `aae-testkit/src/tracker.rs`, `beads.rs:306` (`execute`) | `aae-br/tests/contract.rs`: два `create` с одним ключом → один бид типа epic; дети захваченного корня готовы; close родителя с открытым ребёнком → `open`, без исполнителя. Мутации: убрать поиск по `external_ref`; убрать ветку кода 3 |
| 2 | A request becomes the root bead of its run | `queue.rs`, `machine.rs:32`, `beads.rs` (ветка `create`), `drain.rs` (`create_root`, `created`) | `tests/drain.rs`: запрос → один бид, заголовок/описание, `runs.bead`, claim, промпт видит описание; трекер лежит → рестарт → поднялся: бид один. Мутации: новый ключ на каждый вызов эффекта; снять условие `row.bead` пуст |
| 3 | A bead names the agent and shows the harness of its run | миграция 19, `row.rs`, `compose.rs`, `queue.rs`, `render.rs`, `reload.rs`, `finish.rs:104` | `tests/beads.rs`: `agent:` вариант и агент; `harness:` расходится → `rejected` с обоими именами; без меток — как раньше; формула из двух prompt-шагов держит агента из бида на втором. Мутации: `as_agent` не передан в `advance`; сравнение харнесса `!=` → `==` |
| 4 | The drain step gives every ready bead under the root a run | `config/{mod,files,items,check}.rs`, `drain.rs`, `machine.rs`, `exec_step.rs`, `recover.rs`, `queue.rs`, `beads.rs`, `mod.rs:171`, `aae-front/src/api/config.rs:200`, `schema.rs`, OpenAPI | `tests/drain.rs` (п. 7). Мутации: убрать условие «свежо»; убрать отсев `mine`; убрать `NOT DRAINING` из recover / из `workdir_busy` / из `blocker`; провал ребёнка → провал шага |
| 5 | The chaos stand and criterion 24 kill the daemon mid-drain; docs | `tests/chaos/*`, `aae-acceptance/src/scenarios/c24_drain.rs`, `mod.rs`, доки (п. 9), `.loop/PLAN.md`, READINESS | D1–D3; сценарий п. 6 |

Ядро: ≈ +250 строк (drain.rs 140, config 45, queue 30, остальное 35). `scripts/loop/checks 79` — один раз в конце.

## 6. Критерий 24 в `acceptance-ci`

`c24_drain.rs`, `Launch::Ci`, настоящие демон и br, без модели: exec-шаги и настоящий `claude` на поддельном
провайдере (`claude_agent`, план `Reply::Task(finish)`, образец — `c13_instances.rs:19`). Риги `web` и `api`.

- `ship` (вход `bead`): `plan` (exec) → `drain` (`formula = "work"`) → `report` (exec).
- `work` (вход `bead`): `do` (exec, `work.sh`) → `sign` (prompt, агент из бида).
- `plan.sh`: первая попытка держит (`touch held; exec sleep 60`), вторая заводит под `$AAE_BEAD` биды: A
  (`rig:web,agent:dev,harness:claude`), B (`rig:api,…`, `--deps blocks:A`), F (`rig:web,…`), H (`…,harness:codex`).
- `work.sh`: дописывает `$AAE_BEAD` в общий лог; по заголовку: A — первая попытка держит, F — `exit 3`.
- `report.sh`: первая попытка держит; вторая печатает `$AAE_STEP_DRAIN_DATA`.

Ход: старт `ship` с `{"request": "Ship it"}` на `web` → `kill -9` (`d.restart()`) в `plan` → `kill -9`, когда ребёнок
A в шаге `do`, а F ждёт каталог → `kill -9` в `report`. Проверки: в br один эпик с заголовком запроса; у корня
`run.step_finished` по одному на `plan` и `drain`, попыток drain — одна; детей ровно три (A, B, F), у каждого
`parent_run`, correlation корня, `depth = 1`; A — риг `web`, агент `dev`, попытка с `harness = claude`; B — риг `api`,
стартовал после закрытия A; F — `failed{agent}`, один прогон; для H прогона нет; в логе A, B, F по одному разу;
stdout `report`: `closed = [A, B]`, `failed = [F]`, `refused` содержит H с обоими харнессами; br: A и B закрыты, F и H
открыты без исполнителя, корень открыт без исполнителя.
Запасной вариант, если `sign` на поддельном провайдере нестабилен под рестартом: `work` из одного exec-шага, а агент и
харнесс из бида проверяются только в `tests/beads.rs` и отказом по H — тогда так и написать в отчёте.

## 7. Тесты ядра — `aae-testkit/tests/drain.rs` (MemoryTracker, сценарный драйвер)

1. `a_drain_gives_every_ready_bead_under_the_root_a_run` — дети на риге родителя и на другом; риг и агент из меток;
   `parent_run`, correlation, depth; биды закрыты; итог шага; `max_runs = 1` не мешает.
2. `a_bead_added_meanwhile_runs_too` — бид добавлен, пока жив ребёнок, без `TrackerChanged`: его приносит чтение
   после close.
3. `a_failed_bead_stops_nothing_and_runs_once` — следующий шаг стартует, `failed = [b]`, ребёнок один.
4. `a_refused_bead_gets_no_run_and_a_reason` — `harness:` расходится; нет `rig:`.
5. `a_restart_mid_drain_keeps_the_attempt_and_gives_no_bead_twice` — один закрыт, один жив, один в очереди;
   `w.restart()`; у корня нет `run.attempt_failed`, у каждого бида один прогон.
6. Запрос (коммит 2). 7. Конфиг (`aae-core/src/tests`): ссылка на формулу, вход исполнителя, вход формулы с drain.

## 8. Что проверено на br 0.7.6 (временный трекер, `BEADS_DIR` явно)

- Дети захваченного (`in_progress`) корня готовы; заблокированный `blocks:` — нет.
- `br close` родителя с открытыми детьми: код 3, `skipped: parent issue has N open children`.
- Освобождённый эпик с открытыми детьми в `ready` не попадает; `br show` ребёнка несёт `parent`.
- Метка с точкой отвергается (`only alphanumeric, hyphen, underscore, colon`): `agent:` — имя без пака.
- `--external-ref` пишется и виден в `br list -a --json`; `br show <external-ref>` не находит — поиск только списком.

## 9. Доки

`tracker-br.md` (корень: epic, `external_ref`, close → release; метки; «под корнем» — прямые дети), `contracts.md`
5.1–5.2 (шесть операций), 1.3 и 6.1–6.2 (`step_kind = drain`, попытка без процесса, recover), `config.md` «Formula»
(шаг `drain`, таймаут 24 ч, вход `{ request }`), `invariants.md` (Recovery, список проверок хаоса), спека §10.1 —
одна фраза (п. 10.3).

## 10. Решения по [Д] и вопросы — в журнал

Решения:
1. Форма запроса — `{ request = "<текст>" }` во входе типа `bead`; голая строка остаётся id.
2. Корень создаётся в `preparing`, а не при приёме запроса: у прогона в `queued` бида ещё нет. Тип `epic`, без меток.
3. **Отступление от §10.1 [Д]:** операции «биды под корневым» нет — её закрывают `ready()` и `Bead.epic` (решение 52
   [Р]: операция только когда нужна вызывающему). «Под корневым» = прямые дети.
4. Бид выдаётся один раз за попытку шага `drain`; провалившийся в этом шаге не повторяется.
5. Метки действуют на любой прогон на биде; нет `agent:` — агент формулы; нет `harness:` — проверки нет; `rig:`
   обязателен, когда риг не назван. Имя в `agent:` — без пака (ограничение br).
6. Отказ по метке: без прогона, без ворот и без отдельного события; причина — в итоге шага и в ответе `rejected`.
7. Шаг `drain` всегда `pass`; итог — `{closed, failed, refused}`.
8. Таймаут `drain` по умолчанию 24 ч.
9. Корень прошедшего прогона с открытыми детьми br не закрывает — адаптер возвращает его открытым.
10. Попытка `drain` не получает `daemon_crash`; прогон в `drain` не занимает слот `max_runs` и не держит каталог от
    своих детей.

Известные пределы (комментарий `shortcut:` + строка в доках): корень-сирота, если трекер не ответил за
`start_timeout`; провал (не отмена) родителя оставляет детей жить; плановый `aae drain` демона ждёт прогон в шаге
`drain` до своего таймаута — проверить `read.rs:115`; биды глубже прямых детей не выдаются.

Вопросы владельцу: (а) выдавать ли потомков глубже прямых детей; (б) нужен ли `aae run start --request "<текст>"`;
(в) закрывать ли корень принудительно, когда прогон `passed`, а под ним остались открытые биды.
