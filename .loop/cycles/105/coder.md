# Цикл 105 — coder
STATUS: done

## Взято в работу
Обяз. 1–3 ревью 104 (в порядке «Следующего шага»): хранитель теста recovery, ECHILD (все дети демона — в `OWN`),
полный живой прогон + `-example` + README демо 2 + READINESS + `checks`. Жел. 1 (abort по пределу) — не брал.

## Сделано (коммиты)
- `e9cb8f0` — обяз. 3: хранитель теста recovery крутится, пока есть `old` (уходит вместе с tmp-каталогом), `sleep 5`
  вместо `sleep 60`. Мутация «sweep без ожидания» — тест красный (recovery.rs:216), через 10 с
  `pgrep -f 'while \[ ! -e halt'` пуст.
- `dc5453a` — обяз. 1: `orphans::spawn_std` / `output` / `output_std` + страж `Own`; через них идут br (оба вызова),
  `git` хука `worktree`, git-кэш, `version.rs`, `harness.rs`. Проверка «мимо никто»: `clippy.toml`
  `disallowed-methods` (spawn/output/status у std и tokio `Command`), `#![deny]` в корнях крейтов демона, в workspace
  lint — `allow` (тесты и приёмка свободны); юнит-тесты с прямыми spawn — `#[allow]` на `mod tests`. `bridge.rs` —
  `#[allow]` с причиной: отдельный процесс без жнеца. Тест `orphans::tests::the_reaper_leaves_a_std_child_to_its_wait`
  (ребёнок стал зомби → `reap()` → его `wait` успешен); мутация «spawn_std не регистрирует» — красный. Тесты
  `orphans` вынесены в `orphans/tests.rs` (бюджет 400 строк). Тесты затронутых крейтов 280/280, testkit 257/257,
  `--ci 24 30` 10/10 параллельно с testkit. В `docs/design/contracts.md` — фраза о регистрации.
- `b32c55c` — демо: `[watchdog] human_timeout = "2m"` в `root/config.toml` обоих демо, абзац в README демо 1.
  Причина — живой прогон 1 (`dc5453a`): кодер на claude haiku под `bypassPermissions` запросил разрешение на Bash,
  прогон `requires_action` ждал бы час (журнал 16:05). Прогон 1 погашен мной.
- Полный живой прогон 2 на `b32c55c` (release, чистый worktree, общий `target/`), ждал блокирующими командами:
  48 of 50 за 2650 с; FAIL только 22 (READINESS ещё на `11839e9`), 1 — on a tag. Демо 1 passed за 286 с; демо 2 —
  два запуска подряд, 1260 с и 769 с, по 1 раунду, `ver=aae-0.1.0@b32c55c` без `-dirty`. Отчёт —
  `.loop/cycles/105/notes/full-run-b32c55c.md`.
- `9d34e48` — `-example` обоих демо из этого прогона (как блок `EXAMPLE` в `run.sh`). README демо 2 («two in a row,
  one round») по-прежнему верен — не менял.
- `04984d6` — READINESS: оценка на `b32c55c`, маркеры `full-run` и `readiness`; баллы по букве те же, что до
  ревью 104 (критерий 20 закрыт). После worktree — `touch` исходников: иначе `aae-acceptance` с `CARGO_MANIFEST_DIR`
  worktree читал старый READINESS. Строка 22 — pass.
- WP-85 — `done`. `checks 105` на `04984d6`: RESULT pass; ci 91 с, nextest 548/548 за 52,7 с; acceptance 40 pass /
  0 FAIL / 9 not run / 1 on a tag / 0 open.

## Не получилось и почему
- Жел. 1 (abort сценария по пределу) — не брал: меняет `crates/`, а он нужен был до прогона; `main.rs` 398 строк
  (жел. 2) — не трогал.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Дальше — жел. 1 и 2 ревью 104 (abort сценария по пределу; вынос из `main.rs`/`scenarios/mod.rs`).
## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто (`dc5453a`): все дети демона через `orphans`, clippy denies обход, тест жнеца, `--ci 24 30` 10/10.
- Обяз. 2 — закрыто: полный прогон на `b32c55c` зелёный, кроме 22 (READINESS, закрыт `04984d6`) и on a tag;
  `-example`, READINESS, `checks`.
- Обяз. 3 — закрыто (`e9cb8f0`).
- Жел. 1, 2 — не сделано. Жел. 3 и 4/6 из переносов — ждут владельца / 4 закрыт обяз. 2 (`-example` без `-dirty`).
## Контекст на момент отчёта: ~150k
