# Цикл 87 — coder
STATUS: done

## Взято в работу
Обяз. 1 ревью 86 (тест остановки exec-попытки); жел. 2 и 3 ревью 86.

## Сделано (коммиты)
- `2ef45f0` Core: tests pin the stop of an exec attempt to that attempt.
  - `exec_step::stop` теперь принимает payload `stop_exec` и строит ключ той же функцией `key`, что и `run`
    (`exec.rs` — одна строка вызова). Поведение не менялось.
  - (б) юнит-тесты `run::exec_step::tests`: `a_late_stop_of_the_last_attempt_leaves_the_next_one_running` (команда
    попытки 2 `sleep 0.3; echo done`, stop попытки 1 → exit 0, stdout `done`) и контрольный
    `a_stop_of_an_attempt_ends_its_command` (stop своей попытки → `TimedOut`).
  - (а) `tests/exec.rs`: тест отмены идёт с таймаутом шага 60 с (`check_within`), окно смерти — 5 с от отмены.
  - Мутации: M2 (`machine.rs` без `"attempt"`) → красный `cancelling_the_run_stops_the_command` (5,06 с);
    M3 по смыслу (ключ без попытки в `key`) → красный `a_late_stop_…`. После — откат, `git diff crates` чист.
    Буквальный `git apply -R` диффа `c14bbfd` теперь не ляжет (код `stop` переехал в `key`); мутация M3 —
    правка `key` на `<run>:0`.
  - 5 повторов `exec_step` + бинарник `exec`: 15/15 каждый раз.
- `b52c249` READINESS: `ver=2ef45f0`; в примечании MEASURE сказано, что правка лежит на пути обоих демо (жел. 3).
- Жел. 2: брать попытку из ключа эффекта нельзя — `Executor::execute` получает ключ очереди (`run`), а не dedupe
  `stop:<n>`; вместо этого rustdoc `stop` говорит, что старый `stop_exec` без попытки ничего не находит.
- `scripts/loop/checks 87` на `b52c249`: pass; nextest 459/459 (49 с), acceptance 28 pass / 0 fail / 9 not run.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
—

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыт (оба мутанта убиты).
- Обяз. 2 (`drain` §5.6/§9.1) — ждёт владельца, код не трогал.
- Жел. 1 (чужой подъём Rust 1.100) — не мой; на время `checks` лежал в `git stash`, после возвращён. Точечные
  тесты шли с `--ignore-rust-version`.
- Жел. 2 — rustdoc (см. выше). Жел. 3 — закрыт.

## Контекст на момент отчёта: ~45k
