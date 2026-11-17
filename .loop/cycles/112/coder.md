# Цикл 112 — coder
STATUS: done

## Взято в работу
- Обяз. 1 ревью 111 (ранний `auth_required` claude), Жел. 1 и 3.
- WP-99 (S): фоновый subagent claude под `background_silence`.
- WP-100 (S): инструменты ручного восстановления (реализация — subcoder, проверка и коммит — я).
## Сделано (коммиты)
- `edf5023` WP-97: тест `a_logged_out_retry_fails_at_once_and_an_overload_does_not` (403/unknown, null/oauth_org_not_allowed, null/cloud_credential_error → ровно один `AuthRequired`; 529/overloaded → нет). Мутация ревью (`Some(401)` + выключенный `AUTH`) краснит тест; одна замена `401|403`→`401` тоже краснит. Заодно Жел. 3: `unknown` без текста берёт `subtype=` или `api_error_status=` (тесты в `an_unknown_failure_keeps_...`).
- `ee4060a` WP-98: s16 проверяет текст `required MCP servers failed to initialize` в `raw`; мутация `required = false` краснит 3 из 3.
- `bf14187` WP-99: стенд (claude 2.1.326 + фейковый Messages API, без модели; заметка `.loop/cycles/112/notes/wp99-stand.md`, код стенда не коммичен) показал: после `result` при живом `local_agent` состояние остаётся `running`, нового `session_state_changed` нет до конца само-запущенного хода; пока идёт команда субагента — полная тишина (ни `tool_progress`, ни `stream_event`), шаги субагента (`parent_tool_use_id`) приходят вне хода только на границах шагов. Правка адаптера: после такого `result` — `State{Idle, background: true}` (ядро уже взводит `background_silence` и зовёт `stop_background`, покрыто `aae-testkit/tests/core/watchdog.rs`); `Activity` вне открытого хода не выдаётся. Тест `a_background_subagent_keeps_the_session_idle_with_background_until_its_wake_up_ends` на строках формы стенда. Документы: contracts 4.6 (claude), experiments.md («A background subagent after the result», MEASURE).

- `7134d4c` WP-100: аудит (subcoder, заметка `.loop/cycles/112/notes/wp100.md`) — недостающего кода во фронте/ядре нет. 4 теста на фейковом драйвере в `crates/aae-testkit/tests/core/manual_recovery.rs` (видимость отказа и сессий всех попыток; S1 `--continue-from` на упавшем прогоне; S2/S3 новый прогон на тот же bead, с запиской через вход формулы; S4 bead отпущен, `gate close` держит прогоны), `docs/guide/recovery.md`, ссылки из `docs/README.md`, `README.md`, `docs/api/frontends.md`. Расхождения с текстом плана — в журнале.
- Гейт: `scripts/loop/checks 112` на `7134d4c` — pass, 36 s, nextest 557/557.

## Не получилось и почему
- Жел. 2 ревью 111 (путь `held` в `aae-codex/src/lib.rs`) тестом не закрыт: на codex 0.168 отказ сервера рига без `required` приходит после `turn/start` в 5/5 прогонов (зонд, не коммичен), детерминированно до вызова его не получить. Записано в журнал.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Следующее по плану: WP-101 (скилл восстановления, зависит от WP-100), затем WP-102 отдельным циклом.
## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто (`edf5023`), мутация ревью краснит новый тест.
- Жел. 1 — закрыто (`ee4060a`).
- Жел. 2 — не закрыто: детерминированного теста на настоящем codex нет (см. выше и журнал).
- Жел. 3 — закрыто (`edf5023`).
- Жел. 4 — `aae-codex/src/stream.rs` в этом цикле не правился, разбиение не требовалось.
## Контекст на момент отчёта: ~140k
