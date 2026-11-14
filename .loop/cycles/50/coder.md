# Цикл 50 — coder
STATUS: done

## Взято в работу
Обяз. 1–4 ревью 49 (флейки, мутации N2 и L1), жел. 3 и 4; затем WP-50.

## Сделано (коммиты)
- `bdd72e3` WP-42b: тесты без гонок виртуального времени и портов.
  - Новый шаг `Step::Await(Arc<Notify>)` в `aae-testkit`: смерть/сбой по сигналу теста, а не по `Wait(5s)`.
    `instances.rs::a_shared_instance_that_dies…` — смерть после запуска сироты (обяз. 1);
    `old_instances.rs::a_new_attempt_on_a_shared_instance…` — сбой после сироты и после `running` второй сессии
    (тот же шаблон; раньше сирота могла появиться уже после того, что тест проверяет, — ложнозелёный).
  - `services.rs`: причина флейка установлена — не время, а гонка порта (`Address already in use`, событие
    `service.failed` → рестарт; 2/100 под `yes`). Тесты считают сервисы по id (`World::services`), не старты (обяз. 2).
- `656f393` WP-42b: `wait_gone` подтверждает смерть по pid.
  - `orphans::sweep{,_all}` возвращают убитые (pid, start time); `wait_gone` ждёт, пока каждый — зомби или исчез.
  - Unit-тест `wait_gone_returns_once_what_it_killed_is_dead` (dd 256 МБ). Обяз. 4 — см. журнал 09:41: на этом
    ядре мутация выживает (процесс уже `Z` к концу обхода), на обычном ядре окно есть.
  - `old_instances.rs::a_new_attempt_starts_only_after…` — смерть проверяется через `start_time` (флейк: `Z` в
    `/proc` раньше, чем `waitpid` его отдаёт).
  - Новый тест `the_resync_leaves_the_processes_of_a_live_instance_alone`: N2 у меня красная, откат — зелёный (обяз. 3).
- `docs`: contracts 8 (смерть ждёт убитых по pid), plugin.md (`${AAE_PLUGIN_DIR}` в `env`/`health.command`, жел. 3).
- `2d44e11` WP-50 (А6, А7): `native.<harness>` сливается по ключу (таблица над таблицей; не-таблица заменяет);
  новый `Layer::Merged(Vec<Layer>)` — значение, собранное несколькими слоями: `native`, `system_prompt` с
  фрагментами плагинов, `PATH` с `bin/` плагинов; `explain` пишет `# agent + plugin mail`. Тесты
  `native_tables_merge_by_key_across_the_layers`, обновлён `an_attached_plugin_puts_its_bin…`; доки config.md.
- `dca6ec7` WP-50 (А8, через subcoder, проверено мной): `aae config check|explain` без демона — грузят файл так же,
  как демон (`--config`, как у `doctor`/`fetch`), ответы строят те же функции, что и API (`api::config::{checked,
  explained}`); ошибка API с массивом `errors` печатает их построчно (отказанный `reload`). Тест
  `crates/aae/tests/cli.rs::the_config_is_checked_and_explained_without_a_daemon`; доки frontends.md, config.md,
  first-project.md, READINESS.
- `0849cc5` WP-42b: хелпер `retry` в `old_instances.rs` ждёт `starting, running`, а не фиксированные 200 мс.
- Проверка (итог): clippy чисто; полный nextest 318/318; budget/core-boundary/unused-pub чисто; old_instances под
  8× `yes` — 299/300 (одно падение — известный флейк «resync at the start», журнал 09:42, 10:20).
- Проверка (до WP-50): clippy чисто; полный nextest 316/316; под 8× `yes`: services-тест 100/100, instances 60/60,
  old_instances (весь бинарник) 100/100 (+600/600 до добавления N2-теста).

## Не получилось и почему
- Жел. 2 (один `sweep_all` на всё `after` в `launch`) — не делал: `after` обычно 1–2 записи, выигрыш мал.
- Не прогонял полный `mise run ci` и `acceptance-ci` — только шаги по отдельности (fmt, clippy, nextest, budget,
  core-boundary, unused-pub).
- Редкий флейк «resync at the start» в `old_instances.rs` (2 из ~1500, не тайминг) — причина не найдена, журнал 09:42.

## Начато и не закончено (файлы, точка остановки, следующий шаг)

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1, 2, 3 — закрыты; обяз. 4 — НАХОДКА + изменение продукта + тест (журнал 09:41); обяз. 5 — ждёт владельца.
- Жел. 1 — РЕШЕНИЕ в журнале 09:43; жел. 3 — сделано; жел. 4 — статусы в плане; жел. 2, 5, 6 — нет.

## Контекст на момент отчёта: ~210k
