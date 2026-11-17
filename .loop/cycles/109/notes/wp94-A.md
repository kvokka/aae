# WP-94 · группа A · сводка subcoder

## Сделано (по плану, пункты A1–A4)

- A1 `aae-contract`: удалены `Answer`, `SessionState::RequiresAction`, `RequestKind`, `Normalized::PermissionRequested`,
  `Driver::respond`; `Watchdog::human_timeout_ms` и ветка `"human_timeout"` (ключ уходит в `[watchdog]: unknown key`);
  в `schema.rs` — `run.requires_action` и константа `REQUEST`; тест `the_watchdog_deadlines_come_from_the_orchestrator_file`
  переписан на месте (ошибка разбора — на `silence = "5 minutes"`).
- A2 testkit: удалён `impl respond`, импорт `Answer`, слова о запросах в шапке и доке `ScriptedDriver`; `play(.., first:
  Normalized)`; удалён тест `a_permission_request_continues_after_the_answer`. Сигнал `step0.done` создан.
- A3 `aae-core`: events.rs (ветка `PermissionRequested`, `fn answer`, дедлайн `human`, `attempt_live`), decide.rs, hooks.rs
  (`DECIDING` из 4), msg.rs (`Msg::Answer`, `Decision::Answer`), mod.rs (`answer_json`, `answer_from`), exec.rs
  (`respond`), read.rs (`request` из RunView и `active`), recover.rs, machine.rs, store.rs (колонка `attempts.request`
  удалена, комментарий `paused_ms`, `VERSION` 23). Сигнал `core.done` создан.
- A4: `core/requests.rs` удалён (`git rm`), его случай «сбой до ack» перенесён в
  `driver_events.rs::a_failure_before_the_ack_ends_the_attempt_at_once`; watchdog.rs (шесть порогов, блок с запросом и
  тест паузы на запросе удалены, шапка и импорты); hooks.rs (тест `a_hook_answers_a_permission_request` удалён);
  chaos/check.rs (`human`, `respond`).
- deciding.rs: тест `a_deciding_event_waits_for_the_slow_hook_of_the_run_queued_ahead` переписан по плану: хуки на
  `run.turn_ended_without_outcome` и `run.attempt_failed`, timeout 30 s, `Slow` спит 29 s и решает `Fail{judged}` на оба,
  сценарий `hollow_turn() + Wait(1s) + Exit{signal 9}`, окно 120 s.

## Мутация deciding.rs

`ahead.unwrap_or(tx.now_ms())` → `tx.now_ms()`: тест красный (`run.failed.reason` = `no_decision`, дедлайн
`run.attempt_failed` = 00:00:36 — grace не добавляется, у `Exit` остановленного процесса нет). Возвращено, тест зелёный.

## Расхождения с планом

- Нет по существу. Уточнение: в мутации дедлайн без `ahead` — 36 s, а не «≤ 46 s» плана (GRACE_MS не прибавился: процесс
  умер сам, `stopped` пуст); различение от этого только надёжнее.
- В `recover.rs` комментарий к `DELETE ... hooks:` переписан без упоминания события («The deciding hooks still queued
  judge the lost attempt; ...»).

## Проверка

- `cargo clippy -p aae-contract -p aae-core -p aae-testkit --all-targets -- -D warnings` — чисто.
- `cargo nextest run -p aae-contract -p aae-core -p aae-testkit -E 'not binary(chaos)'` — 378/378.
- `AAE_CHAOS_SEEDS=96 cargo nextest run -p aae-testkit --test chaos` — 10/10.
- `git grep -nE 'requires_action|human_timeout|PermissionRequested|RequestKind|RequiresAction' -- crates/aae-contract
  crates/aae-core crates/aae-testkit` — пусто; самый длинный изменённый файл — machine.rs, 396 строк.
