# Цикл 46 — coder
STATUS: done

## Взято в работу
1. Обяз. 1 ревью 45 — флейк c05b в GitHub CI, просмотр остальных сценариев `acceptance-ci` на окна времени.
2. Обяз. 2 ревью 45 — гайд «cron в UTC».
3. Обяз. 3 ревью 45 = WP-48 (граница ядра §2.1 п. 6, мёртвые способности А10, `instance_idle_s = 0`, unit-тест).
4. Попутно: жел. 5 (MEASURE со ссылкой на незакоммиченные логи) и жел. 6 (READINESS про «повышение» WP-40).

## Сделано (коммиты)
- `03be508` WP-41: c05b и c10 проверяют времена из журнала демона, а не окна настенных часов теста.
  - c05b: первое срабатывание ≥ `daemon.started` + интервал; срабатывания не чаще интервала; после рестарта
    догоняющее < интервала от второго `daemon.started`, следующее — не раньше чем через интервал. Корень флейка
    оказался двойным: окно времени и SIGKILL посреди прогона `tick` → на старте прогон повторяется, догоняющее
    срабатывание законно `trigger.skipped(overlap)` (видно в отладочном прогоне). Теперь убийство после двух
    `run.passed`, `skipped` считается срабатыванием.
  - c10: вместо `sleep(RESET/2)` + «ещё не прошло 10 с» — из журнала: `gate.opened` ≥ `until`; второй прогон
    держал шлюз аккаунта (`waiting_for`/`gate.key`); ни одного перехода второго прогона в `starting` до
    `gate.opened`; первый прогон — ровно два запуска, второй после открытия.
  - Остальные сценарии просмотрены: c07 (замер), c08:291 и c13:39 — отрицательные ожидания (ложного падения не дают),
    c16 — опрос. Окно с утверждением было только в c10.
  - `docs/guide/first-project.md`: cron — в системном поясе или в `timezone`, ссылка на `config.md#trigger`.
- `9025846` WP-48: ядро без имён харнессов и br. `Harness::options` (flatten toml) вместо `max_sessions`/
  `instance_idle_s`; проверка ключей — в `crates/aae/src/daemon.rs::load` (claude — ни одного, codex — serde с
  `deny_unknown_fields`); `Project::tracker: TrackerLink { present, env, actor }` заполняет бинарник (`BEADS_DIR`,
  `.beads`, `BD_ACTOR`); `exec_step` берёт переменную актора из `TrackerLink`. `scripts/core-boundary.sh`
  (поиск `claude|codex|br|max_sessions|instance_idle_s|BEADS_DIR|.beads` вне тестов) — в `mise run ci`. Тесты:
  core — опции проходят непрочитанными, переменные трекера под всеми слоями; `aae/tests/cli.rs` — `config check`
  отвергает `max_sessions` у claude, `max_sessions = 0` и неизвестный ключ у codex, принимает корректные.
- `10178e2` WP-48: удалены `Capabilities::retries_429`, `reset_time`, `Normalized::SessionStarted` (никто не читал);
  `contracts.md` 4.3/4.5 поправлены.
- `ead7c22` WP-48: unit-тест `daemon::tests::the_codex_settings_reach_the_driver_capabilities` (дефолты 8/60 с;
  3/0 доходят до `capabilities()`); `instance_idle_s = 0` допустим — в `config.md`.
- `0a4f5d3` READINESS: убрано «escalation no later than the attempts ceiling» (жел. 6), отмечен WP-48.
- `bf63283` development.md: `src=` у MEASURE — команды, а не незакоммиченные логи; маркер у «14–17 s» (жел. 5).

Проверки: clippy `-D warnings` чисто; nextest 292/292 (после каждого шага); под нагрузкой — `acceptance-ci` 15/15
за 59 с параллельно с полным nextest 291/291 (до WP-48); после WP-48 — сценарии 3, 5a, 13, 16 зелёные;
финальный `mise run ci` на `bf63283` — EXIT 0, затем `acceptance-ci` — 15/15 за 55 с, EXIT 0
(логи `notes/ci.log`, `notes/acceptance-ci-final.log`). deny/machete/unused-pub/budget ok (всего 13120/30000).

## Не получилось и почему
- GitHub CI на запушенном HEAD не проверить: пушит цикл после ревью. WP-41a–d, 43, 44a, 49 остаются doing до
  зелёного CI.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего в рабочем дереве. Следующий пакет по ревью — WP-42 (M, с планом эксперта). Жел. 1–4 ревью 45 не тронуты.

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — исправлено локально (03be508), закрытие — зелёный GitHub CI после пуша.
- Обяз. 2 — закрыто (03be508); `grep -rn -i utc docs/guide README.md` — пусто.
- Обяз. 3 — закрыто WP-48 (9025846, 10178e2, ead7c22), CI-поиск `scripts/core-boundary.sh`.
- Жел. 5, 6 — закрыты (bf63283, 0a4f5d3). Жел. 7 — внесён строкой в WP-44 плана. Жел. 1 (auth.rs на настенных часах), 2 (`auth_backoff_base_s = 0`),
  3 (tzdb), 4 (осеннее наложение `*/15`), 8 (большие тестовые файлы, `config reload`
  без списка ошибок) — открыты.

## Контекст на момент отчёта: ~180k
