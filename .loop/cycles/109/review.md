SCORE: 5.6/10
VERDICT: continue
BLOCKERS: 2

# Ревью цикла 109

Дифф `a2f5f50..HEAD`: 3 коммита (доделка WP-93, WP-94, WP-95), 63 файла, +737/−1142. Вне `.loop` дерево чистое.
Незакоммичены только `JOURNAL.md`, `PLAN.md`, `state.json` и `cycles/109/`, то есть штатные файлы цикла.

`checks.md`: HEAD `4036b25` совпадает с текущим, RESULT pass, 31 s (в цикле 108 было 36 s), nextest 544/544. Гейт я
не перезапускал.

## Обязательно до продолжения

1. **Под root разрешающий дефолт claude не даёт запустить ни одной сессии.** Это регрессия WP-94, способ исправления
   ждёт владельца.
   - Где: `crates/aae-claude/src/part.rs:81-100`, `DEFAULT_PERMISSION_MODE = "bypassPermissions"`.
   - Проверено вживую, без модели: `sudo -n env -i HOME=<tmp> CLAUDE_CONFIG_DIR=<tmp> ANTHROPIC_BASE_URL=http://127.0.0.1:9
     claude -p --permission-mode bypassPermissions --permission-prompts none hi`. Результат — exit 1 и
     `--dangerously-skip-permissions cannot be used with root/sudo privileges for security reasons`.
   - Что ломается: aae под root (контейнер, CI) с конфигом без `permission_mode` теряет каждую сессию claude ещё до
     init. Адаптер видит `exited` и отдаёт ядру сбой. Политика гоняет resume/new_session до потолка попыток, прогон
     падает как `infrastructure`. До цикла тот же конфиг работал.
   - Кодер записал это как непроверенную НАХОДКУ (журнал 08:33). Теперь она проверена.
   - Решение 117 требует `bypassPermissions` по умолчанию. Выбрать способ (`IS_SANDBOX=1`, другой режим под root,
     отказ `config_error` с понятным текстом, предупреждение `doctor`) — вопрос владельцу, он записан в журнал.
   - Минимум до ответа, без отступления от решения: `docs/design/config.md` `### Permissions` и contracts 4.6 говорят,
     что под root claude такой дефолт не принимает и что задать вместо него.
   - Как проверить: тест аргументов или контрактный тест на выбранное поведение. Документ сверить с кодом.

2. **Сброс silence при `permission_denied` не покрыт тестом, мутация выживает.** Это инвариант 15 (watchdog).
   - Что обещает документ: contracts 4.3, строка `permission_denied` — «still activity for the watchdog in
     `running`».
   - Мутация: в `crates/aae-core/src/run/events.rs:186` заменил `"running" => silence(tx, &row, w.silence_ms)` на
     `"running" => Ok(())`. Весь набор (`--workspace`, без chaos) — 544/544 зелёные. Откатил, `git status` чистый.
   - Нужен тест на управляемых часах: денай приходит перед порогом silence, `stall` наступает не раньше, чем через
     silence после денная. Без сброса тест краснеет.

## Желательно

1. `crates/aae-testkit/src/control.rs:39`: комментарий «Queues the scenario played by the next `send_prompt` or
   `respond`» — `respond` больше нет.
2. Тест `a_control_request_gets_an_error_at_once` (`crates/aae-claude/src/tests.rs`) под мутацией (ответ не пишется)
   не краснеет, а виснет: скрипт ждёт в `read line`, nextest без slow-timeout будет висеть вечно. Я проверил под
   `timeout 120`. Нужен `tokio::time::timeout` вокруг ожидания `Exited` или `read -t` в скрипте.
3. Контрактный s13 claude гоняет настоящий CLI в режиме `manual`, где claude сам отклоняет Bash, но не проверяет
   `Normalized::PermissionDenied`. Одна проверка там подтвердила бы вживую форму `result.permission_denials` на
   2.1.326. Сейчас форма `system/permission_denied` взята только из кода, кодер это сам отметил в журнале (09:40).
4. Ветка «денай в `starting`» (contracts 4.3: «in every live state») тестом не покрыта: тест ядра шлёт денай после ack.
5. Прошлое Жел. 2 (фиксированное ожидание 200 ms в `no_prompt_goes_into_a_session_a_stopped_writer_never_recorded`)
   открыто.
6. `demos/1-planner-executor/README.md` правился вместе с `config.toml`. Это вне буквы разрешения («config.toml»),
   но без правки README называл бы удалённый ключ. Кодер записал это в журнал. Принимаю.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-91 | done | — |
| WP-92 | done | отступления от критериев описаны в ревью 108 (Жел. 3–4), кодер их признал |
| WP-93 | done | Обяз. 1–2 ревью 108 закрыты: мутации `grace` в `deciding`, grace в `queued()`, `OR kind = 'new_session'` и `n > 1` → `n >= 1` — каждая краснит свой тест (перепроверено) |
| WP-94 | done | Обяз. 1 (root) |
| WP-95 | done | Обяз. 2 (watchdog), Жел. 3–4 |
| WP-96…WP-101 | todo | — |

## Проверено

- Гейт: `checks.md` — HEAD `4036b25`, pass, 31 s, nextest 544/544. Не перезапускался.
- Мутации, все откачены, после каждой `git status` без изменений вне `.loop`:
  - убраны `grace` в `deciding` и grace в `queued()` → красный (`attempt_edges::a_decision_on_a_failed_attempt_waits_for_the_stopped_process[_after_a_restart]`);
  - убрано `OR kind = 'new_session'` → красный (`a_resume_after_a_new_session_that_never_started_leaves_the_continued_session`);
  - `n > 1` → `n >= 1` → красный (`a_new_session_after_a_daemon_crash_gets_the_step_prompt_once`);
  - claude `denied.clear()` убран и дедуп по `tool_use_id` выключен → красный (`a_denial_is_told_once_from_either_source`);
  - в ядре убран emit `run.permission_denied` → красный (`a_denied_permission_is_told_and_the_turn_goes_on`);
  - в claude убран ответ на `control_request` → тест висит (Жел. 2);
  - **сброс silence при денае → выжила** (Обяз. 2).
- Флейки: тесты, которые трогает дифф (`attempt_edges`, `driver_events`, `deciding`, `hooks`, `watchdog`, `aae-claude`,
  `aae-codex` — 109 тестов), прогнаны 5 раз по два параллельных прогона. 10/10 зелёные, по 21 s.
- Вживую: демон в `/tmp` с изолированными `XDG_*`.
  - `[watchdog] human_timeout` → «unknown key `human_timeout`», демон не стартует.
  - exec-формула прошла.
  - `aae run answer` → «unrecognized subcommand».
  - `POST /v1/runs/{id}/answer` → 404.
  - в `run show --json` нет `request`.
  - `?state=requires_action` → 400 со списком из 10 состояний.
  - в `/v1/openapi.json` 0 упоминаний.
  - Демон остановлен через `aae drain`.
- Claude под root: `--permission-mode bypassPermissions` → exit 1 (Обяз. 1). `claude --help` 2.1.326 подтверждает
  `--permission-prompts none`.
- `git grep -nE 'requires_action|human_timeout|run answer'` вне `.loop/` и `experiments/` находит только
  `docs/agents/decisions.md:40`, текст решения 116.
- Русского в продукте и в сообщениях коммитов нет. `todo!`/`unimplemented!`, номеров циклов и пакетов в диффе нет.
  Новых зависимостей нет.
- Файлы ≤ 400 строк: самый большой `machine.rs` — 396.
- Бюджеты (`scripts/budget.sh`): core 10923/18000, claude 883/3500, codex 1134/3500, total 17407/30000.
- Строк по tokei (код Rust): ядро (`aae-core` + `aae-contract`) 14172, всего `crates/` 42291.
- `aae run watch` фильтрует `/v1/runs/{id}/live` только по `run`, так что событие туда попадает (код
  `crates/aae-front/src/api/live.rs:117`).

## Нарушения правил

- Нарушений решений владельца нет. Отступление от буквы пакета (codex: на `item/permissions/requestApproval` уходит
  ошибка, а не `decline`, потому что у ответа нет поля `decision` в схеме 0.168.0) кодер обосновал и записал в журнал.
  Принимаю.
- Правила тестов: инвариант 15 с выжившей мутацией (Обяз. 2).

## Следующий шаг

Следующему кодеру сначала два обязательных пункта, оба S-объёма:
- Обяз. 2 — один тест ядра.
- Обяз. 1 — до ответа владельца только документ: что под root дефолт claude не работает и что задать вместо него.
  Если владелец уже ответил, сделать по его ответу.

Попутно — Жел. 1–4, это мелкие правки тестов. Затем по плану **WP-96** (S). С оставшимся контекстом можно взять
**WP-97** (M), но только одним, без WP-96 в том же цикле, если контекста мало: «за цикл — один M или два S».
