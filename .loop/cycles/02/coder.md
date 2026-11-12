# Цикл 02 — coder
STATUS: done

## Взято в работу
- Обязательное из ревью 01: версии codex и GitHub Actions.
- Желательное из ревью 01: якорение ключей маркера `MEASURE` в `scripts/budget.sh`, лимит опытов 1 500 строк.
- WP-02 (опыт 1а, протокол claude), WP-03 (опыт 1б, процессы и фон claude) — пара S.
- Запас контекста остался — WP-06 (опыт 3, br как источник событий), S.

## Сделано (коммиты)
- `92820af` WP-01: codex 0.166.0 → 0.167.2 (`mise use`, `mise.lock`), checkout v5 → v7, mise-action v3 → v5; версия в плане.
- `a66dec1` WP-01: ключи `id/ver/env/date` маркера ищутся вне значений в кавычках и только после пробела; случай
  `note="see ver=1"` в `budget-selftest.sh`.
- `35b1fb9` WP-02: отдельный Cargo workspace `experiments/` (корневой workspace его исключает, `target` в .gitignore),
  клиент stream-json `experiments/01-claude-protocol/src/lib.rs`, сценарии basic/resume/permission/interrupt,
  очищенные логи; `budget.sh` — лимит опытов 1 500 строк (+ случай в self-test).
- `dc55a72` WP-03: `experiments/02-claude-process` — orphans (с subreaper и без), background.
- `3011121` итоги WP-02/03: `docs/design/experiments.md`, READINESS, план, журнал.
- `bb64919` WP-06: `experiments/05-br-events` (notify + rusqlite); `generated/` исключён из лимита опытов.
- `7bf66a0` итоги WP-06 в `docs/design/experiments.md`, READINESS, план, журнал.

Главные результаты (подробно — `docs/design/experiments.md`, журнал):
- claude: нативное подтверждение приёма `command_lifecycle{command_uuid}`; `uuid` сообщения = ключ идемпотентности
  (повтор → `completed` без хода) — расходится со спекой, записано в журнал; resume модель не сбрасывает; interrupt —
  `error_during_execution`/`aborted_tools` за 43 мс; init только после промпта.
- Инвариант 7: Bash claude живёт в своей сессии (setsid), `kill(-pgid)` не достаёт; cgroup на хосте недоступна;
  выбран env-маркер + своя группа + subreaper.
- «Idle без фона»: state=idle при живой фоновой shell-команде; правило из трёх условий (idle, пустой список фона,
  нет task_notification после последнего result).
- br: переименование `issues.jsonl` — надёжный сигнал (0 пропусков под 16 параллельными claim); `events` по курсору;
  актор через `BD_ACTOR`/`--actor`; `BR_AGENT_NAME` и т. п. в 0.7.6 не пишутся.

Проверки: `mise run ci` зелёный (fmt, clippy, nextest 0 тестов, deny, machete, budget self-test, budget);
`experiments/`: `cargo fmt`, `cargo clippy --all-targets -D warnings` чисто. Строки опытов: 805 / 1500.
CI на GitHub: прогон после пуша цикла 1 — success (`gh run list`).

Живые прогоны: claude haiku, 8 сессий + 2 запуска с недоступным API (только init.model). Записано в журнал.

## Не получилось и почему
- Субагентов helper/subcoder/expert в сессии нет (доступны только coder/reviewer) — всё сделано самим.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Ничего. Следующее: WP-04 (codex app-server, M) — отдельным циклом; затем WP-05 (M), WP-07 (S) и WP-08.
В WP-04 сгенерированные типы класть в `experiments/03-codex-appserver/src/generated/` — иначе не хватит лимита 1 500.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательно п. 1 (версии) — закрыто (`92820af`), `mise latest codex` = 0.167.2, actions — последние мажоры.
- Ключи маркера подстрокой — закрыто (`a66dec1`), плюс значения в кавычках вырезаются целиком.
- Лимит опытов 1 500 строк — добавлен в `budget.sh` с self-test.
- Пары S за цикл — сделано (WP-02+WP-03, плюс WP-06).
- Формулировка «Assessed … at» в READINESS — теперь «on top of `a66dec1` (the commit before the assessment)».
- `.deja/` — не трогал. CI на GitHub — проверен, зелёный.
- `build.rs` вне `crates/*/src` — не делал (его нет).

## Контекст на момент отчёта: ~175k
