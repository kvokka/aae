# Цикл 45 — coder
STATUS: done

## Взято в работу
1. Ревью 44 обяз. 1 — маркеры MEASURE у замеров README/READINESS, раздел для разработчика в docs/guide, WP-41d.
2. Ревью 44 обяз. 3 — WP-48 в плане дополнен `instance_idle_s` (+ жел. 2, 3, 4 вписаны в план).
3. Ревью 44 обяз. 2 — WP-44a (codex resume на живом app-server → hollow_done), разбор и фикс — expert.
4. WP-43 — `auth_required`/`billing` без повышения, ожидание на воротах аккаунта вне потолка попыток.
5. WP-49 — часовой пояс расписаний (subcoder).

## Сделано (коммиты)
- 7ad7ec3, 18c151a WP-41d: замеры с видимыми датой и sha + `MEASURE` (README: bv; READINESS: chaos 14–17 с на долю,
  hop p50 26 мс, подъём c08 0,04 с, on_claude/on_live 0,06 с со своими sha); `docs/guide/development.md` — раскладка
  test/ci/acceptance-ci/acceptance с временем по стене (29 с / 34 с / 55 с на 1a732a4, 9 ядер); README — вернул
  заголовок «Acceptance spec», убрал критерий 21. Сырые логи замеров — `.loop/cycles/45/notes/measure-*.log`.
- efb476f WP-43: `default-policy` — `auth_required` и `billing` паркуют аккаунт с паузой до 1 ч, без повышения,
  `auth_escalate_s` удалён; ядро: `attempts.waited` (схема v13) — попытка, чей сбой запаркован на `account:`, не
  считается в потолок 20; chaos-проверка инварианта 16 так же. Тест `a_lasting_403_keeps_waiting_past_the_attempts_ceiling`
  (22 сбоя → pass), мутация «считать все попытки» → красный. Документы: contracts 6.1/10, config.md, invariants.md,
  постмортем 0001, READINESS. Решение по толкованию — журнал.
- e05c6b0 WP-44a (expert): причина — после упавшего хода (`systemError`) тред остаётся загруженным в app-server,
  `thread/resume` присоединяется к нему и игнорирует новый config → финишер со старым токеном → `stale_attempt` →
  `hollow_done`. Фикс: app-server с `-c thread_unload_delay_secs=0`, resume сначала `thread/unsubscribe` до
  `notLoaded` (≤ 10 с, иначе `transcript_divergence`). Тест `aae-codex/tests/codex.rs::
  a_thread_resumed_on_its_live_app_server_gets_the_new_attempt_token` (красный без фикса). Обход из c10 удалён.
  Разбор — `.loop/cycles/45/notes/wp44a.md`.
- af8dfd8 WP-49 (subcoder, проверено мной): `cron` в системном поясе, `timezone` в config.toml (jiff, системная
  база), разрыв/наложение, reload перевзводит cron; тесты на POSIX-поясе Берлина.
- Проверка на af8dfd8: `mise run ci` EXIT 0 (nextest 291/291, 31,4 с); `mise run acceptance-ci` 15/15 за 55 с
  (c10 — без обхода). Логи — `.loop/cycles/45/notes/ci-final.log`, `acceptance-ci-final.log`.

## Не получилось и почему
- WP-42 (M, ядро) не начат: бюджет контекста — нужен план эксперта и отдельный цикл.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Ничего. Статусы WP-41a–d, 43, 44a, 49 — doing до ревью и зелёного GitHub CI. Открыто по WP-44a: судьба фоновых
  терминалов/MCP треда при немедленной выгрузке не мерилась; проверено только на codex 0.168.0. Следующий шаг
  плана — WP-42 (с планом expert).
- Рабочее дерево: изменены файлы харнесса `.claude/agents/*.md`, `scripts/loop/loopctl` (не мной, не коммитил).
## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1: закрыт (grep `about|[0-9] s|minute` по README — пусто; в development.md у каждого числа маркер).
- Обяз. 3: текст WP-48 в плане дополнен; сам пункт открыт до ответа владельца/WP-48.
- Обяз. 2: закрыт (WP-44a, e05c6b0; c10 без обхода зелёный).
- Жел. 1 (README заголовок, критерий 21): закрыт. Жел. 2: открытая строка в плане (после WP-48). Жел. 3, 4: внесены в WP-48.
## Контекст на момент отчёта: ~190k
