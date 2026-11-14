# WP-F — добивка после пяти пакетов и полный прогон
STATUS: done

## Шаг 1. Состояние дерева
- Изменён был только `.loop/JOURNAL.md`. clippy всего workspace — чисто; nextest — 385/385 зелёные.
- Найдено на шве: `scripts/core-boundary.sh` уже краснел на HEAD `caa7321` (комментарий в
  `aae-core/src/config/files.rs:112` называл claude/codex) — исправлено в `128299f`.

## Шаг 2. Два бага WP-A — `8941c8d`
- `deciding()` (`run/decide.rs`): дедлайн теперь = MAX(now, MIN(next_at)) + сумма таймаутов хуков, уже стоящих в
  `hooks:<run>`, + свои + 5 с (та же формула, что у `rearm` после рестарта). Оценка сверху: если решение уже пришло,
  а эффект ещё не снят, дедлайн выйдет позже нужного — это безопасная сторона.
- `attempt_event()` (`run/events.rs`): прогон с `waiting` не взводит `silence` поздними событиями сессии.
- Тесты `crates/aae-testkit/tests/deciding.rs`: `a_deciding_event_waits_for_the_slow_hook_of_the_run_queued_ahead`,
  `late_session_events_do_not_stall_a_run_waiting_for_a_decision` — оба краснеют без правки (проверено откатом каждой
  правки отдельно). contracts 3.3 дописан.

## Шаг 3. Правки, оставленные пакетами
- WP-C: `bead.ready` не добавляет глубину — contracts 6.x (`cecc7db`); строка про `--plugin-dir` в
  `docs/guide/plugin.md` (`cecc7db`).
- D 8 (`4b31a88`): встроенный `worktree` запускает `git` в своей группе процессов и убивает группу по таймауту хука
  (30 с; настройка не добавлена — существующей таблицы параметров для этого нет). Тест
  `aae-plugins::tests::a_git_past_the_hook_timeout_is_killed` (post-checkout `sleep 20`): без правки 20 с и Ok —
  красный. В `cfg(test)` таймаут 2 с. Комментарий в `run/hooks.rs::bundled` и `config.md` обновлены.
- D 9 (`94d24e3`): отказ наблюдателя трекера → `tracker.sync_failed{op: "watch"}` (через `Msg::Tracked` с
  `TrackerError::Invalid`, без повтора; строка в stderr осталась). Тест `crates/aae/tests/tracker.rs::
  a_tracker_that_cannot_be_watched_says_so_in_an_event` (`.beads` с правами 000) — без правки красный.
  `tracker-br.md` дописан. Строка в `doctor`/`status` — не делал.
- WP-B (`128299f`): `scripts/core-boundary.sh` — `grep -i`, имена харнессов из таблицы `ADAPTERS`
  в `crates/aae/src/harness.rs`. Этот же скрипт был красным на HEAD до меня (комментарий в `config/files.rs`) —
  комментарий переписан. `"codex"` в `daemon_tests.rs` — поиск адаптера по имени в тесте корня, оставлено.
- WP-E: `doctor` без файла конфига пишет `no config file at <path>, the defaults` (`e6637d3`, тест
  `tests/doctor.rs::with_no_config_file_the_defaults_are_named`, без правки красный); contracts 1.3 — поля
  `reason`/`request`/`gate` (и прочие) вида чтения; 6.4 — остановка демона убивает всех потомков; `--input` как
  текст по типу — `first-formula.md` и `config.md` (`cecc7db`).
- WP-D: `invariants.md` замер с видимой датой; contracts 5.1 «run id is never reused» → «не повторяется в пределах
  одного каталога состояния (счётчик)»; ссылки `§5.4`/`§5.3` в contracts 3.3/3.5 → 1.3 / 1.1 и event-model
  (`cecc7db`). Инвариант 16 уже был исправлен до меня.
- Строки > 120: `crates/` — 55 строк (helper), `docs/` проза — переразбиты абзацы (`de1a61d`, `aeb9a11`). Остались
  длинные строки только в таблицах и в отступленных `<!-- MEASURE -->`.
- Файлов > 400 строк в `crates/` нет (максимум 399: `aae-core/src/process.rs`, `aae/tests/api.rs`).

## Шаг 4. §11.3 спеки сжата (`010cb1b`)
Сверено с оригиналом; в contracts 3.1 дописано «хук без состояния, хранилища для плагинов нет, нужное — в событии».

## Шаг 5. READINESS
Статусы критериев мои правки не меняют — только маркер: `ver=3af4457` (последний коммит в `crates/`), коммит
`feba8b2`.

## Шаг 6. Полный прогон на `feba8b2`
- `mise run ci`: `Summary [  36.691s] 390 tests run: 390 passed, 0 skipped`; deny, machete, unused-pub,
  core-boundary, budget-selftest, budget — ok (`budget: total 15284 / 30000`). rc=0.
- `mise run acceptance-ci`: `21 of 30 rows passed, 0 failed, 9 not run, in 60 s.` rc=0.
- Красное по дороге и починено: `cargo machete` — неиспользуемый `serde` в `crates/aae` (`350ceef`); приёмка
  кр. 11 и 17 ждала 35 и 6 тестов, пакеты добавили по одному (`3af4457`: 36 и 7); кр. 22 краснел как следствие кр. 11.

## Не сделано / что осталось
- D 9: строка об отказе наблюдателя в `doctor`/`status` (событие есть).
- D 8: убивается только группа `git`; потомок, ушедший из группы, доживёт до остановки демона (её зачистка — G5).
- Предложения других пакетов вне этого поручения: B 8 (риг из метки бида), D 7 остаток, G11–G15 (WP-E), G12
  `BEADS_DIR`, правки спеки из отчётов WP-A/WP-B/WP-C (§7.1, §9.5) — не брались.
- Полный `mise run acceptance` с живыми моделями — не запускался (не мой); баллы READINESS поднимаются после него.
