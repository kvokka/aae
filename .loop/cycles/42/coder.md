# Цикл 42 — coder
STATUS: done

## Взято в работу
- Обяз. 1 ревью 41: постмортем 0001 (§11.5 [Р]) в `default-policy` — новый пакет WP-40 (S), добавлен в план.
- Жел. 1–2 ревью 41 (документация).

## Сделано (коммиты)
- `10258a3` WP-40: `crates/aae-plugins/src/policy.rs` — `auth_required` → `park{account, until: now +
  min(auth_backoff_base_s·2^(n−1), auth_backoff_cap_s)}` по серии `n`; попытка после таймера ворот — проба
  «причина ушла»; `until: null` (до оператора) — только если первая попытка серии началась ≥ `auth_escalate_s`
  назад. Без ключа аккаунта — `resume` после той же паузы / `fail` при повышении (как по контракту). `billing` — как
  был (сразу до оператора). Пороги 30 с / 900 с / 7200 с в `[builtin.default-policy]`, проверяются при старте
  (`deny_unknown_fields`). Общий `backoff()` вынесен из ветки 429.
  Тесты: юнит (`aae-plugins/src/tests.rs`: случай контракта + каждый новый порог двигает решение);
  `crates/aae-testkit/tests/auth.rs` — (а) временная 403 (сырьё с Cloudflare сохранено в `failure.raw`) с удвоенным
  событием сбоя + `exited`, затем с потерянным концом (`Vanish`): 2 `attempt_failed`, 2 ограниченные парковки,
  2 `run.resumed` по таймеру, третья попытка — `run.passed`; (б) стойкая 403: две ограниченные парковки, третья —
  `until: null`, новых попыток нет, пока оператор не откроет ворота.
  Мутация «вернуть `until: null`» роняет оба сценария (проверено).
- `a238ab8` документация: `contracts.md` 10 (строки `auth_required` и `billing`), `config.md` (новые пороги;
  жел. 1 — изолированный codex без `http_headers`/`experimental_bearer_token`), READINESS (строка `default-policy`,
  шапка — жел. 2), статус постмортема 0001 (закрыт; живость — инвариант 15; dead-man's switch не нужен — см. журнал).
- Проверка: `mise run ci` EXIT 0, nextest 286/286, budget: плагины 277/1500, всего 13039/30000
  (`.loop/cycles/42/notes/ci.log`); `mise run acceptance-ci` EXIT 0, 15/15 сценариев (`.loop/cycles/42/notes/acceptance-ci.log`).
- Журнал: РЕШЕНИЕ по порогам, `billing`, отсутствию отдельной пробы и dead-man's switch.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
—

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыт (WP-40).
- Жел. 1, 2 — закрыты.
- Жел. 3 — `policy.rs` 201 строка; `run/machine.rs` и `aae-claude/src/lib.rs` не тронуты.

## Контекст на момент отчёта: ~105k
