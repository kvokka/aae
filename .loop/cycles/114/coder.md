# Цикл 114 — coder
STATUS: done

## Взято в работу
- Обяз. 1 и Жел. 1–3 ревью 113.
- Разбор застоя с expert.
- WP-102 (`await`, M, критичный) целиком: план expert → код → тесты ядра и e2e (subcoder) → документы.

## Разбор застоя (expert, `.loop/cycles/114/notes/stagnation.md`, запись в журнале 08:37)
- Главное: в цикле 111 план вырос с 15 до 22 пакетов, а оценка — это доля закрытых пакетов. По старому плану было
  бы 8.0 → 9.3 → 10. Работа не стоит.
- Второе: «Обязательно» ревью 111 и 113 — выжившие мутации охран в свежем коде кодера. Правило с этого цикла:
  до коммита мутировать каждое новое условие; в отчёт — «охрана → мутация → тест» (ниже).
- Третье: темп. Цикл 113 закрыл один S и кончился на ~95k. В этом цикле после добивок взят и закрыт WP-102.
- «Желательно» вне плана (`held` в codex, колонка `class`, флейк chaos) не брал.

## Сделано (коммиты)
- `ac68778`: тест `a_background_subagent_keeps_…` закрепляет обе охраны.
  - В ходе при живом субагенте `api_retry` даёт ровно `[Retrying]`.
  - После `result` ровно один `State`.
  - Скилл `recover` называет `adapter_crash`.
  - Тест скилла сверяет флаги команд в тексте с CLI (`clap` `get_arguments`), абзац в `architecture.md` поправлен.
- `86e3957` WP-102: шаг `await`.
  - Конфиг: `StepFile/StepKind::Await`, `fail_on_timeout` по умолчанию `true`. Чужие ключи дают ошибку
    загрузки через `deny_unknown_fields`.
  - Событие `run.awaiting {run, deadline}` и его схема для триггеров и хуков.
  - Ядро: новый `run/await_step.rs` (`start_await`, `resolve`, `await_timeout`). Поправлены:
    - `NO_PROCESS` (слот не держится);
    - `recover` (рестарт не трогает ожидание);
    - `on_deadline` (таймаут → `failed{check, await_timeout}` или `pass` «timeout»);
    - `finish` (`await` закрывает ряд проверок перед собой);
    - `render` (снимок `{fail_on_timeout}`, 168h).
  - Чтение: фильтр `awaiting`, поле `steps` в виде прогона.
  - API `POST /v1/runs/{id}/resolve`, CLI `aae run resolve <run> <step> pass|fail [--summary] [--data] [--key]` и
    `aae run list --awaiting`; в текстовом `run list` появилась колонка шага. Разбор `--data` вынесен в
    `cli/finish.rs::data_arg`, его используют оба.
  - `openapi.json` перегенерирован.
  - Документы: config.md «Formula», contracts §1.3, §6.1, §6.2, §6.3, §6.4, invariants «Recovery» п. 3, overview
    (строка «A human gate» ушла из «не входит»), architecture, frontends.
  - Тесты: контракт (1) и 9 тестов ядра (`crates/aae-testkit/tests/core/await_step.rs`).
  - `string`/`agent_keys` перенесены из `config/items.rs` в `variants.rs`, чтобы `items.rs` остался ≤ 400 строк.
- `15b2bd6`: e2e `crates/aae/tests/e2e/await_step.rs` (CLI: `--awaiting`, отказ на чужой шаг, повтор с ключом,
  `steps.gate`, `not_found`). В таблице ошибок `replies.rs` добавлены `/resolve` 404 и 409. Пример в guide
  first-project и recovery показывает колонку шага и `--awaiting`.
- Гейт: `scripts/loop/checks 114` на `15b2bd6` — pass, 51 s, nextest 570/570. Бюджет: ядро 11035/18000, фронт
  2738/4000.

## Охрана → мутация → тест
| охрана | мутация | краснеет |
|---|---|---|
| `stream.rs:146` `!self.open` | убрана | `a_background_subagent…` (tests.rs:191) |
| `stream.rs:146` `self.agent` | убрана | 1 тест aae-claude |
| `stream.rs:146` `!out.is_empty()` | убрана | 4 теста aae-claude |
| `stream.rs:146` «последнее не State» | убрана | tests.rs:195 |
| флаги в тексте скилла | `--parent` → `--parents` | `every_command_a_skill_names_exists` |
| `resolve`: state / step_kind / step, лимит summary, лимит data (каждое отдельно) | → `false` | `an_outcome_for_another_step_is_rejected_and_changes_nothing` |
| `await_timeout`: `fail_on_timeout == false` | → `true` / → `false` | оба теста таймаута, тест рестарта |
| ветка `await` в `events.rs` | удалена | оба теста таймаута, тест рестарта |
| `NO_PROCESS` с `'await'` | убрано | `a_waiting_run_leaves_its_slot_to_the_next_run` |
| `self.schedule` в `start_await` | → `Ok(())` | тот же |
| исключение `await` в `recover.rs` | убрано | `a_restart_leaves_the_wait_and_its_deadline_alone` |
| `row_ends` с `Await` | убрано | `a_failed_check_before_an_await_sends_the_run_back_first` |
| CLI `("awaiting", …)` | убрано | e2e `an_outcome_from_the_cli_moves_a_waiting_run_on` |
| SQL `?5` в `read.rs` | → `1` | тот же e2e |
| `view["steps"]` | удалено | тот же e2e |

Мутации ядра и e2e гоняли subcoder'ы, их отчёты выше сведены в таблицу. Мутации `stream.rs` и скилла гонял я сам.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
—

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 закрыт: мутация `!self.open` краснит тест.
- Жел. 1 закрыт: строгий счёт `State` после `result`.
- Жел. 2 закрыт: `adapter_crash` добавлен в скилл.
- Жел. 3 закрыт: тест сверяет флаги в тексте скилла.
- Жел. 4 (флейк chaos) и Жел. 5 (перенос из 112: `held` в codex, колонка `class`) не брал: они вне плана, правило
  разбора застоя.

## Для ревью
- Допущения WP-102 записаны expert в журнал (08:45):
  - таймаут даёт класс `check`;
  - имена `resolve`, `awaiting`, `steps`;
  - превышение лимитов — 409;
  - проверки после упавшего `await` идут вместе с ним;
  - без ключа идемпотентности поздний повтор применится ко второму раунду, это описано в `frontends.md`.
- В API у `/resolve` поле `outcome.summary` обязательно, как у `/finish`: без него 400. CLI по умолчанию шлёт `""`.
- Не проверены тестом: `--data @<file>` у `resolve` (общая функция с `aae finish`) и исход `fail` через CLI (в ядре
  покрыт).

## Контекст на момент отчёта: ~175k
