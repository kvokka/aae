# Строка 24: команда бида F (`a-ty5`) исполнена дважды — разбор

Артефакты прогона: `/tmp/aae-acceptance-J1SBxB` (журнал `state/aae/journal/events.jsonl`, `state/aae/state.db`,
`work.log`). Биды: A = `w-r13`, B = `a-037`, F = `a-ty5`, H = `w-mdd`. `work.log`: `a-ty5, a-ty5, w-r13, a-037`.

## Вывод

Дефект продукта в восстановлении (`crates/aae-core/src/run/recover.rs`), не допущение теста о времени. Изменения
цикла 102 (`queue.rs::create`/`lent()`, `machine.rs::terminal` → `end_child`, `formula_step.rs`) ни при чём: оба
места закрыты условием `step_kind = 'formula'`, у `run_3` шаг `exec`, у родителя — `drain`.

## Механизм

1. Команда exec-шага — эффект `exec:<run>` (`exec_step.rs:98`). Исполнитель (`run/exec.rs:52–59`) запускает команду,
   шлёт писателю `Msg::ExecDone` и возвращает `true`.
2. Писатель (`writer.rs:279–296`) фиксирует `ExecDone` одной транзакцией (прогон → `failed`), а строку эффекта
   удаляет позже, отдельной записью (`effects::finish`, `writer.rs:291`) — после `fire_deadlines`, `journal.drain`,
   `effects::heads` и возврата задачи исполнителя. Между двумя записями — окно.
3. `kill -9` в этом окне оставляет в `effects` строку `exec:run_3` при прогоне уже в `failed`.
4. `recover()` чистит оставшиеся `exec:<id>` только у прогонов из `caught` — состояния `starting`, `running`,
   `requires_action`, `finishing` (`recover.rs:44–53`, блок 63–81). Прогон в `failed` (так же `passed`, `cancelled`,
   `preparing` следующего шага, `interrupted`) туда не входит: его эффект «продолжается сам» и новый демон
   исполняет команду ещё раз. Ответ отбрасывается (`exec_done`: `row.state != "running"`, `exec_step.rs:115`),
   поэтому в журнале и в `attempts` следа нет — только побочный эффект команды.

Комментарий в `recover.rs:63–65` («One that ended with the dead daemon before its effect was done does not run again
either») и contracts 6.4 («the result of an exec command stand, … the command do not run again», «a finished step
never runs again») обещают обратное; строка 24 §15 — «ни один шаг … не исполнен дважды».

## Свидетельства

- Журнал: у `run_3` одна попытка — seq 42 `running` (10:29:58.580), seq 44–46 `finishing` → `failed`, `exit 3`
  (58.584). Второй попытки, `attempt_failed`, повторной выдачи бида дрейном нет: детей три (`run_2`, `run_3`,
  `run_4`), `run.queued` на `a-ty5` один (seq 36).
- `state.db`, таблица `attempts`: у `run_3` одна строка (`n = 1`, `exec`, `do`, outcome `exit 3`). Две строки в
  `work.log` при одной попытке даёт только повторное исполнение эффекта.
- Время: `failed` зафиксирован в 58.584, восстановление следующего демона (seq 51 `daemon.started`, pid 390458) —
  в 58.618, то есть `kill -9` пришёл не позже чем через ~30 мс после фиксации (в это время входит запуск процесса).
  Тест сам целится в это окно: `until_failed` (`c24_drain.rs:125`) ждёт `failed` у F и сразу делает `restart()`.
- Порядок в `work.log`: второй `a-ty5` стоит до `w-r13`. Оставшийся `exec:run_3` — самый старый эффект, новый демон
  запускает его на первом же витке после восстановления (58.618); новая попытка A начата в 58.625 (seq 52) и
  дописала `w-r13` к 58.630 (seq 54).
- Исключено: повтор эффекта с задержкой 500 мс в том же демоне (вторая строка легла бы после `w-r13`; в
  `daemon.log` нет «an effect panicked»), `runaway_turn`/`stall`/политика (нет `attempt_failed` у `run_3`).

Один и в живом прогоне проходит, потому что окно — доли миллисекунды–единицы миллисекунд, а тест опрашивает раз в
50 мс; под нагрузкой (все строки разом, запись журнала, голодание демона по CPU) окно растёт.

## Правка (минимальная)

`crates/aae-core/src/run/recover.rs`: чистку `exec:<id>` вынести из цикла по `caught` и выполнять до него для всех
ключей `exec:%`:

- выбрать `key` и `COALESCE(json_extract(payload, '$.marker'), json_extract(payload, '$.instance'))` из `effects`
  `WHERE key LIKE 'exec:%' ORDER BY id`;
- `DELETE FROM effects WHERE key LIKE 'exec:%'`;
- на каждый маркер — `tx.effect(key, "sweep:{marker}", {"op":"sweep","instance":marker})`;
- строки 63–81 из цикла убрать (логика та же, только без привязки к `caught`; объём кода не растёт).

Почему это безопасно: эффект `exec` создаётся в той же транзакции, что переводит прогон в `running` (`start_exec`)
или запускает проверку в `finishing` (`check`) — оба состояния в `caught`, где эффект удаляется и сейчас. Значит, у
прогона вне `caught` любой `exec:<id>` — устаревший: результат команды уже принят или отброшен. Подметания стоят в
очереди раньше новых попыток, как и раньше. `COALESCE` с `$.instance` заодно сохраняет подметание, не успевшее
выполниться до второго подряд падения (сейчас у `caught`-прогонов оно теряется: у `sweep` нет поля `marker`).

Попутно закрывается тот же класс: прогон отменён во время команды, демон убит до `ExecDone` — прогон `cancelled`,
эффект остаётся, новый демон исполнил бы команду отменённого прогона целиком.

Тест-регрессия (детерминированный, рядом с `a_restart_kills_the_command_of_an_exec_step_the_dead_daemon_left` в
`crates/aae-testkit/tests/core/recovery.rs`): exec-шаг `echo ran >> runs.log; exit 3`, дождаться `run.failed`,
`w.killer().abort()`, вставить в `effects` строку `exec:<run>` (dedupe `1`, payload с `op: exec`, той же командой,
`cwd` — workdir прогона, `marker`, `next_at_ms = 0`), `w.restart()`, подождать и проверить `runs.log == "ran\n"`.
Без правки — две строки.

Сценарий `c24_drain.rs` менять не нужно: он проверяет ровно то, что обещает спека.

## Не проверено

- Правку не применял и тест не писал (код не трогаю); воспроизведение под нагрузкой не гонял — вывод из журнала,
  `state.db` и чтения кода.
- Имена столбцов `effects` для вставки в тесте: `id, key, dedupe, payload, attempts, next_at_ms`.
- Запись в `.loop/JOURNAL.md` (НАХОДКА: дефект восстановления, contracts 6.4) — за кодером.
