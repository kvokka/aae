# Цикл 27 — coder
STATUS: done

## Взято в работу
- Жел. 1–5 ревью 26.
- WP-22b разбит на WP-22b1 (конфиг через API, перечитывание, перечень триггеров, сценарий 12) и WP-22c (демон: состояние,
  дренаж, рестарт, живой просмотр, вкл/выкл триггера, экземпляры, TCP). Взят и закончен WP-22b1.

## Сделано (коммиты)
- `a1289fa` — жел. 1–4 ревью 26: `waiting_for == "gate"` в сц. 10; демон-тест kill -9 codex ждёт `sleep 20` в `/proc`
  вместо таймера; `max_sessions` у не-codex харнесса — ошибка конфига с файлом (тест); хвосты READINESS переписаны.
- `4f42d1a` — WP-22b1, ядро: `Msg::Reload(Arc<Config>)` (`run/reload.rs`): отказ при смене набора харнессов или проектов
  с трекером; перевзвод расписаний с изменённым `cron`/`every`; простаивающие экземпляры останавливаются с reason
  `config`; `config.reloaded{config_generation, restarted_instances}`. `Reader::triggers(&Config)` с `next_at` из
  `deadlines`; `Cron` хранит исходный текст. Тест `aae-testkit/tests/triggers.rs::a_reload_rearms_...`.
- `9a53967` — WP-22b1, фронт: `GET /v1/projects|agents|formulas|packs|plugins` (`?project=`, 404 на неизвестный),
  `GET /v1/config/explain`, `POST /v1/config/check`, `POST /v1/config/reload` (ошибки файла — 400 со списком,
  нужен рестарт — 409), `GET /v1/triggers`. `Api.config: RwLock<Arc<Config>>`, `Api.load` — общая со стартом загрузка с
  `aae_plugins::check` (`daemon.rs::load`; закрывает ревью 23 жел. 5 и ревью 18 жел. 3). CLI: `aae project|agent|formula|
  pack|plugin list`, `aae config check|explain|reload`, `aae trigger list`; OpenAPI обновлён; тест на демоне
  `aae/tests/cli.rs::the_config_from_the_cli_lists_explains_checks_and_reloads`; `docs/design/config.md`.
- `b294af6` — адаптер claude: без слоя хоста в `--settings` ставится `claudeMdExcludes` (`~/.claude/CLAUDE.md`,
  `~/.claude/rules/**`): сценарий 12 показал, что `CLAUDE.md` хоста попадал в изолированную сессию через обход предков
  рига под `HOME`. Закрыт открытый вопрос в `docs/design/experiments.md`.
- `3b29374` — сценарий acceptance 12 (`c12_explain.rs`, CI): «объясни» через API и CLI совпадают, у каждого значения
  слой, одна строка хоста; на подставном HOME наследующая сессия видит `CLAUDE.md` хоста, изолированная — нет.
  Прогон: `aae-acceptance --ci 12` — pass; `--ci 4` — pass (регрессия после изменения settings).
- `3f35339` — READINESS: сборка конфига 3 → 6, триггеры, API, CLI, acceptance; маркер `ver=3b29374`.
- Проверено: clippy `-D warnings` чисто; nextest 231/231; бюджеты: ядро 6979/12000, фронтенды 1450/4000, всего 11041.
  Полный `mise run acceptance-ci` не гонял (только сц. 4 и 12).

## Не получилось и почему
- Субагенты недоступны (Agent видит только coder/reviewer; хук запрещает coder→coder) — всё сделано в одном
  контексте, поэтому WP-22b разбит. Запись в журнале.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет. Следующий: WP-22c (состояние демона, дренаж/рестарт со снятием `shortcut:` в `crates/aae/src/daemon.rs`, живой
  просмотр, вкл/выкл триггера, экземпляры, TCP+токен) или WP-32 (критерий 15) — по ревью.
- Остановка простаивающих экземпляров при перечитывании тестом не покрыта (в тесте экземпляров нет).

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было. Жел. 1–4 закрыты в `a1289fa`; жел. 5 — дописан в WP-39 (план). Жел. 6 — остаётся в WP-32/WP-38.
- Попутно: ревью 18 жел. 3 (`arm_schedules` при перечитывании) и ревью 23 жел. 5 (проверка параметров встроенных
  плагинов при перечитывании) — закрыты в WP-22b1.

## Контекст на момент отчёта: ~205k
