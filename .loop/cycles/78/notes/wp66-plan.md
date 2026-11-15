# WP-66 «Шаги формулы» — план реализации (expert, цикл 78)

Основание: спека §9.1, §9.3, §9.4, §5.5, §5.6; решения 13, 17, 80; критерий 24 (часть без бидов);
`.loop/OWNER-RULES.md` раздел 8. Код не запускался и не собирался: оценки строк — по чтению.

## 0. Суть в одном абзаце

Шаг — не новая машина, а **поле на прогоне**. Состояния `preparing … finishing` остаются как есть и относятся к
текущему шагу (§9.3 [Д]). Единственная новая развилка — в `finished()` (`run/finish.rs:79`): принятый `pass` не
последнего шага вместо `terminal("passed")` вызывает `advance()`, который в **той же транзакции** закрывает попытку,
сдвигает `runs.step` и заново входит в существующий `render_and_prepare()` (`run/machine.rs:40`). Нумерация попыток
остаётся сквозной по прогону, поэтому токены, ключи эффектов и `PromptKey` не меняются. `recover.rs` не меняется
вообще: текущий шаг лежит в состоянии и двигается атомарно с принятием исхода.

## 1. Шаг на машине состояний

**Хранение.** Миграция 18 в `store.rs:193`:
`ALTER TABLE runs ADD COLUMN step TEXT NOT NULL DEFAULT '1'; ALTER TABLE attempts ADD COLUMN step TEXT NOT NULL DEFAULT '1';`
Хранится **имя** шага, не индекс: `Row::view()` (`run/row.rs:85`) не видит конфига, а имя нужно в событиях; имя же
переживает вставку шага при `config reload`. `Row` получает поле `step` (+ `COLUMNS`, `row.rs:37`).

**`runs.agent` — агент текущего шага.** `advance()` пишет `step` и `agent` одним `UPDATE`. Тогда всё, что читает
`row.exec()` (`row.rs:80`) и `row.agent`, работает по текущему шагу без правок: ключи ворот (`gates.rs:68`),
запрет `remark` для exec (`decide.rs:238`), `step_kind` в `RunView`, снимок `runs.config` (он и так пишется в
`render_and_prepare`, значит ключ модели `gates.rs:74` тоже по шагу).

**Переход.** Новая функция в `run/finish.rs` (~30 строк; `machine.rs` уже 354 строки при лимите 400):

```
finished(): (Pass, None) => self.advance(tx, row)          // было terminal("passed")
advance():  следующий шаг по конфигу «сейчас» (позиция row.step + 1)
            нет следующего  -> terminal("passed", "pass")
            имя шага пропало из формулы -> terminal("failed", infrastructure, "step `x` is gone from `f`")
            есть -> outcome = Self::outcome(); end_attempt(None);
                    emit run.step_finished { run: view (ещё со старым step), outcome };
                    UPDATE runs SET step, agent; row.set_state("preparing"); render_and_prepare(tx, row)
```

`end_attempt` (`machine.rs:282`) переиспользуется: останавливает сессию `prompt`-шага, отпускает экземпляр и сервисы.
Провал шага (`Fail`, вето) — по-прежнему `terminal("failed")`: это и есть «провал шага без возврата завершает
прогон» (§9.1 [Д]); исход прогона = исход последней попытки = исход последнего шага (§9.4) — `terminal()` не трогаем.
Агент следующего шага — тем же помощником, что в `queue.rs:32-44` (вынести общий: `""` для exec, `for_run(..).0`
для prompt).

**Без ожидания интервала.** Без хуков `deciding()` применяет решение сразу (`decide.rs:40`), поэтому
`ExecDone(шаг k)` → `finishing` → `advance` → `preparing` → `start_exec(шаг k+1)` — одна транзакция, эффект после
фиксации. `run.preparing` решается на каждом шаге (хук может дать env и workdir шагу); `run.started` — один раз, как
сейчас (`prepare()`); захват бида — один раз (`machine.rs:34` уже проверяет `bead_data`). Плагин `worktree` на
повторный `run.preparing` идемпотентен (`aae-plugins/src/worktree.rs:67`).

**Своя сессия у каждого `prompt`-шага** (решение 17) — три точечные правки в `start_attempt`:
- `INSERT INTO attempts` пишет `step` (`machine.rs:196`, `exec_step.rs:39`);
- `last_session` для `own` — с условием `AND step = ?` (`machine.rs:122-132`): `resume` продолжает сессию только
  своего шага; первая попытка шага идёт с `kind = "new_session"` → `None` → новая сессия;
- `continue_from` действует только на первом шаге: в запрос `machine.rs:137` добавить
  `AND NOT EXISTS (SELECT 1 FROM attempts WHERE run = ?1 AND step != ?2)`; в `queue.rs:38` отказ, если **первый** шаг
  — exec.

**Потолок попыток (инвариант 16).** `decide.rs:149` считает все попытки прогона; у одношагового это ровно неудачные.
Добавить `AND failure IS NOT NULL` (то же в `chaos/check.rs:92`): для одношаговых поведение то же, а у формулы
потолок не съедают успешные шаги (иначе 4 шага × 3 раунда WP-68 уже 12 из 20). Формулировка в `invariants.md` и
contracts 6.1: «20 failed attempts per run».

## 2. Итог шага для следующих

**Хранилище — уже есть:** `attempts.outcome` (`{result, summary, data}`; у exec `data = {exit_code, stdout,
stderr}`, хвост 4 КиБ, `exec_step.rs:119`). Итог шага = `outcome` последней попытки с этим `step`. Новой таблицы нет.
Один помощник в `run/render.rs` (~15 строк):
`step_results(conn, run, current) -> Map<name, Outcome>` — `SELECT step, outcome FROM attempts a WHERE run = ?1 AND
step != ?2 AND outcome IS NOT NULL AND n = (SELECT MAX(n) FROM attempts WHERE run = a.run AND step = a.step)`.

- **Шаблон** (`prompt`-шаг): новый корень `steps` в контексте `render()` (`render.rs:43`):
  `{{ steps.<имя>.result }}`, `{{ steps.<имя>.summary }}`, `{{ steps.<имя>.data.<ключ> }}` (для exec —
  `data.stdout`, `data.stderr`, `data.exit_code`). `PROMPT_ROOTS` (`items.rs:21`) + ветка в `prompt_path`
  (`items.rs:247`): `<имя>` — шаг, стоящий **раньше**; поле — `result | summary | data…`. Неразрешённый путь на
  старте шага — `failed{infrastructure}`, как сейчас.
- **Окружение** (`exec`-шаг): в `start_exec` рядом с `AAE_INPUT_*` (`exec_step.rs:57-65`):
  `AAE_STEP_<NAME>_RESULT` (`pass|fail`), `AAE_STEP_<NAME>_SUMMARY`, `AAE_STEP_<NAME>_DATA` (JSON, если есть).
  `<NAME>` — тем же правилом, что `input_variable` (`config/mod.rs:313`; обобщить до префикса); два шага с одной
  переменной — ошибка конфига, как у входов (`items.rs:136-147`).
- В окружение сессии `prompt`-шага `AAE_STEP_*` не кладём: у него есть шаблон. Не просили — не делаем.

## 3. Рестарт (`kill -9`)

**Журналируется:** `runs.step`, `attempts.step` (состояние — источник истины); в журнале событий —
`run.step_finished {run, outcome}` и поле `step` в `RunView` всех событий прогона.

**`recover.rs` — без правок**, потому что граница шага атомарна:

| Где убит демон | Что в состоянии | Что делает существующий recover |
|---|---|---|
| шаг k `finishing`, ждёт хуков | `waiting` стоит, исход записан | ждёт дальше, хуки заново, команда/ход не повторяются (`recover.rs:79-89`) → `advance` |
| транзакция `advance` не зафиксирована | то же, что строкой выше | то же |
| шаг k+1 `preparing`, ждёт хуков | `step = k+1` | не в `caught`; `rearm` перевзводит `decide` (`recover.rs:112`) |
| шаг k+1 `starting`/`running` | `step = k+1`, попытка жива | `daemon_crash` → политика → новая попытка **того же** шага; `resume` берёт сессию этого шага |
| exec-команда шага в полёте | `exec:<run>` в эффектах | sweep по маркеру, попытка `daemon_crash`, команда шага запускается заново (contracts 6.2) |

Толкование критерия 24 «ни один шаг не исполнен дважды»: **законченный** шаг не повторяется; прерванная на полуслове
exec-команда перезапускается новой попыткой того же шага — как у одношаговых сейчас.

**Инварианты.** 2 — попытки сквозные, токен шага k после `advance` устарел (`finish.rs:43`), проверить тестом.
9, 10 — сдвиг шага и эффект следующего в одной транзакции. 11, 17 — `resume` только в сессию своего шага.
12, 13 — прогон `passed` только исходом последнего шага. 14 — `preparing` шага >1 либо мгновенно, либо с дедлайном
`decide`. 16 — см. п. 1.

**Хаос-стенд** (`aae-testkit/tests/chaos`; часы на паузе → exec-шагов там быть не может, правило 7 владельца):
- третья формула `ship` — три `prompt`-шага на сценарном драйвере (`main.rs:48 FIX`); `create()` (`steps.rs:72`)
  выбирает `build | fix | ship`;
- новые проверки в `check.rs` после каждого шага:
  S1 — у прогона нет двух `run.step_finished` с одним `run.step`, порядок — порядок формулы;
  S2 — `attempts.step` не убывает по `n` (в порядке формулы) и у последней попытки равен `runs.step`;
  S3 — ни одна `attempts.session` не встречается у двух разных `step` (решение 17);
  S4 — `passed` прогон `ship` имеет ровно два `run.step_finished`;
- `undone()` (`steps.rs:20`) получает категорию `step` (есть живой прогон с `step != '1'`), чтобы каждая доля сидов
  гарантированно убивала демон между шагами (`KILLS`).
Границы с exec — отдельным тестом на настоящем времени (п. 7).

## 4. Формат конфига и миграция

**Один синтаксис — массив `[[step]]`**, как `[[hook]]`, `[[trigger]]`, `[[plugin]]`; формула из одного шага — массив
из одного элемента (§9.1 [Р] «частный случай»). Двойного разбора `[step]`/`[[step]]` не заводим: релиза не было,
совместимость не нужна, а доки должны показывать одну форму.

```toml
name = "ship"
[[step]]
name = "build"            # необязательно; по умолчанию — номер шага: "1", "2", …
kind = "exec"
command = ["make"]
[[step]]
name = "review"
kind = "prompt"
agent = "dev"
prompt = "review.md"      # {{ steps.build.data.stdout }}
```

- `files.rs:333`: `pub step: Vec<StepFile>`; в оба варианта `StepFile` — `name: Option<String>` (файл 393 строки,
  лимит 400 — влезает).
- `config/mod.rs:175-196`: `Formula.steps: Vec<Step>`;
  `struct Step { name: String, timeout: Option<Duration>, kind: StepKind }`,
  `enum StepKind { Prompt { agent, prompt, settings }, Exec { command } }` (WP-67 добавит `Drain`).
- `items.rs:99`: цикл по шагам; проверки: шагов ≥ 1; `self.name(file, "step", ..)`; имена уникальны; коллизия
  переменных окружения; `steps.<имя>` в шаблоне — только более ранний шаг.
- Старый `[step]` даёт ошибку загрузки от serde (`invalid type: map, expected a sequence`) — достаточно, тест на это.

**Сколько мест** (`grep -rE '\[step\]'`, без `docs/init/reports`): **53 вхождения в 39 файлах** — `crates` 45 в 32
(testkit/tests 12 файлов, aae/tests 10, aae-core/src 5, aae-acceptance 5), `docs` 4 в 3 (`config.md`,
`guide/first-formula.md`, `guide/first-project.md`), `demos` 4 в 4 (уйдут в WP-70, но до тех пор должны грузиться).
Правка механическая: `git grep -lE '\[step\]' -- crates docs/design docs/guide demos | xargs sed -i 's/\[step\]/[[step]]/g'`,
затем глазами `git diff --stat`. Имена шагам в этих файлах не нужны (умолчание `"1"`).

**Потребители `formula.step`** (все — цикл по шагам или шаг по имени): `compose.rs:47` `for_run(project, rig,
formula, step: &str)` и `:199` `check_runs`; `check.rs:30`; `queue.rs:32`; `render.rs:23`; `reload.rs:109`
(отпечатки **всех** prompt-шагов, иначе reload остановит экземпляр следующего шага); `aae/src/doctor.rs:119`;
`aae-front/src/api/config.rs:193`. Вызовы `for_run` в тестах: `role_prompt.rs` ×4, `config_launch.rs` ×1 — добавить
`"1"`.

## 5. `explain` и OpenAPI

- Слой настроек шага называет шаг: `Layer::Formula(format!("{name} step {step}"))` (`compose.rs:57`) → в выводе
  «formula kit.ship step review» (поправить `config_launch.rs:106`).
- `explained()` (`api/config.rs:188`) для формулы идёт по шагам; `text` — секции по порядку:
  `# step `review` of `ship` on `web`: agent kit.dev` + `effective.explain()`; для exec —
  `# step `build` of `ship`: exec `make``. Параметр `--step` не добавляем.
- `Explained` (`api/replies.rs:173`): `agent: Option<String>` (для формулы `null`), новое
  `steps: Vec<{ step, kind, agent: Option<String> }>` (для `--agent` пусто); `inherits_host` — «хоть один шаг».
- `Run` / `RunView`: новое поле `step: String` (`replies.rs:12`, `row.rs:91`, `schema.rs:28 RUN`); `agent` и
  `step_kind` — текущего шага. `Attempt` (`replies.rs:72`, `row.rs:129`): новое `step: String` — видно, какая
  сессия чьего шага (§7.7).
- Новое событие `run.step_finished {run, outcome}`: contracts 1.3 + `schema.rs:97 payload` (для `where` триггеров).
  Новый тип и необязательные поля версию схемы не поднимают (contracts 1.1).
- `docs/api/openapi.json` перегенерировать (сверяет `aae-front/tests/openapi.rs`).

## 6. Коммиты и строки

| № | Коммит | Ядро | Прочее |
|---|---|---|---|
| 1 | Formula files list their steps as `[[step]]`: типы, загрузчик, `for_run(.., step)`, sed-миграция 39 файлов; временный отказ «several steps» (2 строки, снимается в №2) | +55 | тесты конфига +40 |
| 2 | A run goes through the steps of its formula: миграция 18, `Row.step`, `advance`, сессия на шаг, потолок по неудачным, `run.step_finished`, `RunView.step`, reload/doctor по шагам | +75 | `aae-testkit/tests/steps.rs` ~150 |
| 3 | A step's outcome reaches the later steps: `step_results`, корень `steps`, `AAE_STEP_*` | +45 | +50 к `steps.rs` |
| 4 | `config explain --formula` names each step; `Run.step`, `Attempt.step`, `Explained.steps`; OpenAPI | +5 | front +30, тест CLI +20 |
| 5 | The chaos stand kills between steps: формула `ship`, S1–S4, категория `step` | 0 | chaos +70 |
| 6 | Docs: contracts 1.3, 2.3, 6.1, 6.2, 6.4; `config.md` «Formula» и explain; `invariants.md` (16, Recovery, список проверок); `guide/first-formula.md`, `first-project.md` | 0 | доки |

Ядро: **+180…200 строк** → ~9 750 из 18 000 (сейчас 9 555). Фронтенды +30 (2 636 / 4 000).
`scripts/loop/checks 78` — один раз в конце.

## 7. Тесты «Готово, когда»

Новый `crates/aae-testkit/tests/steps.rs`, настоящее время (есть exec — `start_paused` запрещён), помощники уже
есть: `hooked_in`, `Hanging`, `w.until`, `w.decide`, `w.restart` (образец — `restart_decisions.rs:164`).

1. `three_exec_steps_run_in_order_once_each` — `a, b, c`, каждый дописывает строку в файл в риге. Лог `a\nb\nc\n`;
   `run.step_finished` ×2 (`a`, `b`); `attempts.step = a, b, c`; исход прогона — шага `c`; шаг `b` видит
   `AAE_STEP_A_RESULT=pass` и stdout шага `a` в `AAE_STEP_A_DATA`.
2. `exec_then_prompt_then_exec` — `pre` (exec, печатает слово) → `work` (prompt, шаблон с
   `{{ steps.pre.data.stdout }}`, сценарный драйвер, финишер `pass` с `data`) → `post` (exec, проверяет
   `AAE_STEP_WORK_RESULT` и `AAE_STEP_WORK_DATA`). Промпт содержит слово; пока идёт `work`, `RunView` =
   `{step: work, step_kind: prompt, agent: dev}`; сессия ровно одна; финишер с токеном шага `work` после перехода —
   `stale_attempt`.
3. `each_prompt_step_has_its_own_session` — prompt → prompt: две разные сессии, обе `new_session`; сбой попытки на
   втором шаге + решение `resume` возобновляет сессию **второго** шага.
4. `a_failed_step_fails_the_run` — `false` посередине: `failed{agent}`, исход — этого шага, третий шаг не запускался.
5. `a_kill_at_every_step_boundary_keeps_the_step` — формула из п. 2, точки (цикл по точкам или шесть коротких
   тестов): (а) `a` в `finishing` под висящим хуком; (б) `b` в `preparing` под хуком; (в) `b` в середине хода;
   (г) `b` в `finishing`; (д) `c` в `preparing`; (е) `c` — команда в полёте (`sleep`, строка в лог после него).
   После `w.restart()` и решений: прогон `passed`; в логе каждая строка один раз; `run.step_finished` по одному на
   `a` и `b`; `attempts.step` не убывает; сессия шага `b` одна (в (в) — `resume` в неё же).
6. Конфиг (`aae-core/src/tests`): имена по умолчанию `"1"`, `"2"`; дубль имени; `steps.<позже>` в шаблоне; старый
   `[step]`; коллизия `AAE_STEP_*` — ошибки загрузки с именем файла.
7. `aae config explain --formula` печатает заголовок каждого шага (`aae/tests/cli.rs`).

Сценарий c24 в `aae-acceptance` с настоящим `kill -9` — WP-67 (критерий 24 целиком), здесь не делаем.

## 8. Что оставлено WP-67 и WP-68 (модель не мешает)

- **WP-67 `drain`:** `StepKind::Drain`, третья ветка в `start_attempt` рядом с `row.exec()` (`machine.rs:103`); итог
  шага — в `attempts.outcome` строки с `harness = 'drain'`, дальше общий путь `finishing` → `advance`. Вид шага тогда
  читать из снимка `runs.config`, а не из «агент пуст».
- **WP-68 возврат:** развилка там же, в `finished()`, ветка `Fail`: шаг с возвратом пишет `runs.step` назад. «Возврат
  продолжает сессию шага» уже следует из `last_session … AND step = ?`. `step_results` берёт последнюю попытку шага,
  так что повторный заход перекрывает прежний итог. Проверки S1, S2, S4 хаоса станут «в пределах раунда».

## 9. Допущения — дописать в `.loop/JOURNAL.md` при реализации (РЕШЕНИЕ)

1. Синтаксис один — `[[step]]`; `[step]` перестаёт грузиться (53 вхождения переписаны).
2. Инвариант 16: потолок 20 считает **неудачные** попытки прогона; для одношаговых поведение прежнее.
3. Критерий 24 «не исполнен дважды» = законченный шаг не повторяется; прерванная exec-команда перезапускается.
4. `run.preparing` решается на каждом шаге; `run.started` — один раз на прогон.
5. Итог шага: шаблон — `prompt`-шагам, окружение — `exec`-шагам; `AAE_STEP_*` в сессию агента не кладётся.
6. Известные пределы (комментарий `shortcut:` + строка в `config.md`): лимит `max_runs` агента читается при выходе
   из очереди и из парковки, а не на границе шага; `agent_failures_1h` (`machine.rs:263`) относит сбои прежних шагов
   к агенту текущего; суммарный размер `AAE_STEP_*_DATA` не ограничен (до 64 КиБ на шаг).
