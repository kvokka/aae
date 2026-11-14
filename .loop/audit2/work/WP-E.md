# WP-E — честность приёмки, CLI и демон, хвосты
STATUS: done (кроме пунктов «не сделано» ниже)

Журнал: две записи РЕШЕНИЕ 23:50, 23:55 (аудит-2); `.loop/JOURNAL.md` не закоммичен — его правят все пакеты.

## По шагам поручения

1. **Диагностика приёмки** — `815f687`. Дерево демона упавшего сценария (`daemon.log`, журнал, `state.db`)
   не удаляется, путь печатается в строке отчёта («daemon.log and journal kept in /tmp/aae-acceptance-…»);
   прошедшего — удаляется. В CI — `actions/upload-artifact@v7` при `failure()`. Тест
   `a_failed_scenario_keeps_its_tree_and_names_it` (main.rs).
2. **Критерий 8, сироты (E2 а)** — `4d04d63`. Шаг `work` — скрипт: первая попытка держит `sleep 60`, повтор после
   краха — выход 0 (файл-флаг `held-$AAE_RUN`); убийство — после появления флага. Мутация (шаг через
   `setsid env -u AAE_INSTANCE sleep 60`, т. е. недостижим для sweep) → `FAIL … orphans of the killed daemon`;
   без мутации — pass, сценарий 13 с вместо 51 с. E2 (б) и (в) — не делал (ход настоящего харнесса в CI-строке 8,
   `passed > 0` живой части).
3. **Критерий 22 (E3, A4)** — `48d8433`, `7942f5e`, `caa7321`. По букве §14 и §2.1 п. 16: в `READINESS.md`
   столбец `Criteria`; открытый критерий (FAIL; в полном прогоне — not run или отфильтрованная строка) = потолок 6;
   10 — только при зелёном 20 в этом прогоне; полный прогон требует у всех ≥ 8 (буква кр. 22), CI-прогон проверяет
   потолки (строки с моделью CI судить не может). Свежесть: `ver=` маркера — коммит не раньше последнего изменения
   `crates/` (`git merge-base --is-ancestor`); в CI `fetch-depth: 0` для обоих заданий. `budget.sh` больше не
   требует ≥ 8 (это и делало проверку круговой), проверяет 0–10 по столбцу `Score`; самотест обновлён. Тесты:
   `readiness_may_not_claim_more_than_the_open_rows_allow`, `readiness_is_stale_before_the_last_change_of_the_code`;
   вручную: `ver=0000000` → FAIL «re-assess», `Recovery 8` в полном режиме → FAIL «at most 6».
   **Баллы на `7942f5e`**: 11 компонентов — 6 (их критерии 2, 4a, живые 8 и 17, сборки 1 на этом коде не шли;
   полного прогона не было с `5c5014f`), 8 — триггеры, ворота, конфиг, реестр, br, хуки, `worktree`, общий MCP.
   **Важно мастеру**: после любого коммита в `crates/` после `7942f5e` (WP-C ещё partial) строка 22 станет FAIL
   «re-assess» — обновить `ver=` (и дату) в маркере `docs/READINESS.md` на последний коммит кода.
   Сопоставление «компонент → критерии» — моё (журнал 23:50), владелец может поправить.
4. Демон и CLI:
   - **G3** относительный `--config` → абсолютный один раз в `Paths::resolve`; **G6** явный `--config` без файла —
     ошибка (`config check` rc 1, демон не стартует, `doctor` → `config_error`) — `0948198`, тест в
     `tests/cli.rs` (краснел без правки). Файл по умолчанию может отсутствовать, как раньше.
   - **G5** SIGTERM посреди exec — `e638021`: при выходе (сигнал, дренаж, рестарт) сначала `abort` задачи ядра,
     затем `orphans::kill_descendants()` (новая функция в `aae-core/src/orphans.rs`: все потомки демона, он же
     subreaper). Паника «an effect panicked… cancelled» ушла: её давала отмена эффектов рантаймом под живым
     писателем. Тест `tests/stop.rs` — обе мутации краснеют (без убийства — процессы живы; без `abort` — паника
     в логе).
   - **G4** `run show` — `8da39ca`: в `RunView` поля `reason` (столбец `runs.reason`, есть с V1: текст вето,
     причина отмены, `pass`), `request` (открытый запрос в `requires_action`), `gate` (старейшее закрытие ключей
     прогона, если ждёт ворот или `parked`); OpenAPI и гайд (`8d3e053`: id запроса из `run show`, а не grep
     событий). `RunHandler::keys` теперь берёт `&Connection`. Тесты: `tests/cli.rs` (gate, reason pass/cancel),
     `aae-testkit/tests/requests.rs` (request).
   - **G7** `plugin list` без `--project` — оркестратор (`project: null`) + каждый проект — `8de2192`.
   - **G8** `stopping` в статусе с начала дренажа — `5bcf8e4` (поле `Api.stopping: OnceLock`); install.md:
     `aae drain` возвращается сразу.
   - **G9** входы — `7a02cb8` (`--input` без `=` — ошибка clap, rc 2) и `1dee5b0` (ядро: `null` = нет значения,
     текст integer/boolean читается по типу, `string` требует строку; CLI шлёт текст, JSON — только объект/массив,
     так `version=1.10` не станет числом 1.1). Тест `aae-testkit/tests/exec.rs` обновлён.
5. **Мост без таймаута (A8)** — `be1a0c1`: `aae mcp-bridge --timeout <s>` (600 по умолчанию), по сроку — JSON-RPC
   ошибка клиенту, запись `pending` удаляется, серверу `notifications/cancelled`. Тест
   `a_call_the_server_does_not_answer_in_time_is_an_error`.
6. **OpenAPI против факта (E12)** — `8e1d447`: 401 у каждой операции (тест в `openapi.rs` краснел); `/v1/doctor`
   без 500; 400 у `reload` называет `config_error`; новый тест `the_errors_answered_are_declared_with_their_codes`
   (7 запросов к живому демону: статус ответа объявлен у операции и его описание называет код; краснел без
   правки `reload`). Отпечаток идемпотентности включает query: повтор `drain` с другим `timeout_s` под тем же
   ключом — 422 (мутация краснеет). Ошибка чтения журнала посреди потока — событие `journal`, не `gap`
   (`events.rs`, `live.rs`; без теста — ошибку ввода-вывода посреди потока дёшево не вызвать). `frontends.md`
   про 410 до старта потока исправлен.
7. **Живой просмотр** — `d583894`: таймер 500 мс убран. Каждая запись журнала уже будит просмотр; сессия
   становится доступной для отвода до события `starting → running` (первый промпт подтверждён), так что нужное
   событие есть. Тесты claude live/watch зелёные 3 раза подряд.
8. Хвосты:
   - JSON без `--json` (G10, Г) — `1526974`: ответ печатается строками `key: value`, вложенные ключи через точку,
     `null` опускаются, пустой список — ничего; строки событий без `null`, со `from -> to` и `reason=`;
     `run list --state bogus` → 400 (`STATES` в `api/runs.rs`). Тест — `tests/cli.rs`.
   - `install.md`: `GET /v1/doctor`, поведение `drain`/сигнала — `cf7f73a`.
   - Старый `lock` (ревью 61) — `a9312d7`: старт удаляет `$XDG_STATE_HOME/aae/lock`; тест в `stop.rs`.
   - Замеры (E9): версия в отчёте и маркерах приёмки теперь `aae 0.1.0 <git describe --always --dirty>` —
     `cf7f73a`. В `README.md` замер с маркером один (`bv-workspace-read`) — правка не нужна. Места E9 в
     `docs/design/*`, `docs/guide/development.md` — не моя зона (ниже).
   - Логи (E10): `git rm --cached` 24 лога `notes/*.log` (`3be62e6`) и `.loop/cycles/43/ci.log` с правилом
     `.loop/cycles/*/*.log` в `.gitignore` (`070d778`, `e995893`); файлы на диске остались.
   - G15/G14 — абзац в `first-formula.md` (сбой рига закрывает `rig:<p>/<r>`, `aae gate list`/`gate open`) —
     `39ff4ed`.
   - Строки > 120: `doctor.rs:1`, `contracts.md` — чужие, не трогал.
9. **Красный CI, критерий 8**: WP-A прогонял 8+8 раз под нагрузкой на старом сценарии — 16/16 pass (их отчёт).
   Я прогнал **новый** сценарий (держатель 60 с) 10 раз под 8 × `yes` (9 ядер) на `1dee5b0`: **10/10 pass**,
   11,5–13,8 с, рестарт до ответа ≤ 0,04 с. Причина падения на `9e6c3eb` не установлена; при следующем падении в
   CI артефакт `acceptance-failures` даст `daemon.log` и журнал.

## Изменённые файлы

`crates/aae-acceptance/src/{main,daemon,checks}.rs`, `scenarios/c08_crash.rs`; `crates/aae/src/{main,daemon,
bridge}.rs`; `crates/aae/tests/{cli,api,replies,bridge,stop}.rs` (stop.rs новый); `crates/aae-front/src/api/{mod,
config,daemon,events,live,replies,runs}.rs`, `cli/{mod,commands}.rs`, `tests/openapi.rs`; вне зоны, с
разрешения поручения/после WP-A: `crates/aae-core/src/orphans.rs` (новая `kill_descendants`),
`crates/aae-core/src/run/{read,gates,machine,queue}.rs`, `crates/aae-testkit/tests/{requests,exec}.rs`;
`docs/api/{openapi.json,frontends.md}`, `docs/guide/{install,first-formula}.md`, `docs/READINESS.md`;
`scripts/{budget,budget-selftest}.sh`; `.github/workflows/ci.yml`; `.gitignore`.

## Нужны правки чужих файлов и спеки

- `crates/aae/src/doctor.rs` (WP-B): при отсутствующем файле по умолчанию `doctor` пишет `ok config <path>` —
  должно быть «no config file, the defaults» (G6, вторая половина).
- `docs/design/contracts.md` §1.3 RunView: добавить `reason`, `request`, `gate`; §6.4/инварианты: остановка
  демона (сигнал/дренаж/рестарт) убивает всех его потомков до выхода; `docs/design/invariants.md` «Recovery» —
  то же. `contracts` 1.3/фронтенды: `--input` — текст по типу входа.
- E9 в `docs/design/event-model.md:55`, `docs/guide/development.md:13`, `docs/design/invariants.md:69-72` —
  дописать дату/sha/маркер (WP-D).
- Спека §15 кр. 22: формулировка «у каждого компонента балл не ниже 8» выполнима только полным прогоном —
  стоит прямо написать, что CI проверяет потолки и свежесть, а «≥ 8» — полный прогон.

## Не сделано / осталось (по приоритету)

1. Полный `mise run acceptance` на текущем коде (живые модели) — по команде владельца; после него поднять баллы.
2. E2 (б): CI-вариант критерия 8 с ходом настоящего claude/codex на поддельном провайдере; E2 (в) — живая часть
   требует только `passed > 0`.
3. G11 (404 против 409 у отсутствующих объектов, дубль «rejected» в тексте, лишние поля тела принимаются,
   ворота несуществующих объектов закрываются молча), G13 (одна ошибка на TOML-файл), G15-мелочи
   (`config_generation`, `workdir …/.`, `next_at` у выключенного триггера, `events --from` за головой).
4. G12 `BEADS_DIR` у проектов без `.beads` — `harness.rs`, зона WP-B.
5. Таймаут моста 600 с — константа по умолчанию; конфигурировать из конфига сервиса не стал (не просили).
