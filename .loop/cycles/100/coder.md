# Цикл 100 — coder
STATUS: done

## Взято в работу

- Разбор застоя с expert (требование мастера).
- WP-81 (`drain`: `pool`, `on_item_failure`, критерий 30) — сам.
- WP-83 (фон после исхода, критерий 34) — subcoder параллельно, если expert подтвердит, что влезает.

## Сделано (коммиты)

- Застой: разбор expert — `notes/stagnation.md`, вывод в журнале (17:30). Причина структурная (ступень шкалы:
  минимум компонентов 6 → SCORE ≤ 7); рычаг — WP-83 параллельно у subcoder, мерило цикла — длина `OPEN`.
- WP-81 (план expert — `notes/wp81-plan.md`):
  - `f87fffb` WP-81: `pool` (умолчание 1) и `on_item_failure` у шага `drain`; остановка выводится из детей входа;
    фикстуры с явным `pool` (SWEEP/AGAIN 8, хаос 2, c24 4); explain; тесты конфига и `core/drain_pool.rs` (6).
  - `03bcc0e` тест пула перечитывает трекер при `queued`-ребёнке (убивает мутацию «живые — только running»).
  - `b16da54` приёмка 30: сценарий `c30_drain.rs` на настоящем br без модели (one/two/soft c kill -9/hard) + строка
    тестов; 30 снят из `OPEN`.
  - `1b3d44e` демо: `pool = 2` в обоих `deliver.toml`, README.
  - `c86e54e` docs: `config.md`, `contracts.md` 6.2, `invariants.md`, гайд.
  - Мутации (все убиты): free==0 не проверяется; живые — только running; stopped=false; hard без отмены; отмена и при
    soft; Fail→Pass; stopped без `mine`; умолчание pool 2; `not_started` пуст; `pool` не в снимке. На сценарии c30
    (с пересборкой `aae`): pool не проверяется, hard без отмены, отмена при soft — все красные.
  - Точечно зелёные: `aae-core`+`aae-testkit`+`aae-front` 372/372; приёмка 24, 30 (×3), 20 (CI-части).
- WP-83 (план expert — `notes/wp83-plan.md`; код — subcoder, отчёт `notes/wp83-done.md`; ревью core-диффа и коммиты — мои):
  - `57d1262` эффект `stop_background` (`Idle{background}` + законченный ход + исход, либо из финишера), метод
    `Driver::stop_background`, claude `stop_task` для `local_bash`, codex `thread/backgroundTerminals/clean`, codex
    добивает отвязанные команды по токену попытки при закрытии треда (`orphans::sweep_var`); testkit-драйвер.
  - `7060785` тесты claude; `dd4e714` тесты codex; `180aa61` приёмка `c34_background.rs` (claude и codex на поддельных
    провайдерах), 34 снят из `OPEN`, `contracts.md` 4.x/6.x, `adapters.md`, `experiments.md`.
  - Мутации subcoder: 9 из 10 + хвост убиты (10-я — сигнал вместо `stop_task` — руками не проверена).
  - Мной после коммита: clippy workspace чисто; core+testkit+claude+codex 441/441; приёмка 34, 4, 8 pass.
- `65a88af` гайд: комментарий скрипта-проверки называет 75 (жел. 4 ревью 99).
- Первый `checks` на `4e26046` — FAIL: `crates/aae/tests/claude.rs`
  `a_finisher_called_with_a_background_command_running_waits_for_it` ждал старого поведения (команда в фоне
  дожидается после исхода) — subcoder его не нашёл, т.к. гонял не крейт `aae`. `0266585`: тест переименован в
  `..._stops_it` и проверяет, что команда остановлена и прогон прошёл без неё; workspace 530/530.
- `4e26046`, затем `13a4433` READINESS переоценён (маркер — на `0266585`): 30 и 34 закрыты, баллы не меняются (держат 31 и живые строки 1, 2, 4a,
  8); маркер `note` — «no full live run» (жел. 3 ревью 99). Строка 22 pass.
- `checks 100` на `13a4433`: RESULT pass — ci 0 (52 с, nextest 530/530), acceptance 37 pass / 0 fail / 9 not run /
  2 open (20, 31).
- PLAN: WP-81 и WP-83 — `done`. Журнал: 17:30 (застой), 17:31 ([Д] WP-81), 18:40 ([Д] WP-83).
## Не получилось и почему

- Хаос-стенд не проверяет остановку `drain` (только получает `pool = 2`): инвариант «после остановки нет новых бидов»
  под `kill -9` держат тест `a_restart_mid_stop_gives_no_new_bead` и фаза `soft` в c30.
- Живые прогоны не делались: 4a после WP-83 (claude `stop_task` при живом субагенте) и codex 0.167.2
  (`backgroundTerminals/clean` проверен опытом на 0.168.0) — в общий живой прогон после WP-82.
## Начато и не закончено (файлы, точка остановки, следующий шаг)

Нет. Следующий — WP-82 (критерий 31), затем заморозка `crates/` и полный живой прогон строк 1, 2, 4a, 8, затем WP-84.
## Замечания предыдущего ревью: что закрыто, что нет

- Обязательных не было.
- Жел. 3 (маркер `readiness`) и 4 (комментарий гайда) — закрыты. Жел. 5 (длинные строки) — частично: абзац
  READINESS переписан с переносом. Жел. 1 (75 у `back_to`) — ждёт владельца. Жел. 2 (`HookRunner::played`) и 6
  (шаблон из метки `agent:`) — не тронуты, `exec.rs` трогался только веткой `stop_background`.
## Контекст на момент отчёта: ~175k
