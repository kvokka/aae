# Цикл 54 — coder
STATUS: done

## Взято в работу
- Ревью 53, обяз. 1 и 2.
- WP-51 разбит на 51a (API/MCP: 422, ключ у MCP, ворота 400), 51b (CLI и doctor), 51c (OpenAPI). Взяты 51a и 51b.
- Ревью 53, жел. 2, 4, 5.

## Сделано (коммиты)
- `5bacf3f` WP-42b: состояние «прохода ещё не было» в `wait_gone` теперь в типе `Walks` (`Default`), unit-тест
  сворачивает последовательности проходов. Мутация `Walks::default()` → `Walks(true)` — красная; `!left` → `true` /
  `false` в `mem::replace` — красные по построению теста. `process.rs` 399 строк.
- `8907edf` WP-47: unit-тест `a_listener_on_every_interface_is_reached` на `listener()` со строками `/proc/net/tcp*`:
  `127.0.0.1`, `0.0.0.0`, `::`, `::ffff:127.0.0.1` → сокет; другой порт, не LISTEN, `127.0.0.2`, `::1` → нет. Обе
  мутации ревью (`00000000`→`FFFFFFFF`, `::`→`F0…0`) — красные. IPv6 на хосте не нужен.
- `a5d5c5e` WP-51a: `CoreHandle::send_request(key: Option<(key, request)>)`, колонка `idempotency.request`
  (миграция 14), `Error::KeyReused` → 422 `idempotency_key_reused`; middleware `fingerprint` (FNV-1a метода, пути,
  тела); MCP `start_run`/`cancel_run` принимают `idempotency_key`; `POST /v1/gates` и `/v1/gates/open` с неверным
  ключом → 400. Тесты: ядро (`tests/idempotency.rs`), API (`api.rs`: 422 на другое тело и другой путь, 400 ворот),
  MCP (`mcp.rs`). Мутация «снять middleware» — красная. Доки `frontends.md`, `mcp.md`, `openapi.json`.
- `5a3117c` WP-51b: CLI под `--json` пишет ошибку JSON в stderr (`no_daemon` без демона); `run start` при
  `nothing_ready` — сообщение и код 3; `null` в человеческом режиме не печатается; `doctor` показывает разрешённый
  путь (в т. ч. найденный через `PATH`) и вывод `--version` (`aae_core::untested` → `tested_version` с `Result`).
  Тесты: `cli.rs`, `doctor.rs`, `tracker.rs` (код 3 на настоящем br). Старый тест `cli.rs` повторял ключ `k1` с
  другим телом — поправлен (теперь это 422 и проверяется).
- `75a5ee0` ревью 53 жел. 2, 4, 5: строки 9 и 11 приёмки проверяют число тестов (2 и 34); токен сервиса не
  `${…}`; `ver` маркера — `ea7f664`.

## Не получилось и почему
- Ничего из взятого.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-51c (схемы ответов и коды ошибок в OpenAPI) — не начат. Там же — ответ `gate close/open` (сейчас `null`).

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1, 2 — закрыты (мутации красные). Обяз. 3 — ждёт владельца.
- Жел. 2, 4, 5 — сделаны. Жел. 1 (mise.lock) — решение владельца. Жел. 3, 6, 7 — не трогал.

## Проверки
- `scripts/loop/checks 54` на `75a5ee0`: RESULT pass; nextest 331/331; приёмка 19 pass / 0 fail / 1 open / 7 not run
  (строка 1 после `a08a0c2` — только на тегах). Итог — `.loop/cycles/54/checks.md`.

## Контекст на момент отчёта: ~200k
