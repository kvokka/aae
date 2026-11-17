SCORE: 6.6/10
VERDICT: continue
BLOCKERS: 0

# Ревью цикла 110

Дифф `9f70a3d..HEAD`: 5 коммитов, 10 файлов, +107/−10. В цикле закрыты Обяз. 1–2 и Жел. 1–4 ревью 109 и сделан
WP-96. Вне `.loop` дерево чистое. Незакоммичены только штатные файлы цикла: `JOURNAL.md`, `PLAN.md`, `state.json` и
`cycles/110/`.

`checks.md`: HEAD `7362724` совпадает с текущим, RESULT pass, 45 s, nextest 546/546. Гейт не перезапускал.

## Обязательно до продолжения

Нет.

## Желательно

1. Три файла почти упёрлись в лимит 400 строк: `crates/aae-contract/src/config/items.rs` (397),
   `crates/aae-claude/src/tests.rs` (398), `crates/aae-claude/tests/harness/contract.rs` (398). Следующая правка в
   любой из них должна начинаться с разбиения файла, иначе выйдет нарушение правила.
2. `crates/aae-core/src/run/events.rs:185-188`: ветка `match row.state { "running" => silence, _ => Ok(()) }`.
   Если ставить silence во всех состояниях, тест всё равно зелёный, потому что мутация эквивалентна: дедлайн
   `silence` срабатывает только в `running` (`events.rs:293`), а при ack его перевзводят. Код верный, но `match`
   можно убрать или пояснить, что он ничего не меняет. Дефекта нет.
3. Гейт вырос с 31 до 45 s. Сам nextest почти не изменился (24.7 → 25.1 s), значит, рост пришёлся на пересборку:
   цикл трогал `aae-contract`, от которого зависит почти всё. Причина понятна, но в следующем цикле время стоит
   проверить ещё раз.
4. Остаётся открытым прошлое Жел. 5: фиксированные 200 ms в
   `no_prompt_goes_into_a_session_a_stopped_writer_never_recorded`. Объяснение кодера (тест проверяет, что события
   нет, а положительного сигнала у драйвера нет) принимаю. Пункт можно закрыть, когда у драйвера появится такой
   сигнал.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-93 | done | — |
| WP-94 | done | Обяз. 1 ревью 109 закрыт в документах (config.md `### Permissions`, contracts 4.6). Сам вопрос (ставить ли адаптеру `IS_SANDBOX=1` под root) ждёт владельца, журнал 07:25 и 10:20 |
| WP-95 | done | Обяз. 2 ревью 109 закрыт: тест на сброс silence краснеет под мутацией. Жел. 3 закрыто: s13 проверяет один `PermissionDenied` на настоящем CLI |
| WP-96 | done | Все критерии выполнены. Шаг формулы задаёт `native.<harness>` в существующем слое 8. Pack `[settings]` по-прежнему принимает только три launch-настройки, пакет большего не требовал |
| WP-97…WP-101 | todo | — |

## Проверено

- Гейт: `checks.md` — HEAD `7362724`, pass, 45 s, nextest 546/546. Не перезапускал.
- Мутации. После каждой откат `git checkout`, `git status` показывает изменения только в `.loop`:
  - `events.rs:186`, `"running" => Ok(())` (денай не сбрасывает silence): тест
    `watchdog::a_denied_permission_is_activity_for_the_watchdog` краснеет, left 1:00, right 1:50;
  - `events.rs`, silence при денае в любом состоянии: тест зелёный, мутация эквивалентна (Жел. 2);
  - `items.rs`, `native` шага отброшен: тест `config_launch::a_formula_step_sets_the_native_settings_of_its_agent`
    краснеет, left "default", right "dontAsk".
- WP-96 по критериям пакета:
  - шаг переопределяет `native.claude.permission_mode` и `native.codex.approval_policy` проекта;
  - `[rigs.backend.agents.implementer]` бьёт шаг (решение 71);
  - `explain` показывает `project + formula … step 1`;
  - `native.pi` даёт ошибку загрузки.
- Документы сверены с кодом: config.md (раздел settings, Permissions, слой 8, пример формулы) и contracts 4.6.
  Утверждений не ослаблено, истории в тексте нет. Решения 71, 90 и 120 не нарушены.
- Утверждение документа про root проверил вживую, без модели (claude 2.1.326, base URL на закрытый порт, `sudo -n`).
  `bypassPermissions` → «--dangerously-skip-permissions cannot be used with root/sudo privileges». С `IS_SANDBOX=1`
  claude доходит до `system/init`. `env` сессии передаётся процессу claude: `part.rs:58`, `part.rs:245`.
- Проверил пользовательский путь вживую: демон в `/tmp`, все `XDG_*` и `HOME` изолированы. Проект с
  `[settings.native.claude] permission_mode = "default"` и формула со step `native.claude = { permission_mode =
  "dontAsk" }`.
  - `aae config explain --formula summarize` → `native.claude = {"permission_mode":"dontAsk"}  # project + formula
    hello.summarize step 1`.
  - `--agent helper` → `default  # project`.
  - `native.pi` в шаге → `aae config check` сообщает «`native.pi`: no harness `pi`…».
  - Демон после проверки остановлен.
- Флейки: тесты, которые трогает дифф, гонял 3 раза по два параллельных прогона, все зелёные:
  - `watchdog::*`, `config_launch::*` и lib-тесты `aae-claude` — 33 теста;
  - `contract::s13` claude и codex.
- Строк по tokei (код Rust): ядро (`aae-core` + `aae-contract`) 14215, всего `crates/` 42371 (в цикле 109 было
  14172 / 42291).
- `todo!`/`unimplemented!` в коде нет. Новых зависимостей нет.

## Нарушения правил

Нет. Код, документы и коммиты на английском. Заглушек и кода «на вырост» нет.

## Следующий шаг

WP-97 (M, классификация отказов по структурным полям). Брать одним пакетом в цикле: от него зависит WP-98. Если к
концу останется бюджет — WP-99 (S, не зависит от WP-97). Перед правкой `items.rs` или тестов `aae-claude` файл
сначала разбить (Жел. 1).
