# Цикл 55 — coder
STATUS: done

## Взято в работу
- Обязательные 1–3 ревью 54 (4-й — ждёт владельца).
- Желательные ревью 54: 3 (старые ключи), 4 (маркеры замеров), 5 (длина строки mcp.md, код выхода clap), 6 (testkit lib.rs > 400).
- WP-45 (exec-шаг: окружение, типы входов, метка рига у бида; contracts 5.1 по коду).
- WP-51c (OpenAPI: схемы ответов, коды ошибок) — субагентом subcoder.

## Сделано (коммиты)
- 876b8ad — обяз. 1: тест «тот же ключ, то же тело, другой путь» (`POST /v1/gates` → `/v1/gates/open`), мутация
  «путь убран из отпечатка» краснеет (проверено).
- 5ee08d0 — обяз. 2: `cli::fail(json, code, message)`; под `--json` все локальные ошибки — `{"error":{code,message}}`
  (коды `usage`, `config_error`, `not_found`, `internal`, плюс `no_daemon`); `doctor`/`fetch` в `main.rs` тоже.
  Тесты: `finish` без окружения и `config explain --project nope` под `--json`. Документ `frontends.md` перечисляет
  коды и говорит, что ошибка разбора аргументов — код 2 (жел. 5). `mcp.md` — строка ≤ 120.
- 72cf7bd — обяз. 3: CI-строка 17 в `checks.rs` (nextest `binary(openapi)|test(both_epochs_serve_the_same_five_tools)`,
  ровно 3 теста); `aae-acceptance --ci 17` — pass. Жел. 4: маркеры `dev-ci-wall` и `dev-acceptance-ci-wall`
  перемерены честно по `checks.md` цикла 54 (`ver=75a5ee0`: 62 с и 71 с). Убран дубль строки rustdoc у `tests`.
- 4f98cdf — жел. 3: ключ с пустым `request` (до миграции 14) совпадает с любым запросом; тест + мутация краснеет.
- b4aadc7 — WP-45: `AAE_RUN`, `AAE_BEAD`, `AAE_INPUT_<NAME>` у exec-шага; проверка `integer`/`boolean` при старте;
  бид по id с меткой другого рига — отказ при старте (по кэшу ready) или `failed{bead_of_another_rig}` при claim с
  release; `contracts.md` 5.1–5.2 описывает 5 операций кода. Тесты в `exec.rs` и `beads.rs`, обе проверки рига
  мутациями краснеют.
- 4f0a3f3 — жел. 6: `aae-testkit/src/lib.rs` 429 → 329 строк (`control.rs`).
- d507e2e — WP-51c (subcoder, проверено и закоммичено мной): `ErrorReply` — то, что `ApiError` сериализует сам; у
  каждой операции перечислены достижимые ошибки (ключевые POST: 400/409/422/500/503, finish без 409; GET `{id}`:
  404; потоки: 410; 401 по TCP — в описании документа); типизированные схемы ответов (`api/replies.rs`), вид прогона —
  `Object` со ссылкой на contracts 1.3; кривой path/query → JSON 400 (`Param<E>`, был текст axum). Тесты:
  `every_operation_lists_the_errors_it_can_return`, `a_malformed_query_is_an_error_reply`,
  `crates/aae/tests/replies.rs::the_replies_fit_their_schemas` (18 настоящих ответов против схем; переименование поля
  краснеет — проверено субагентом). POST на неизвестный `{id}` — 409, не 404 (журнал, НАХОДКА).
- 72ae704 — строка 17 ждёт 4 теста (новый тест в `openapi.rs`). **Ошибка:** коммит сделан с `-a` и захватил
  `.loop/JOURNAL.md`, `.loop/PLAN.md`, `.loop/state.json`; историю не переписывал (запрещено).

Проверки: `scripts/loop/checks 55` на `72ae704` — RESULT pass: ci 64 с, nextest 335/335, acceptance 20 pass, 0 fail,
1 open (20), 7 not run.

## Не получилось и почему
- Жел. 2 (имя инструмента в отпечатке MCP) — не сделано: у `start_run` и `cancel_run` аргументы не пересекаются,
  тест, различающий их, упирается в порядок проверки ключа и разбора тела; риск мал.
- Жел. 1 (точка вызова `Walks::default()` в `wait_gone`) — не трогал.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет. Следующее по плану: WP-44 (codex `CODEX_HOME`, M), WP-54 (чистка: `experiments/` → docs, лимит 5 000),
  WP-53 (демо), WP-56. Схемы не покрыты проверкой: список экземпляров, `null`-ответы.

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1–3 — закрыты; обяз. 4 — ждёт владельца.
- Жел. 3, 4, 5, 6 (lib.rs) — закрыты; жел. 1, 2 — нет (см. выше); остальные переносы жел. 6 — нет.

## Контекст на момент отчёта: ~180k
