# WP-18 — план кодера (expert недоступен в этой сессии: агента `expert` нет в списке типов)

Разбиение: WP-18a (этот цикл) — реестр экземпляров в состоянии; WP-18b — учёт потомков (`AAE_INSTANCE`, subreaper,
проверка `/proc/*/environ`), проверка здоровья экземпляров; сервисы §11.4 — в WP-34 (там уже «сервисы в реестре»).

## WP-18a
- Схема v6: `instances(id, kind, harness, fingerprint, process, handle, state starting|ready|stopping, sessions,
  capacity, idle_ms, idle_since_ms)`. Запись — намерение до эффекта (инв. 9).
- `driver::Capabilities.instance_idle` (claude 0, codex 60 с); `start_instance(id, process, sink)` — id выдаёт
  реестр, адаптер кладёт его в `handle.id` и в `AAE_INSTANCE` (contracts 4.1, 8).
- Захват в `start_attempt` (та же транзакция): живой `starting|ready` с тем же отпечатком и `sessions < capacity`,
  иначе новая запись `starting` + `instance.starting`. `attempts.instance` — сразу.
- Эффект `launch`: ждёт смерти экземпляров `after` (прошлые экземпляры прогона в `stopping`) по pid+start_time
  (инв. 6), затем общий для всех ждущих старт (`OnceCell` по id) → `InstanceReady|InstanceFailed`.
- Освобождение в `end_attempt`: `sessions − 1`; при 0 — `idle_ms == 0` → `stopping` + эффект `stop_instance`,
  иначе дедлайн `<id>:idle`. `stop_instance` → `InstanceStopped` → строка удаляется, `instance.stopped`.
- События драйвера: по сессии среди живых попыток экземпляра; без сессии — `failure`/`exited` всем попыткам;
  `exited` вне `stopping` — `instance.failed`, запись снимается.
- Тесты (testkit, сценарный драйвер): один отпечаток — один экземпляр, разный — два; предел сессий — второй;
  простой → остановка по дедлайну на управляемых часах; падение общего экземпляра роняет все его попытки;
  `alive`/`wait_dead` на настоящем процессе. Критерий 13 (ядро).
