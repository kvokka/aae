# Цикл 49 — coder
STATUS: done

## Взято в работу
1. Обяз. 1 ревью 48 — тест области `project` для `[[service]]` и stdio за мостом; жел. 1, 2 ревью 48.
2. WP-42b (А3 инв. 6, А5.2 resync по маркерам) по `.loop/cycles/47/notes/wp42-plan.md`.

## Сделано (коммиты)
- `88f3b7b` WP-47: два проекта, сервис и stdio-сервер с `scope = "project"` → 4 процесса, разные URL; `orchestrator` → 2,
  общие (`aae-testkit/tests/services.rs::a_project_service_runs_once_per_project_an_orchestrator_one_once`).
  Мутации ревью M5 (`McpScope::Project => Orchestrator`) и M12 (`Project => "orchestrator"`) — обе красные, откат.
- `35414da` WP-47: пробник отвечает на `/health` только с токеном. Мутация M6 (здоровье http без `Authorization`) —
  красная (`a_service_with_an_http_check_is_ready_once_it_answers`).
- `07d4ea1` WP-47: `${AAE_PLUGIN_DIR}` подставляется и в `env`, и в `health.command` сервиса плагина; тест в
  `config_services.rs`, `config.md` поправлен.
- `f885dfd` WP-42b (А3): `launch` ждёт всё от старых экземпляров — лидеру даёт дожить, остальное с маркером убивает,
  пока `/proc` не опустеет (`process::wait_gone`); `old_instances` берёт и удалённые записи (`LEFT JOIN`, `i.id IS NULL
  OR i.state = 'stopping'`); `sweep` считает найденные, а не убитые. Тесты: `tests/old_instances.rs` (2) и
  `process::tests::wait_gone_spares_a_live_leader…`. Мутации (все откачены):
  M-a убрать цикл `after` → `a_new_attempt_starts_only_after…` красный; M-b старый SQL → он же красный;
  M-c убрать условие по состоянию → `a_new_attempt_on_a_shared_instance_leaves…` красный; M-d убрать ожидание лидера →
  unit красный; M-e2 `wait_gone` без sweep → unit и `a_new_attempt_starts…` красные. (Мутация «один проход sweep вместо
  цикла» выживает — гонку форка под sweep детерминированно не воспроизвести.)
  Доки: `contracts.md` 8 «Confirmed death», `invariants.md` строка 6 и «How they are checked» (6 не в chaos);
  `chaos/check.rs` метка → invariant 2.
- `595de66` WP-42b (А5.2): `check_instances` (resync каждые 5 мин и теперь при старте после `recover`) кладёт в эффект
  `lost` — экземпляры из `attempts` (живые попытки или закончившиеся в последний час), которых нет в `instances`;
  исполнитель одним обходом `/proc` (`orphans::sweep_all`) убивает процессы с их маркерами. Чужие маркеры не трогаются
  по построению (кандидаты только из своего состояния). Отступление от плана эксперта (обход на каждом resync +
  `Msg::Marked`) — в журнале (РЕШЕНИЕ 16:30): обход на каждом resync поднял chaos 20 → 70–88 с, nextest 29 → 85 с.
  Тест `old_instances.rs::the_resync_sweeps_what_carries_the_marker_of_a_gone_instance` (две фазы + `foreign`).
  Мутации (откачены): N1 исполнитель не обходит → красный (фаза 1); N3 нет `check_instances` в `started()` → красный
  (фаза 2); **N2 убрать `NOT IN (SELECT id FROM instances)` — выживает**: процесс живого записанного экземпляра
  убился бы resync-ом; тест через `sleep(300 s)` не вышел — за 5 мин виртуального времени живой экземпляр гасит сторож
  простоя. Доки: `event-model.md` «Resync», `contracts.md` 8 «Resync», `invariants.md` «Recovery» п. 4.

## Проверки
- `cargo clippy --all-targets -D warnings` чисто; полный nextest 314/314 за 31,4 с (было 29,2 на 309);
  chaos 3 раза подряд 10/10 (19–22 с); `budget.sh`: ядро 8260/12000, всего 13694/30000. `mise run ci` и
  `acceptance-ci` целиком не гонял.

## Не получилось и почему
- WP-50 не брал: после WP-42b осталось меньше бюджета, чем нужно на M.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-42b в плане — «doing — ждёт ревью»; WP-47 → `done` после пуша и зелёного GitHub CI (не моё действие).
- Выжившая мутация N2 (см. выше) — нужен тест «resync не трогает процесс живого экземпляра»: живой экземпляр на
  момент resync без вмешательства сторожа (например, отключить стопор простоя в формуле теста или вызвать resync
  через рестарт-подобный путь).

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто (`88f3b7b`). Обяз. 2 (А5.3) — ждёт владельца, не трогал.
- Жел. 1, 2 — закрыты. Жел. 3–7 — не брал.

## Контекст на момент отчёта: ~200k
