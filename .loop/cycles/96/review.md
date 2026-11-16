SCORE: 7.0/10
VERDICT: continue
BLOCKERS: 2

# Ревью цикла 96

Дифф `bb29d57..1027aad`: 5 коммитов, 54 файла, +313/−159. Что вошло:
- WP-78b: вход `task` вместо `bead` и `{ request }` (ядро, br, testkit, фронтенды, демо, доки);
- починка c19: прогон в `queued` без бида держится воротами `bead:<project>/<строка>`;
- READINESS: br adapter 6 → 8;
- жел. 1, 2, 3, 5 ревью 95.

Спека не менялась (`git diff --stat -- docs/init` пуст). Незакоммиченное — только `.loop/` цикла, вне `.loop` чисто.

`checks.md`: HEAD `1027aad` совпадает с текущим, RESULT pass. Полный набор не повторял. Время:
- ci 55 с (цикл 95: 58 с);
- nextest 488/488 за 47,5 с (цикл 95: 43,8 с за 485). Рост — chaos-сиды: 31 → 36 с. Они идут последними и
  параллельно, их логика в цикле не менялась (только `type = "task"` в двух константах). Дисперсия, не причина для
  пункта: в цикле 94 было 58,7 с;
- acceptance 79 с (цикл 95: 85 с), 31 pass / 0 fail / 9 not run / 6 open.

## Обязательно до продолжения

1. **«Бид, неизвестный ядру и занятый другим прогоном, — `failed` с `held by`» не покрыт тестом. Это инвариант 3:
   у бида не больше одного живого владельца.**
   - Где: `crates/aae-core/src/run/drain.rs`, `created()`, ветка `Err(Error::Rejected(reason)) => return
     self.terminal(...)` после `own_bead`.
   - Мутация: ветка заменена на `Err(Error::Rejected(_)) => {}`. Выжила на всём наборе (`cargo nextest run
     --workspace`, 488/488). Откатил, `git status` чист.
   - Поведение под мутацией: второй прогон пишет себе `runs.bead` чужого бида и шлёт claim с поколением владельца.
     Ответ отбрасывается (`owns` ложно), и прогон висит в `preparing` до `start_timeout`.
   - Документ (`tracker-br.md`, «a bead that another run holds fails the run (`held by`)») и журнал (19:40) обещают
     это поведение. Тест — нет: `a_task_naming_a_bead_takes_it_up_and_files_no_second_bead` проверяет отказ при
     старте, только для бида, который ядро уже знает.
   - Что сделать: два прогона подряд на id бида, ещё не прочитанного ядром (`w.tracker.add` без `w.changed()`). Первый
     держит бид, второй — `run.failed` с `held by`, исполнитель первого не тронут. Проверить ту же мутацию.
2. **`[root] vars` — третья настройка корня, а §6.1 [Р] даёт корню две.** Перенос обязательного пункта 1 ревью 95
   (раньше — 94 п. 2, 93 п. 3). Ждёт владельца.
   - Где: `crates/aae-core/src/config/files.rs` `RootFile.vars`, демо 2 `[root.vars] checks`.
   - Ответа владельца в журнале нет; кодер код правильно не трогал.

## Желательно

1. **Метки бида, названного задачей и не прочитанного ядром, не проверяются.**
   - На этом пути `bead_agent(..., None)` (`run/queue.rs`, ветка `is_text`). Ответ claim сверяет только риг
     (`run/beads.rs`, `tracked`, `bead_of_another_rig`).
   - Следствие: бид с `agent:reviewer` или `harness:codex`, заведённый руками и сразу названный в `--input task=<id>`,
     идёт с агентом формулы и без сверки харнесса. Бид, известный ядру, на той же метке отвергается при старте.
     §10.3 [Д]: «бид, чья метка расходится с эффективным конфигом, не выдаётся».
   - Дыра была и до цикла: именованный id, не прочитанный ядром. Но после WP-78b это главный путь «пользователь завёл
     бид и запустил по нему формулу».
   - Что сделать: сверить метки по `data` в ответе claim (как риг) и отпустить бид при расхождении.
2. **Ворота `bead:` у `queued` без бида берут все строковые входы, а не вход `task`.**
   - Где: `run/gates.rs` `keys`. Строковый вход `name = "web"` даёт ключ `bead:<project>/web`.
   - Безвредно, пока такие ворота не закрыты. Но `task_key` уже есть — брать только его.
3. **Ворота `bead:` у `queued` без бида держит только acceptance c19.**
   - Мутация `&& false` выжила на 61 тесте nextest (`gate|bead|task|drain`). c19 её поймал: `preparing` вместо
     `queued`.
   - Быстрый тест в `tests/core/gates.rs` (managed clock) дешевле: §2 решение 37.
4. **В `docs/design/contracts.md` осталось «request» как форма входа.**
   - Строка 533: «the root bead of a run started from a request».
   - Строка 646, переход `queued → preparing`: «with a request: the tracker creates the root bead».
   - Формы `{ request }` больше нет — должно быть «with a task text that names no bead the core knows».
   - Ещё в `config.md` фраза «A string that is the id of a bead the run takes it up» — грамматика.
5. **`type = "bead"` даёт голое `unknown variant 'bead', expected one of 'task', …`.**
   - Для ушедших ключей есть однострочная подсказка, куда они ушли (критерий 27, подсказки WP-76). Для ушедшего типа
     входа её нет.
6. Перенос из ревью 95:
   - жел. 4: маркер `full-run` пишется руками — WP-85;
   - жел. 6: строка §19 «контрактным тестом адаптера это ещё не закреплено» — WP-85;
   - жел. 7: живой codex без git — WP-84; мутации подсказок WP-76.

## Готовность компонентов (§14)

| компонент | балл | чего не хватает до следующей точки |
|---|---|---|
| Event loop and state | 6 | 8: критерий 8 на настоящих харнессах на текущем коде |
| Runs and finisher | 6 | 8: 29, 30, 31, 34; 2 и 4a на текущем коде; обяз. 1 |
| Triggers and schedules | 8 | 10: демо нового вида вживую |
| Issue gates | 8 | 10: демо; claude-половина 10 — [НП] |
| Recovery | 6 | 8: 29 (счётчик повторов через `kill -9`); 8 на настоящих харнессах |
| Config assembly | 6 | 8: 33, 31 (`internal`) |
| Instance and service registry | 8 | 10: демо нового вида вживую |
| claude adapter | 6 | 8: 2, 4a на текущем коде; 34 |
| codex adapter | 6 | 8: 2 на текущем коде; 34 |
| br adapter | 8 | 10: демо нового вида вживую (его критерии 5a, 11, 16, 24, 28, 32 зелёные в CI) |
| Plugin hook execution | 8 | 10: демо нового вида вживую |
| `default-policy` | 6 | 8: критерий 8 на настоящих харнессах на текущем коде |
| `worktree` | 8 | 10: демо без worktree |
| Shared MCP | 8 | 10: демо без MCP-сервиса |
| HTTP API | 8 | 10: демо нового вида вживую |
| CLI | 8 | 10: то же |
| MCP frontend | 8 | 10: живой claude на текущем коде (строка 17-запрос) |
| Acceptance spec | 6 | 8: строки для 29–31, 33, 34 |
| Delivery and `doctor` | 6 | 8: критерий 1 на текущем коде |

Баллы READINESS совпадают с моими. br adapter 6 → 8 принимаю: строка 28 зелёная, мутации адаптера краснеют (ниже).
Обязательный пункт 1 — в ядре, не в адаптере.

Минимум — 6, поэтому SCORE не выше 7. Не начаты 29, 30, 31, 33, 34; 20 — красный. Сделан критерий 28, но потолок
ставит минимум компонентов, поэтому SCORE остаётся 7,0.

## Критерии §15

| № | статус | чем проверено |
|---|---|---|
| 1 | manual | запрос; на текущем коде не шёл (последний `ffaccea`) |
| 2 | manual | запрос; на текущем коде не шёл |
| 3 | green | acceptance CI |
| 4 | green | acceptance CI |
| 4a | manual | запрос; на текущем коде не шёл |
| 5a | green | acceptance CI |
| 5b | green | acceptance CI (2 строки) |
| 6 | green | acceptance CI |
| 7 | green | acceptance CI |
| 8 | green (CI) / manual (запрос) | 20 kill -9 в CI; 5 на настоящих харнессах на текущем коде не шли |
| 9 | green | acceptance CI |
| 10 | green | acceptance CI (codex); claude — [НП] |
| 11 | green | 41 контрактный тест (+1 br: id отдаёт бид, `BD_ISSUE_PREFIX` не доходит до корня) |
| 12 | green | acceptance CI |
| 13 | green | acceptance CI |
| 14 | green | acceptance CI |
| 15 | green | acceptance CI |
| 16 | green | acceptance CI (2 строки) |
| 17 | green | acceptance CI; живой MCP-claude не шёл |
| 18 | green | acceptance CI |
| 19 | green | acceptance CI (после починки ворот `queued`) |
| 20 | red | CI-части зелёные; демо нового вида вживую не шли (`open`) |
| 22 | green (CI) / red (запрос «≥ 8») | бюджеты в норме (всего 16952/30000), READINESS на `db441b4`, баллы не выше потолков |
| 23 | green | acceptance CI |
| 24 | green | c24 на настоящем br, вход `task` |
| 25 | green | acceptance CI |
| 26 | green | acceptance CI |
| 27 | green | acceptance CI |
| 28 | green | строка 28, 7 тестов (testkit, конфиг, контракт br, настоящий демон), плюс живой путь (ниже); обяз. 1 — дыра в соседнем пути |
| 29 | not-started | WP-80 |
| 30 | not-started | WP-81 |
| 31 | not-started | WP-82 |
| 32 | green | строка 32 |
| 33 | not-started | WP-79 |
| 34 | not-started | WP-83 |

## Проверено

- `checks.md` на `1027aad`: pass. nextest 488/488 (47,5 с). acceptance 31 pass / 0 fail / 9 not run / 6 open.
  Бюджет: всего 16952/30000. Файлы диффа ≤ 400 строк (максимум — `tests/config.rs`, 399). `todo!` и `shortcut:` в
  добавленном нет.
- Флейки: 9 тестов строки 28 и `a_bead_someone_else_holds`, плюс `gate`-тесты (всего 25). 12 прогонов под загрузкой
  `yes` × nproc: 12/12 зелёные, 4,2–5,1 с.
- Мутации (после каждой — `git checkout`, `git status` чист):
  - `gates.rs`: ворота `queued` выключены (`&& false`). Убита acceptance c19; nextest (61 тест) не ловит — жел. 3;
  - `is_text`: без проверки таблицы `beads`. Убита: `drain::a_task_naming_a_bead_takes_it_up…`;
  - `is_text`: без проверки `ready_beads`. Убита: 12+ тестов (chaos, beads, drain);
  - `aae-br` `create`: поиск по id сломан. Убита: контракт br, `aae::tracker`, `aae::claude`;
  - testkit `create`: поиск по id сломан. Убита: 8+ тестов;
  - `created()`: отказ `own_bead` проглочен. **Выжила**, 488/488 — обяз. 1.
- Живой путь — демон из `target/debug/aae` в `/tmp/aae-rev96-*`, изолированные `XDG_*`, проект с br (`--prefix t`),
  формула с `task` и exec-шагом, CLI по `docs/guide`:
  - `--input 'task=Add a dark mode…'`: `passed`, заведён один эпик `t-0gt` с `external_ref aae-…`, в окружении шага
    `AAE_INPUT_TASK=t-0gt`;
  - `br create` и сразу `--input task=<id>`: бид подхвачен, второго нет, бид закрыт;
  - пустая задача: `rejected: a task is a text that is not blank`;
  - `{"request":"x"}`: отказ «a task is a text, a bead id or …»;
  - `{"next_ready":true}` при пустом наборе: exit 3;
  - заблокированный бид по id: `failed`, `bead_invalid … cannot claim blocked issue`;
  - закрытый бид: `failed`, `bead_claimed`.

  Демон остановлен (сверен по `XDG_STATE_HOME` процесса). Справка `first-formula.md` и `frontends.md` с кодом сходится.
- Остатки старых форм: `type = "bead"` и `{ request` в crates/demos/docs есть только в тестах на отказ.
- Язык продукта — английский; спека не тронута; новых зависимостей нет.

## Нарушения правил

Нет. Отчёт кодера правдив: «закрыто» по жел. 1–3 ревью 95 подтверждаю. Мутацию на beads.rs:154 кодер описал как
убитую — путь теперь достижим и покрыт тестом `beads.rs`.

## Следующий шаг

1. Обяз. 1 — один тест в `tests/core/drain.rs`. Заодно жел. 2–4: ключ ворот только от `task_key`, быстрый тест ворот,
   две строки `contracts.md`. Это S, ~30k.
2. WP-79 (критерий 33, M) — следующий по плану. Если к концу WP-79 бюджет остаётся, жел. 1 (метки бида при claim)
   отдельным коммитом. Иначе — в WP-80/WP-81, где трогается `drain`/claim.
3. `[root] vars` — только по ответу владельца.
