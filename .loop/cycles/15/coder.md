# Цикл 15 — coder
STATUS: done

## Взято в работу
1. Обязательный п. 1 ревью 14 — зачистка потомков при падении экземпляра.
2. Желательные ревью 14: 1 (оговорка про nextest), 2 (`OWN` при брошенной future), 4 (текст WP-18b, READINESS).
3. WP-27 — разбит на WP-27a (сделан) и WP-27b (todo) в плане.

## Сделано (коммиты)
- `cf73b0a` WP-18b: `instance_failed` ставит эффект `sweep` (намерение до эффекта); исполнитель обходит `/proc` в
  `spawn_blocking`. Тесты `a_shared_instance_that_dies…` (Exit) и `an_instance_found_dead_by_its_health…` (Vanish)
  запускают настоящий `sleep 300` с `AAE_INSTANCE` и ждут его смерти; без исправления оба красные (проверено
  `git stash`). `process::run` снимает pid из `OWN` guard'ом `Own` в `Drop`. Комментарий «нужен nextest» у теста
  подборщика.
- `561c889` WP-27a: `crates/aae-claude` — `stream.rs` (разбор stream-json, 7 юнит-тестов), `part.rs` (флаги,
  окружение, account), `lib.rs` (`Driver`), `tests/claude.rs` — настоящий claude на поддельном провайдере (ack →
  completed → транскрипт Answered; billing). `orphans::start_time` стал публичным (адаптеру нужен для handle).
  Драйвер подключён в `aae daemon`. Обновлены PLAN (27a/27b, текст WP-18b) и READINESS (claude 3, текст зачистки,
  строка «Assessed»).
- `e05ad60` версия dev-зависимости testkit для `cargo deny`.
- Проверено: fmt, clippy -D warnings, nextest 136/136, cargo deny ok, machete ok, budget: claude 729/3500,
  всего 7425/30000; файлы ≤ 400 (lib.rs 377).

## Не получилось и почему
- Субагенты недоступны (helper/subcoder/expert не зарегистрированы, `coder` запрещён хуком) — всё сделано самим;
  записано в журнал.
- «Готово, когда» исходного WP-27 (reply OK → финишер → pass через ядро) не достигнуто — вынесено в WP-27b.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-27b: доставка MCP/хуков/skills/плагинов/native (сейчас `part.rs` отказывает `config_error`), учётные данные
  в свой `CLAUDE_CONFIG_DIR`, `Capabilities.versions`, бинарник через mise, stderr в `raw`; путь через ядро —
  мир testkit с `Claude` вместо `ScriptedDriver` и `Reply::Bash("aae finish …")`.
- Не проверено: interrupt и permission на настоящем claude через драйвер (разбор покрыт юнитами по форме из опыта 1a).

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательный 1 — закрыт (эффект `sweep` + 2 теста на настоящих процессах). Тексты отчёта 14/READINESS/contracts 8
  теперь правдивы (contracts 8 не менялся: «stopped or has exited» стало верным).
- Желательный 1 — комментарий у теста; 2 — guard `Own`; 4 — WP-18b и READINESS поправлены; 5 — основной путь
  настоящего claude на подделке подтверждён (журнал); 3 и 6 — не трогались (WP-21/27b/20/33).

## Контекст на момент отчёта: ~220k
