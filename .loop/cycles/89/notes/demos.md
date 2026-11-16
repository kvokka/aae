# Живой прогон демо, цикл 89, aae c1fa9bc

Команда: `mise exec -- cargo run -q --locked -p aae-acceptance -- 20` (полный режим; строки критерия 20 запускают
`AAE=target/debug/aae demos/<demo>/run.sh`). Лог: `.loop/cycles/89/notes/demo-both.log`; отчёт приёмки:
`target/acceptance/report.md`. Итог: 4 из 5 строк pass, 1 FAIL, 1550 с. На время прогона стоял
`git update-index --assume-unchanged` на `.loop/{state.json,JOURNAL.md,PLAN.md}` (чтобы sha не был `-dirty`); снят.

## Демо 1 (planner-executor; claude haiku + codex luna) — PASSED, 347 с
- sha: `aae-0.1.0@c1fa9bc` (без -dirty), claude 2.1.325, codex 0.168.0. RESULT: `demos/1-planner-executor/RESULT.md`.
- Токены: ввод 141 773, вывод 50 321.
- Drain: эпик `root-99i` + 2 плоские задачи (`.1` backend, `.2` frontend), вложенных эпиков нет; drain закрыл обе
  (`closed:[root-99i.1, root-99i.2], failed:[], refused:{}`), runs 1-3 passed, перезапусков нет.

## Демо 2 (multirepo-qa; codex sol/luna/terra) — FAIL: строка приёмки «did not finish within 1200s»
- Тот же sha c1fa9bc. Бидов: эпик `root-ng6` + `.1` (backend) + `.2` (frontend), вложенных нет, refused не было.
- Drain отработал: run_2 (backend) passed в 22:28:57; run_3 (frontend, codex) стартовал в 22:28:57.
- ЗАВИСАНИЕ: codex закончил ход (`task_complete` в rollout, 22:36:11Z; токены ввод 937 685, вывод 15 321), в
  `aae run show run_3` уже есть `outcome.result: pass` и `outcome.summary`, но run_3 остался `state: running, step: code`;
  после seq 44 (22:28:57) в журнале событий нет НИ ОДНОГО события (нет `run.finishing`). run_1 застрял на шаге
  `work` (drain, ждёт run_3). Наблюдал ≥10 минут после task_complete (до ~22:47Z), демон молчал (daemon.log — одна строка).
  Файлы фронтенда готовы, коммита в `frontend/.git` нет. codex app-server жив, CPU 0.8%.
- Журнал событий демо 2: `.loop/cycles/89/notes/demo2-events.jsonl` (46 КБ, seq 1-44).
- Отличие от цикла 84 (777 с passed): теперь codex-ход закончился, но aae не перевёл run_3 в finishing.
  Возможные причины для проверки: потеря/гонка сигнала конца хода codex после `aae finish`/outcome; регресс WP-74
  (drain/restart attempt). Не расследовал по коду (вне поручения).
- Демо остановил вручную: `kill` демона (drain ждал бы вечно: active_runs 1). Артефакты (`demos/2-multirepo-qa/.aae`,
  `frontend`, `backend`, `root/.beads`) остались, в .gitignore; перед повтором: `git clean -ffdX -- demos/2-multirepo-qa`.
- Для повтора демо 2 отдельно: `AAE=$PWD/target/debug/aae demos/2-multirepo-qa/run.sh` (повторный прогон не делал: одно демо — один прогон).

## Рабочее дерево
`git status --short`: только `.loop/` (JOURNAL, PLAN, state, cycles/89); в `demos/` отслеживаемых изменений нет.

## Демо 2, повтор (helper, HEAD c1fa9bc)
- Команда: `AAE=$PWD/target/debug/aae demos/2-multirepo-qa/run.sh` (одно демо; в `aae-acceptance` строки только для демо 2 нет). Перед
  прогоном состояние первого прогона сохранено в `.loop/cycles/89/notes/demo2-run1/` (daemon.log; RESULT.md не было), `git clean -ffdXq`.
  На время прогона `assume-unchanged` на .loop/{state.json,JOURNAL.md,PLAN.md} (снят), sha без -dirty.
- Итог: PASSED за 1070 с, 2 раунда проверок. sha `aae-0.1.0@c1fa9bc`, codex 0.168.0. RESULT: `demos/2-multirepo-qa/RESULT.md`;
  лог: `.loop/cycles/89/notes/demo2-run2.log`. Токены: ввод 2 681 200, вывод 30 094.
- Drain: эпик `root-ly9` + `.1` backend, `.2` frontend, затем `.3` (QA-раунд 2: «Hide game overlay during active play»); вложенных
  эпиков нет; runs: run_2 backend, run_3 frontend, run_4 frontend (бид .3), все passed; run_1 (dev.deliver) passed по qa. Перезапуск
  drain-попытки: `generation 6`, attempts до n=6 у run_1 (шаг work перезапускался при появлении нового бида в раунде 2). refused не смотрел отдельно, ошибок нет.
- Зависания нет: run_3 закончился штатно. Фоновые процессы: `python3 -m http.server 5173` кодера frontend был жив во время run_4,
  но ход кончился и прогон не завис. После конца строки процессов демо не осталось (pgrep пуст).
