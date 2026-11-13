# Цикл 26 — coder
STATUS: done

## Взято в работу
- WP-28 (контрактные тесты claude) + жел. 3 ревью 25 (claim через смерть сессии).
- WP-30 (контрактные тесты codex, критерии 10 и 13).
- Из WP-29b: `max_sessions`.
- Жел. 1 и 4 ревью 25 (маркер READINESS, формулировка WP-37).

## Сделано (коммиты)
- 10a6514 WP-28: 15 сценариев §7.8 на настоящем claude 2.1.322 + поддельный Anthropic (`crates/aae-claude/tests/contract.rs`); в подделке `Reply::Background`; дубль billing-теста из `claude.rs` удалён.
- ee0e1cd WP-28: демон-тест критерия 9 для claude (kill -9 посреди Bash → `harness_crash` ≤ 1 с → resume → passed) с бидом: claim держится между смертью и исходом, бид закрыт (критерий 16, жел. 3).
- dee06a6 WP-28: 4а на подделке — финишер при фоновой shell-команде, прогон passed только после неё.
- 6170729 WP-28: `docs/harness-updates.md` — процедура обновления харнесса, таблицы сценариев, журнал версий.
- d43ac77 WP-30: 15 сценариев на настоящем codex 0.167.2 (`crates/aae-codex/tests/contract.rs`); исправлена классификация: `insufficient_quota` (codex: `usageLimitExceeded` «…billing details») → `billing`.
- 848a344 WP-30: acceptance `c10_quota.rs` (ворота аккаунта, второй прогон ждёт, оба passed после ворот; 70,6 с) и `c13_instances.rs` (3 агента → 2 app-server'а, ленивый старт, остановка `idle`; 62 с). Подделка OpenAI: `UsageLimit(secs)`, `Task(cmd)`.
- 99f3ec1 WP-30: демон-тест критерия 9 для codex (kill -9 app-server посреди команды → `harness_crash` ≤ 1 с → тред продолжен в новом app-server → passed).
- d8adfa6 WP-29b: `[harness.codex] max_sessions` (NonZeroU32, по умолчанию 8), `docs/design/config.md`, тест конфига.
- READINESS (маркер `ver=` вместе со строкой Assessed), PLAN, JOURNAL — коммитит loopctl/последний коммит цикла.

Проверки: `mise run ci` EXIT 0 (nextest 229/229, бюджеты: ядро 6857, codex 988, всего 10626); контрактные наборы прогнаны по 3–4 раза без флейков; `mise run acceptance-ci` (на 848a344): EXIT 0, 12/12 (новые 10 — 70,3 с, 13 — 62,2 с).

## Не получилось и почему
- Субагенты недоступны (Agent: «Available agents: coder, reviewer»; `coder` запрещён хуком) — запись в журнале.
- Реальный прогон haiku / дешёвой модели codex, 4а с настоящим фоновым субагентом, тест «повторный `New` той же попытки» — не делал, перенесено в WP-39 (план).

## Начато и не закончено (файлы, точка остановки, следующий шаг)
- WP-29b (doing): осталось skills в `CODEX_HOME/skills`, resume после рестарта демона с проверкой `clientId`, форма ответа `item/tool/requestUserInput`.
- WP-20c: сверка экземпляров на `daemon:resync` — не брал.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Жел. 1 (маркер READINESS) — закрыт: `ver=` = коммит строки Assessed.
- Жел. 2 (5б на настоящих часах) — не трогал, флейка не было.
- Жел. 3 (claim через смерть сессии) — закрыт демон-тестом `crates/aae/tests/claude.rs::a_claude_killed_in_a_command_is_resumed_and_its_run_passes`.
- Жел. 4 (WP-37 «Готово, когда») — сужено в плане.
- Жел. 5 (ревью 24) — без изменений, в WP-32/WP-38.

## Контекст на момент отчёта: ~220k
