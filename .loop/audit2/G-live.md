# G. Живое поведение глазами нового пользователя (аудит 2, 2026-11-14)

Аудитор G. HEAD `9e6c3eb`, бинарник `target/debug/aae` (собран 17:16, пересборка не понадобилась; чужие правки
`crates/aae-core/src/config/*` в рабочем дереве появились в 20:04, уже после моих прогонов — в проверенный бинарник
не попали). Всё запускалось в `/tmp/aae-audit2-live/` с собственными `HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`,
`XDG_RUNTIME_DIR` (продукт читает только `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR`, `HOME`, `AAE_SOCKET` —
`crates/aae-front/src/dirs.rs`, `crates/aae/src/daemon.rs:43`). Живые модели — отдельный демон
`/tmp/aae-audit2-live/live` с настоящим `HOME` (логины) и своими state/runtime. В конце все демоны остановлены
(`aae drain`), `pgrep -x aae` и `pgrep -fa aae-audit2-live` пусты, сирот `sleep` нет. Репозиторий не менялся, кроме
этой записи и одной записи в `.loop/JOURNAL.md` (живые модели).

Времена в событиях — UTC. `ev` — мой скрипт-выжимка из `events.jsonl` (seq, время, тип, прогон, ключевые поля).

---

## Находки

### 1. Простой демона съедает дедлайн решающих хуков: ложный `hook_deadline` в `preparing` и `no_decision` в `interrupted`

**класс**: баг · **важность**: высокая · **где**: `crates/aae-core/src/run/decide.rs:34` (дедлайн ставится один раз,
абсолютным временем), `decide.rs:49-56` (`decide_deadline`), восстановление его не сдвигает
(`crates/aae-core/src/run/recover.rs`) · **требование**: `docs/design/invariants.md:54-57` («A run in `preparing` or
`interrupted` keeps its state and its hooks run again», «Re-arm deadlines»), `docs/guide/plugin.md` («a hook cut by a
daemon crash runs again after the restart»), contracts 3.3 («one hook can never hold a run longer than its own
timeout») — нигде не сказано, что простой демона = отказ хука.

**факт** (три прогона, плагин с одним хуком `sleep 30` → решение, `timeout = "40s"`; дедлайн события = 40 + 30
(bundled) + 5 = 75 с):

- **C** (`run.preparing`, демон лежал 51 с — дольше таймаута хука, но меньше дедлайна):
  ```
  242 12:54:00.964 run.state_changed run_24 queued->preparing attempt=0
  244 12:54:00.964 run.preparing     run_24 deadline=12:55:15.964
      (kill -9 демона 12:54:05, рестарт 12:54:56)
  245 12:54:56.111 daemon.started
  246 12:55:15.969 run.state_changed run_24 preparing->failed reason=hook_deadline attempt=0
  247 12:55:15.969 run.failed        run_24 reason=hook_deadline class=check
  хук: 12:54:00.97 start seq=244 → 12:54:30.99 end (осиротевший, доработал сам)
       12:54:56.13 start seq=244 → 12:55:26.14 end (повтор после рестарта; уложился в свои 40 с, решение `proceed` выброшено)
  ```
  Команда формулы не запускалась ни разу. Хук здоров (30 с < 40 с), никто не отказывал — прогон упал с `check`.
- **C2** (то же, демон лежал 81 с — дольше всего дедлайна): отказ через 3 мс после старта, хук всё равно
  перезапускается впустую:
  ```
  251 12:55:57.957 run.preparing run_25 deadline=12:57:12.957
  252 12:57:23.074 daemon.started
  253 12:57:23.077 run.state_changed run_25 preparing->failed reason=hook_deadline attempt=0
  254 12:57:23.077 run.failed run_25 reason=hook_deadline class=check
  хук: 12:55:57.99 start → 12:56:28.10 end; 12:57:23.10 start → 12:57:53.12 end (уже после провала прогона)
  ```
- **AF** (`run.attempt_failed`, хук без решения → должен решать bundled `default-policy`; формула с `timeout = "2s"`,
  в обычном потоке политика даёт вторую попытку — см. сценарий «таймаут шага» ниже; демон лежал 81 с):
  ```
  416 13:01:12.799 run.state_changed run_41 running->interrupted reason=runaway_turn attempt=1
  417 13:01:12.799 run.attempt_failed run_41 deadline=13:02:27.799
  418 13:02:36.872 daemon.started
  419 13:02:36.875 run.state_changed run_41 interrupted->failed reason=no_decision attempt=1
  420 13:02:36.875 run.failed run_41 reason=no_decision class=infrastructure
  ```
  Прогон, который политика повторила бы, стал терминально `failed{infrastructure}` только из-за простоя демона;
  `default-policy` не спрошена. Хук затем отработал второй раз впустую (13:02:36.90 → 13:03:06.92).

**как проверено**: `scen C ok 4 50 50`, `scen C2 ok 4 80 40`, `scen AF to 5 80 50` (скрипт: `aae run start`, через N с
`kill -9` демона, пауза, рестарт, выжимка событий); журналы — `/tmp/aae-audit2-live/out/scen-{C,C2,AF}.log`.

**что сделать** (M): при восстановлении для прогонов, ждущих решающее событие (`runs.waiting`), переставлять дедлайн
`decide` на «старт демона + сумма таймаутов хуков + 5 с» (хуки всё равно запускаются заново), либо вычитать простой.
Тест в testkit: рестарт позже дедлайна → прогон не падает до ответа перезапущенного хука.

### 2. `kill -9` в `finishing`: объявленный исход отбрасывается, exec-команда и хук выполняются второй раз (вход для спорного решения А5.3)

**класс**: отход от запроса (спорно; код идёт по букве §5.6, см. журнал, цикл 66) · **важность**: средняя ·
**где**: `crates/aae-core/src/run/recover.rs` (`finishing` среди прерываемых) · **требование**: §5.6, §9.4;
contracts 3.4.

**факт**. Формула `ok` — exec-шаг, пишет строку в файл-счётчик; плагин `run.finishing`: `sleep 30`, затем
`{"decision":"accept"}`, `timeout = "40s"`.

- **A** (рестарт через 2 с после `kill -9`):
  ```
  215 12:50:13.991 run.state_changed run_22 running->finishing attempt=1
  216 12:50:13.991 run.finishing run_22 remarks=0 deadline=12:50:58.991 outcome=pass/exit 0
      (kill -9 12:50:18.05; рестарт 12:50:20.09)
  217 12:50:20.102 run.state_changed run_22 finishing->interrupted reason=daemon_crash attempt=1
  218 12:50:20.102 run.attempt_failed run_22 deadline=12:50:55.102
  219 12:50:20.102 daemon.started
  220 12:50:20.115 run.state_changed run_22 interrupted->running attempt=2
  221 12:50:20.125 run.state_changed run_22 running->finishing attempt=2
  222 12:50:20.125 run.finishing run_22 remarks=0 deadline=12:51:05.125 outcome=pass/exit 0
  223 12:50:50.143 run.state_changed run_22 finishing->passed reason=pass attempt=2
  224 12:50:50.143 run.passed run_22 outcome=pass/exit 0
  хук: 12:50:14.00 start seq=216 pid=3364450 (осиротел; его `sleep 30` убит новым демоном по маркеру — к +2 с его нет)
       12:50:20.14 start seq=222 pid=3364770 → 12:50:50.14 end
  счётчик exec-команды: 2 строки (run_22, run_22)
  ```
- **B** (демон лежал 51 с — дольше таймаута хука 40 с и дедлайна события 45 с):
  ```
  230 12:51:50.689 run.state_changed run_23 running->finishing attempt=1
  231 12:51:50.689 run.finishing run_23 remarks=0 deadline=12:52:35.689 outcome=pass/exit 0
      (kill -9 12:51:54.71; рестарт 12:52:45.76)
  232 12:52:45.764 run.state_changed run_23 finishing->interrupted reason=daemon_crash attempt=1
  233 12:52:45.764 run.attempt_failed run_23 deadline=12:53:20.764
  234 12:52:45.764 daemon.started
  235 12:52:45.772 run.state_changed run_23 interrupted->running attempt=2
  236 12:52:45.776 run.state_changed run_23 running->finishing attempt=2
  237 12:52:45.776 run.finishing run_23 remarks=0 deadline=12:53:30.776 outcome=pass/exit 0
  238 12:53:15.794 run.state_changed run_23 finishing->passed reason=pass attempt=2
  239 12:53:15.794 run.passed run_23 outcome=pass/exit 0
  хук: 12:51:50.70 start seq=231 → 12:52:20.71 end (осиротевший доработал полностью, его accept ушёл в никуда)
       12:52:45.78 start seq=237 → 12:53:15.79 end
  счётчик exec-команды: 2 строки
  ```

Итог по обоим: заявленный исход попытки 1 (`pass/exit 0`, уже зафиксирован в `run.finishing` seq 216/231) теряется;
exec-команда, которая **полностью завершилась** до падения, выполняется второй раз; хук `run.finishing` запускается
дважды (в B — оба раза до конца), на разных `seq`. Ложного отказа по дедлайну в `finishing` нет (прогон уходит в
`interrupted` и получает новое событие с новым дедлайном) — в отличие от `preparing` (находка 1). Сирот нет. Для
exec-шага с побочными эффектами (деплой, push) это двойное выполнение уже завершённого шага; для prompt-шага —
потеря уже вызванного `aae finish pass` и новая попытка на модели (не проверено: модель).

**как проверено**: `scen A ok 4 1 75`, `scen B ok 4 50 45`; `/tmp/aae-audit2-live/out/scen-{A,B}.log`.

**что сделать**: решение владельца. Вариант цикла 47 (сохранять `finishing`, хуки заново) не повторяет завершённый
шаг, но тогда ему нужен перевзвод дедлайна из находки 1 — иначе в B был бы ложный `hook_deadline`.

### 3. Относительный `--config` оставляет все пути относительными: `BEADS_DIR` указывает мимо, `workdir` в событиях относительный

**класс**: баг · **важность**: средняя · **где**: `crates/aae/src/daemon.rs:43-46` (`--config` берётся как есть),
`crates/aae/src/harness.rs:93-97` · **требование**: §10.2 (трекер всегда по явному пути), first-project.md («A
relative project path is relative to the config file»).

**факт**: `cd /tmp/aae-audit2-live/alt && aae daemon --config c.toml` (в `c.toml` — `projects = ["p"]`), exec-шаг
`env | sort; pwd`:
```
BEADS_DIR=p/.beads
PWD=/tmp/aae-audit2-live/alt/p
```
то есть для шага `BEADS_DIR` = `…/alt/p/p/.beads`. `aae project list`: `"root": "p"`, `"path": "p/."`; `workdir`
прогона в событиях — `p/.` (хук плагина работает с cwd = каталог плагина и получит неверный путь). То же в
`aae config explain --config config.toml …`: `env.BEADS_DIR = "proj/.beads"`. `aae daemon --service` путь
абсолютизирует — страдает только ручной запуск.

**как проверено**: вывод выше; `/tmp/aae-audit2-live/out/envdump-rel`.

**что сделать** (S): `std::path::absolute` для `--config` в `Paths::resolve`; тест.

### 4. `aae run show` не говорит, почему прогон упал/отменён/ждёт

**класс**: недоделка · **важность**: средняя · **где**: `RunView` (вывод `GET /v1/runs/{id}`) · **требование**: §12
(«состояние прогона: где ждёт»), plugin.md («the run fails with class `check` and the reason from stderr»).

**факт**: причина есть только в событиях `run.state_changed`/`run.failed`/`run.cancelled`.
- Вето плагина (гайд plugin.md, `require = "tested"`): `aae run show run_3` → `"state": "failed"`, `"class": "check"`,
  `"outcome": {"result": "pass", "summary": "exit 0"}` — «failed с исходом pass», текста
  `the summary of a pass must say the work is tested` нет.
- Нет бинарника claude: `"class": "infrastructure"`, `"failure_kind": "config_error"`; `claude: No such file or
  directory` — только в `instance.failed`.
- `aae run cancel run_39 --reason "audit cancel"` — причина в `run show` не видна.
- Прогон в `requires_action` (живой claude по гайду): `"waiting_for": null`, id запроса в `run show` нет — гайд
  отправляет грепать `aae events --json | grep requires_action`.
- Прогон за воротами: `"waiting_for": "gate"` без ключа ворот (надо догадаться про `aae gate list`).

**как проверено**: вывод `aae run show` (все ключи: agent, attempt, attempts, bead, class, correlation, created_at,
depth, formula, generation, id, inputs, outcome, parent_run, project, rig, started_at, state, step_kind, waiting_for,
workdir).

**что сделать** (S–M): поля `reason`, `request` (открытый запрос), `gate` в `RunView`.

### 5. SIGTERM демона посреди exec-шага оставляет дерево процессов команды жить; в лог — «an effect panicked»

**класс**: недоделка · **важность**: средняя · **где**: путь остановки по сигналу; `crates/aae-core/src/writer.rs:278`
· **требование**: инвариант 7 (после остановки сессии не остаётся потомков), install.md (остановка — `aae drain`).

**факт**: `aae run start lab main slow` (`sh -c 'sleep 611 & sleep 611; wait'`), `kill -TERM <демон>`:
```
403 13:00:32.827 daemon.stopping reason=signal
```
демон вышел за < 2 с; `pgrep -x sleep -a` → `3387738 sleep 611`, `3387739 sleep 611` живы. В логе демона:
`aae: an effect panicked, it is retried with backoff: task 154 was cancelled`. Следующий старт их убирает
(`running->interrupted reason=daemon_crash`, новая попытка, новые pid). Под systemd маскируется убийством cgroup;
при ручном запуске (`Ctrl-C` в терминале гайда — «In one terminal: aae daemon») команда продолжает работать без
надзора до следующего старта демона. Плановая остановка названа `daemon_crash`.

**как проверено**: вывод выше, `out/daemon-D.log`.

**что сделать** (S–M): по сигналу останавливать активные exec-попытки и экземпляры (TERM группе, затем sweep) до
выхода, либо документировать; отмену задачи при остановке не печатать как panic.

### 6. Несуществующий файл в явном `--config` — не ошибка

**класс**: баг · **важность**: низкая–средняя · **где**: `crates/aae-core/src/config/load.rs:34` (`file.exists()` →
иначе пустой конфиг), `crates/aae/src/doctor.rs:53`.

**факт**: `aae config check --config nofile.toml` → пусто, rc 0. `aae daemon --config /…/nofile.toml` →
`aae daemon listening on …` (демон с пустым конфигом). `aae doctor` на чистой машине без конфига:
`ok config /tmp/aae-audit2-live/cfg/aae/config.toml` — файла нет. Опечатка в пути юнита = демон без проектов и «ok»
от всех проверок.

**что сделать** (S): отсутствие файла допустимо только для пути по умолчанию (и `doctor` пишет «no config file,
defaults»); явный `--config` без файла — ошибка.

### 7. `aae plugin list` без `--project` не показывает плагины проектов и bundled `worktree`

**класс**: отход от документа · **важность**: низкая · **где**: `crates/aae-front/src/api/config.rs:113-114` ·
**требование**: plugin.md («`aae plugin list` lists the declared and the bundled plugins»), help («List them, of
every project or of `--project`»).

**факт**: плагин объявлен в `proj/lab/aae.toml`, есть формула с `worktree = true`. `aae plugin list` → только
`default-policy`. `aae plugin list --project lab` → `slowfin`, `default-policy`, `worktree`. Остальные списки
(`formula`, `agent`, `pack`) без `--project` показывают все проекты.

**что сделать** (S): без `--project` — оркестратор + все проекты с полем `project`.

### 8. `aae status` во время drain/restart показывает `"stopping": null`

**класс**: баг · **важность**: низкая · **где**: `crates/aae-front/src/api/daemon.rs:28` (читает `state.stop`),
`daemon.rs:86-96` (`stop` выставляется только когда ожидание закончилось).

**факт**: `aae restart --timeout 5` при идущем прогоне → через 1 с `aae status | grep stopping` → `"stopping": null`
(событие `daemon.stopping{restart}` уже в журнале, новые прогоны уже не стартуют). То же при `aae drain --timeout 30`.
Дренаж до 300 с невидим в статусе. Попутно: `aae drain` возвращается сразу (0,01 с) с JSON; фраза install.md
«`aae drain` waits until no run is in a turn … and exits» читается как «команда ждёт».

**что сделать** (S): статус берёт причину из состояния ядра (`Msg::Stopping`), гайд уточнить.

### 9. Входы прогона: `--input file` без `=` и `null` проходят как обязательная строка; тип `string` не проверяется

**класс**: баг · **важность**: низкая · **где**: `crates/aae-front/src/cli/commands.rs:304`
(`split_once('=').unwrap_or((text, ""))`), проверка входов в ядре.

**факт** (вход `from = { type = "string", required = true }`):
`aae run start hello main summarize --input file` → `run_4`, `"inputs": {"file": ""}` (prompt-прогон запущен с пустым
именем файла); `--input from=null` → `run_37`, `"inputs": {"from": null}`, шаг получил `AAE_INPUT_FROM=null`;
`--input from=7` → integer 7; `--input 'from={"a":1}'` → объект в строковом входе. Пользователь с
`--input version=1.0` или `--input name=true` получит число/булево в строковом входе.

**что сделать** (S): `--input` без `=` — ошибка использования; для входа `string` значение не разбирать как JSON
(или проверять тип), `null` не засчитывать как присутствие `required`.

### 10. Вывод CLI без `--json` — это JSON почти везде; `null` в текстовом выводе

**класс**: мелочь (первый аудит, Г: «списки печатают JSON без `--json`» — не закрыто) · **важность**: низкая.

**факт**: человекочитаемый вывод есть только у `run list`, `gate list`, `events`, `run watch`, `doctor`,
`config explain`. Pretty-JSON без `--json`: `status`, `run show`, `project|formula|agent|pack|plugin|trigger|instance
list`, `config reload`, `trigger fire|enable|disable`, `drain`, `restart` — с `null` (`"last_error": null`,
`"stopping": null`, `"agent": null` …). `aae events`: `1  daemon.started  null`; `run.state_changed  run_1` без
`from → to` (самое полезное в строке). `aae instance list` пустой печатает `[]`, `aae gate list` пустой — ничего.
`aae config check` при успехе молчит (rc 0) — ок, но рядом `--json` даёт `{"errors":[],"ok":true}`.

### 11. Несогласованные коды и тексты ошибок

**класс**: мелочь · **важность**: низкая.

**факт**: нет такого прогона: `run show run_999` → `404 not_found`; `run cancel run_999`, `run answer`, а также
`instance stop nope`, `trigger fire … nope`, `run start nope main check` → `409 rejected` («нет объекта» как конфликт).
`run list --state bogus` / `--state PASSED` / `--project nope` → пусто, rc 0 (а `project list --project nope` → 404).
Текст дублирует слово: `aae: rejected: no formula `nope` (409 rejected)`. Ошибки разбора аргументов при `--json`
идут текстом clap (rc 2). Неизвестный маршрут API → 404 с пустым телом. `POST /v1/runs` с лишним полем (`"extra":1`)
принимается (200). `aae gate close rig:hello/zzz`, `project:nope` — ворота несуществующих объектов закрываются
молча.

### 12. `BEADS_DIR` и `BD_ACTOR` у шагов проекта без `.beads`

**класс**: мелочь (первый аудит, Г — не закрыто) · **где**: `crates/aae/src/harness.rs:93-97`.

**факт**: проект `lab` без `.beads`: exec-шаг получает `BEADS_DIR=/tmp/aae-audit2-live/proj/lab/.beads`,
`BD_ACTOR=aae:run_10:1`; `aae config explain` показывает `env.BEADS_DIR = … # project` новому пользователю, у
которого трекера нет; `env_keys: ["BEADS_DIR"]` в каждом `run.preparing`.

### 13. Ошибки конфига: «every error is reported» верно только между файлами

**класс**: мелочь · **требование**: first-project.md §5 («every error is reported with its file»).

**факт**: `max_runz = 3` + `[harness.claude] model = 5` в одном файле → сообщена одна ошибка (тип), неизвестный ключ —
только после исправления первой. Ошибка в `aae.toml` проекта (`bogus_key = 1`) скрывает ошибки его формул
(несуществующий агент `nobody`, `command = "true"`, `timeout = 5`) до следующего захода. Между файлами — всё
сообщается (проверено: 7 ошибок в 4 файлах одним выводом).

### 14. Опечатка в команде exec-формулы закрывает ворота всего рига бессрочно

**класс**: мелочь/UX (поведение задокументировано: contracts 6.2, строка «spawn error → `workspace_failure`», §10
«`fail{gate: rig, until: null}`») · **важность**: низкая.

**факт**: формула `command = ["/nonexistent/prog"]`: `run.failed reason=workspace_failure class=infrastructure`,
`gate.closed key=rig:lab/main closed_by=policy until=None`; следующий прогон любой другой формулы этого рига —
`queued`, `waiting_for: gate`, пока оператор не выполнит `aae gate open rig:lab/main`. В гайдах про ворота политики
ни слова — новый пользователь увидит «прогон висит в queued» без объяснения.

**что сделать** (S): абзац в first-project.md («если прогон стоит в `queued` — `aae gate list`»); по желанию — не
считать `ENOENT` команды отказом рабочего каталога.

### 15. Прочие мелочи

- `config_generation` растёт при каждом `aae config reload` без изменений (2 → 3 → 4) и сбрасывается при рестарте
  демона (8 → 2).
- `workdir` из гайда (`path = "."`) везде показывается как `…/hello/.` (не нормализован).
- `aae trigger list` у выключенного триггера показывает `next_at`, как у включённого.
- `aae events --from 99999` (дальше головы журнала) молча ждёт, без подсказки.
- `aae trigger fire` пропуск (`{"skipped": "input_unresolved"}`) — rc 0; задокументировано в help.

---

## Сценарии: команды, вывод, ожидание, вердикт

### Гайды

| Шаг | Команда | Факт | Вердикт |
|---|---|---|---|
| install: проверка | `aae doctor` (нет конфига) | 4 строки `ok`, rc 0; `ok config <несуществующий путь>` | работает; находка 6 |
| install: проверка | `aae doctor`, `aae doctor --json` (конфиг гайда, фейковый HOME) | `error claude (from PATH): not found`, `warning claude login …`, rc 1 | ок по документу |
| install: сервис | `aae daemon --service systemd` | юнит с `ExecStart`, `PATH`, `--config` абсолютный | ок |
| install: сборка, `mise install`, launchd | — | не проверялось (сборка debug из дерева; macOS нет) | не проверено |
| first-project §1–4 | `aae daemon`; `aae project list`; `aae formula list`; `aae run start hello main check`; `aae run list`; `aae run show run_1` | `run_1`; `run_1  passed  hello/main  hello.check  …`; JSON прогона | ок (вывод списков — JSON, находка 10) |
| first-project §4 | `aae events --from 0`; `aae run watch run_1` | строки `seq тип прогон`; watch завершился, rc 0 | ок (`null`, находка 10) |
| first-project §5 | `aae config check`; `aae config reload` | пусто rc 0; `{"config_generation": 2, …}` | ок |
| first-project: Triggers | `[[trigger]] every = "1h"`; `aae trigger list`; `aae trigger fire --project hello --rig main hourly-check`; `disable`/`enable` | `next_at` +1 ч; `{"run": "run_2"}`; `enabled: false/true` | ок |
| first-formula §1 | `aae config explain --project hello --agent helper` | 4 значения со слоями + строка `host:` | ок (BEADS_DIR — находка 12) |
| first-formula §3–4 | живой claude haiku, см. «Живые модели» | `requires_action` → `aae run answer … allow` → `passed` | ок, буквально по гайду |
| first-formula: worktree | `worktree = true`, два прогона сразу | оба `running` одновременно, каталоги `.git/aae/worktrees/run_16|17`, ветки `aae/run_16|17`; после — убраны | ок |
| first-formula: tracker | — | не проверялось (br) | не проверено |
| plugin: declare/attach | `[[plugin]] path = "~/aae-plugins/aae-plugin-review"`, `config = { require = "tested" }`; `plugins = [{ name = "review" }]` | `aae plugin list` показывает `review`; `explain` показывает `system_prompt`, `env.PATH`, `hooks.*`, `skills.*` с `# plugin review` | ок |
| plugin: вето | `aae run start hello main check` (сводка `exit 0` без слова) | `failed`, `class: check`; причина только в событиях | ок по поведению; находка 4 |
| plugin: accept | `require = "exit"` → `aae config reload` → прогон | `passed` | ок |
| plugin: agent-сторона (skills, harness-хук, prompt), git-плагин + `aae fetch`, сервис | — | не проверялось (модель / сеть) | не проверено |

### Поверхность CLI

`aae --help` и `--help` всех 18 команд и вложенных подкоманд — есть, без паник. Ошибочные вызовы (около 60, с `--json`
и без): паник нет; ошибки API при `--json` — `{"error":{"code","message"}}` в stderr, rc 1; ошибки clap — текст, rc 2.
`aae status` без демона: `aae: no daemon on …/aae.sock (…); start it with `aae daemon``, rc 1 — понятно.
Идемпотентность: `--key k1` повтор → тот же `run_34`; тот же ключ с другой формулой →
`422 idempotency_key_reused`. Находки — 9, 10, 11.

### Устойчивость на exec-формулах

| Сценарий | Команды | Факт | Ожидание | Вердикт |
|---|---|---|---|---|
| passed | `aae run start lab main ok` | `passed`, `outcome.data = {exit_code: 0, stdout: "out\n", stderr: "err\n"}` | exit 0 → pass | ок |
| failed | `… bad` (`exit 3`) | `failed`, `class: agent`, `summary: "exit 3"`, stderr в `data` | exit ≠ 0 → fail | ок |
| окружение шага | `… envdump` | `AAE_INSTANCE, AAE_RUN, BD_ACTOR, BEADS_DIR, HOME, LANG, PATH, PWD` | §2.1 п. 10 | ок (BEADS_DIR — находка 12) |
| команда не найдена | `… nocmd` | `workspace_failure`, ворота `rig:lab/main` закрыты политикой | contracts 6.2 | по документу; находка 14 |
| таймаут шага | `… to` (`timeout = "2s"`, `sleep 612 & sleep 612`) | `running->interrupted reason=runaway_turn` ×2 (попытки 1 и 2, команда выполнена 2 раза), затем `failed class=infrastructure`; `sleep 612` не осталось | contracts 6.2 | ок |
| отмена идущего | `… slow` → `aae run cancel run_13` | `cancelled reason=by the operator`; оба `sleep 611` убиты; следующий прогон стартовал | потомки убиты | ок |
| два прогона в риг | `slow`, `slow`, `ok` подряд | 1-й `running`, 2-й и 3-й `queued`, `waiting_for: "workdir"`; стартуют по очереди | второй ждёт | ок |
| `kill -9` демона посреди exec | `… mid` (`sleep 8`), `kill -9` на 2-й с, рестарт через 0,5 с | до рестарта `sleep 8` жив (сирота); после: `running->interrupted reason=daemon_crash`, `interrupted->running attempt=2`, старый `sleep` убит, новый запущен; счётчик запусков 2, «done» 1; итог `passed`; сирот нет | at-least-once, сирот нет | ок |
| `kill -9` в `finishing` (A, B) | см. находку 2 | команда ×2, хук ×2, исход переобъявлен | спорно | находка 2 |
| `kill -9` в `preparing` (C, C2) | см. находку 1 | ложный `hook_deadline`, `class=check` | хук повторяется, прогон идёт дальше | **баг**, находка 1 |
| `kill -9` в `interrupted` на хуке `attempt_failed` (AF) | см. находку 1 | `no_decision`, `class=infrastructure`, политика не спрошена | решает `default-policy` | **баг**, находка 1 |
| `aae drain` при идущем прогоне | `… mid`; `aae drain --timeout 30` | ответ сразу `{"active_runs": 1, "reason": "drain"}`; новый `aae run start` принят и стоит `queued`; прогон доработал (`passed`), демон вышел, сокет убран; после старта демона очередь поехала | install.md | ок; находка 8 |
| `aae restart` при идущем прогоне | `… slow`; `aae restart --timeout 5` | через 5 с тот же pid перезапустился; `running->interrupted reason=daemon_crash`, попытка 2, старые `sleep 611` убиты, новые запущены; повторный `aae restart` во время ожидания — тот же ответ, rc 0 | install.md | ок; находка 8 |
| SIGTERM демона при идущем прогоне | `kill -TERM` | демон вышел за < 2 с, `sleep 611` ×2 остались | — | находка 5 |
| второй демон на том же состоянии | `aae daemon` | `aae daemon: another daemon owns …/state/aae`, rc 1 | — | ок |
| цепочка по событию | `[[trigger]] on = "run.passed"`, `where = { "run.formula" = "lab.ok" }`, `inputs = { from = "{{ event.run.id }}" }` | `trigger.fired trigger=lab.chain new_run=run_30 cause_seq=297`; шаг получил `AAE_INPUT_FROM=run_29` | config.md | ок |
| расписание `every` | `every = "3s"`, прогон 7 с | `trigger.fired` 12:59:10, `trigger.skipped reason=overlap` 12:59:13 и :16, следующий `fired` 12:59:19 | overlap-пропуск | ок |
| ворота | `aae gate close rig:hello/main --reason audit`; прогон; `aae gate open …` | `rig:hello/main  operator  until opened  audit`; прогон `queued`, `waiting_for: gate`; после open — `passed`; `--until` в прошлом → 409, `--until tomorrow` → 400 | §5.7 | ок |
| битый конфиг: `check` | неизвестный ключ; неверный тип; незакрытый массив; несуществующий агент/харнесс/вход шаблона; несуществующий каталог рига и проекта; два источника триггера; неизвестный тип события | rc 1, файл + позиция + список допустимых ключей | first-project §5 | ок; находки 6, 13 |
| битый конфиг: `reload` на живом демоне | `bogus_key = 1` в `aae.toml` | `aae: the config has errors (400 config_error)` + текст; `--json` — `{"error":{"code":"config_error","errors":[…]}}`; старый конфиг в силе (расписание продолжило срабатывать, формулы на месте) | «the old config stays in force» | ок |
| `explain` несуществующего агента/проекта | `aae config explain --project hello --agent nope` | `aae: no agent `hello.nope` in `hello``, rc 1; с `--json` — `not_found` | — | ок |

### doctor и MCP

- `aae doctor --config config.toml` на хосте (настоящий HOME): `ok claude …/installs/claude/2.1/claude `2.1.325 (Claude
  Code)` (from mise)`, `ok claude login …/.credentials.json`, `ok codex …/codex `codex-cli 0.168.0` (from mise)`,
  `ok codex login MY_CLI_LLM_PROXY_API_KEY is set`, `ok codex sandbox every codex agent sets sandbox_mode` (в
  `~/.codex/config.toml` хоста `sandbox_mode` действительно задан), каталоги, состояние; rc 0.
- `GET /v1/doctor` (`curl --unix-socket …/aae.sock http://x/v1/doctor`): те же 11 проверок JSON-массивом, 200. Ок.
- MCP: отдельной команды stdio-фронтенда нет (и по §12 не требуется: MCP — это `POST /mcp` самого API;
  `aae mcp-bridge` — обратный мост для сервисов плагинов, скрыт из `--help`). `initialize` (2025-06-18) → `serverInfo
  {name: aae, version: 0.1.0}`; `tools/list` → пять инструментов: `list_formulas(project)`, `start_run(project, rig,
  formula, inputs, continue_from, idempotency_key)`, `run_state(id)`, `list_runs(project, state, limit)`,
  `cancel_run(id, reason, idempotency_key)`. Ок.

### Живые модели (ровно два прогона; запись в `.loop/JOURNAL.md` сделана)

- `claude --version` → `2.1.325`; `codex --version` → `codex-cli 0.168.0`; `codex login status` → `Not logged in`
  (хост ходит через прокси-провайдера из `~/.codex/config.toml`, `aae doctor` это видит).
- claude haiku, формула `summarize` буквально из first-formula.md: `running` → через 4 с `requires_action` (запрос
  `permission` на `aae finish pass …`) → `aae run answer run_1 <id> allow` → `finishing` → `passed`; `SUMMARY.md` создан;
  23 с вместе с ручным ответом; `aae instance list` во время ожидания показывает экземпляр claude. Ок.
- codex `gpt-5.6-luna`, «создай `CODEX-OK.txt` и вызови `aae finish pass`»: `passed` за 12 с, файл создан. `aae drain`
  после — app-server aae остановлен, сокет убран. Ок.

---

## Проверено и чисто

Все команды четырёх гайдов существуют и работают как написано (кроме оговорённого в находках 4, 7, 8); pass/fail
exec-шага с захватом stdout/stderr; таймаут шага; отмена с убийством потомков; очередь на один риг; `worktree = true`
(создание, параллельность, уборка каталога и ветки); `kill -9` посреди exec-команды (сирота убит при старте, повтор,
без дублей событий); осиротевший хук убивается новым демоном по маркеру; `aae drain` / `aae restart` (в том же
процессе, по таймауту); защита от второго демона; цепочка триггеров с подстановкой входа, `every` с overlap-пропуском,
`fire/disable/enable`; ворота оператора и политики, `--until`; `config check/explain` без демона, `reload` с ошибкой
оставляет старый конфиг; качество текстов ошибок конфига; идемпотентные ключи (повтор и 422); `--json`-форма ошибок
API; `aae doctor` и `GET /v1/doctor`; MCP `initialize` + `tools/list`; базовый путь на живых claude haiku и codex.
Паник CLI/демона, битых строк в `events.jsonl`, пропусков `seq` не встречено.

## Гипотезы (не проверено)

- Находка 1 должна так же бить по `run.turn_ended_without_outcome` (ветка `_ => self.apply(tx, row, None)` →
  `failed{hollow_done}`) и по прогону в `preparing`/`interrupted` при плановом `aae drain` + поздний старт, если дренаж
  не считает такой прогон «в ходе».
- При варианте «сохранять `finishing`» без перевзвода дедлайна сценарий B дал бы `failed{check} hook_deadline`.
- Прогон в `requires_action` с истёкшим за время простоя `human_timeout` получит `deny` сразу при старте.
- Относительный `--config` (находка 3) ломает `workdir` в `run.preparing` для внешних хуков и `br` в сессиях агентов.

## Не успел / не проверено

- `kill -9` в `finishing` для prompt-прогона (нужна модель сверх разрешённых двух прогонов); ветка `remark`.
- Трекер: `.beads`/br, вход `type = "bead"`, `bead.ready`-триггеры.
- `aae fetch` с git-паком/плагином и `aae.lock`; сервисы плагинов и `aae mcp-bridge`; agent-сторона плагина (skills,
  harness-хуки, фрагмент промпта) на модели.
- `api_tcp` с токеном; SSE `gap`/`lagged`; `run watch` с сообщениями живой сессии; `--continue-from` на prompt-прогоне.
- `cron`-триггеры и часовой пояс; пропущенное за простой расписание.
- Быстрый рестарт в `preparing` (хук дважды на одном `seq`, прогон проходит) — ожидаемо по документу, не гонял.
- Release-сборка, musl-цели, macOS/launchd, реальная установка юнита systemd.
