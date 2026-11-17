# Цикл 110 — coder
STATUS: done

## Взято в работу
- Ревью 109, «Обязательно» 1 (claude под root) и 2 (сброс silence при `permission_denied`).
- Ревью 109, «Желательно» 1–4.
- WP-96 (S): формула задаёт `native.<harness>`.

По правилу плана «за цикл — один M или два S» больше не брал: WP-97 — это M, а WP-98 от него зависит.

## Сделано (коммиты)
- `32e89ce` — тест `watchdog::a_denied_permission_is_activity_for_the_watchdog`:
  - в `running` денай на 50 s сдвигает `stall` с 1:00 на 1:50;
  - в `starting` событие пишется в 0:20, а `start_timeout` остаётся 0:30. Это закрывает и Жел. 4.
  - Мутация из ревью (`"running" => Ok(())`) краснит тест: left 1:00, right 1:50. Мутацию откатил.
  - Попутно Жел. 1: из комментария `control.rs` убрал упоминание `respond`.
- `29676a8` — Обяз. 1, только документы, до ответа владельца:
  - `docs/design/config.md` `### Permissions` и contracts 4.6 теперь говорят, что от root claude отклоняет
    `bypassPermissions` и выходит до `init`.
  - Что задать вместо: `env = { IS_SANDBOX = "1" }` или другой `native.claude.permission_mode` (`dontAsk`,
    `acceptEdits`) вместе с `permissions.allow`.
  - Проверил без модели на 2.1.326 (base URL на закрытый порт): с `IS_SANDBOX=1` CLI доходит до `init`, `dontAsk` под
    root тоже проходит. `env` настроек действительно попадает в процесс claude (`part::env`).
  - Запись в журнале.
- `a7c3d48` — Жел. 2: в `a_control_request_gets_an_error_at_once` ожидание обёрнуто в `timeout` 10 s. Проверил
  мутацией (ответ не пишется): тест падает за 10 s с «the request got no answer», а не виснет. Мутацию откатил.
- `5e86eed` — Жел. 3: контрактный s13 claude (настоящий CLI, фейковый API) теперь проверяет, что пришёл ровно один
  `PermissionDenied{Bash, tool_use_id: Some}`.
  - Вживую событие приходит по system-пути: `input: null`, `reason: "asyncAgent"`. Находка записана в журнал.
- `7362724` — WP-96: у ai-шага формулы появился ключ `native.<harness>`.
  - Слой 8 уже существовал, нового механизма слоёв нет.
  - Pack `[settings]` по-прежнему принимает только три launch-настройки: в пакете он не назван.
  - Тест `config_launch::a_formula_step_sets_the_native_settings_of_its_agent` проверяет:
    - шаг переопределяет `native.claude.permission_mode` и `native.codex.approval_policy` проекта;
    - `explain` показывает `# project + formula kit.implement step 1`;
    - правка `[rigs.backend.agents.implementer]` бьёт шаг;
    - `native.pi` даёт ошибку загрузки.
  - Обновлены `docs/design/config.md`: settings, Permissions, слой 8, пример формулы.
- Гейт: `scripts/loop/checks 110` на HEAD `7362724` — pass, 45 s, nextest 546/546.

## Не получилось и почему
- Жел. 5 (фиксированные 200 ms в `no_prompt_goes_into_a_session_a_stopped_writer_never_recorded`) не трогал. Тест
  проверяет, что события нет, а положительного сигнала, которого можно было бы дождаться вместо паузы, у драйвера нет.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Ничего. Следующий по плану — WP-97 (M), его стоит брать одним в цикле.

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыто документом, как требовало ревью. Вопрос владельцу (журнал 07:25 и 10:20) открыт: ставить ли
  адаптеру самому `IS_SANDBOX=1` под root.
- Обяз. 2 — закрыто тестом, мутация краснит.
- Жел. 1–4 — закрыты.
- Жел. 5 — открыто, причина выше.
- Жел. 6 — принято ревьюером, работы нет.

## Контекст на момент отчёта: ~125k
