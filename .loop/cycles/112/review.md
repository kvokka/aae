SCORE: 6.3/10
VERDICT: continue
BLOCKERS: 0

# Ревью цикла 112

Дифф `299507a..7134d4c`: 4 коммита кодера, 13 файлов, +392/−5. Это WP-97 (добивка), WP-98 (добивка), WP-99 и
WP-100. Вне `.loop` дерево чистое; незакоммичены только `.loop/` (план, журнал, `state.json`, `cycles/112/`), как и
положено до `loopctl`.

`checks.md` снят на текущем HEAD `7134d4c`: pass, 36 s, nextest 557/557. Сам гейт я не перезапускал.

## Обязательно до продолжения

Нет.

## Желательно

1. **Внеходовые события субагента могут сбросить фоновый бюджет до `silence`.** Кодер правильно глушит `Activity`
   вне хода: иначе каждый шаг субагента возвращал бы watchdog с 120 min на 15 min. Но в ядре
   (`crates/aae-core/src/run/events.rs:191-229`) любое другое событие в `running` тоже ставит `silence_ms`. Это
   ветка `_ =>` для `Usage` из `rate_limit_event`, `Retrying` (`silence_ms + delay`) и `PermissionDenied`. Если
   субагент после `result` получит `api_retry` или `rate_limit_event`, бюджет фона сожмётся до 15 min. Стенд с
   фейковым API таких строк не дал, так что риск не проверен. Варианты: в адаптере не отдавать вне хода `Retrying` и
   `Usage` без токенов, или в ядре держать `background_silence`, пока последнее `State` было
   `Idle{background:true}`. Покрыть тестом.
2. **Мутация пережила замену проверки `local_agent`.** `self.agent = !tasks.is_empty()` в
   `crates/aae-claude/src/stream.rs:113` оставляет весь `aae-claude` зелёным (41/41). Почти эквивалентно: при одном
   фоновом `local_bash` claude сам шлёт `idle` с background. Но в тесте не хватает строки: после `result` при одном
   `local_bash` дополнительного `State` нет.
3. **Код стенда WP-99 лежит в `.loop/cycles/112/notes/`:** `wp99_stand.py` и `wp99-logs/*.ndjson`, около 100 KB. Его
   закоммитит `loopctl` вместе с `.loop`. План (WP-99) и решение 54 говорят: «код стенда не коммитить». Прецедент
   уже был (`cycles/98/notes/wp83_*.py`), но формулировку плана это нарушает. Скрипт и логи стоит убрать до коммита
   цикла или перенести вне репозитория; итог уже записан в `wp99-stand.md` и в `experiments.md`.
4. **Жел. 2 ревью 111 (путь `held` в `crates/aae-codex/src/lib.rs`) по-прежнему без теста.** Объяснение в журнале
   (codex 0.168 в 5 из 5 прогонов шлёт отказ после `turn/start`) убедительно. Остаётся вариант с фейковым
   app-server, если он появится.
5. **Текстовый `aae run list` не показывает `class`.** Гайд честно ведёт через `run show`, но для разбора нескольких
   упавших прогонов колонка `class` сэкономила бы шаг. Это не требование плана.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-87…WP-96 | done | — |
| WP-97 | done | Обяз. 1 ревью 111 закрыт: мутация `401|403→401` вместе с выключенным `AUTH` краснит `a_logged_out_retry_fails_at_once_and_an_overload_does_not`. Жел. 3 закрыт (`subtype=`/`api_error_status=`), мутация краснит тест |
| WP-98 | done | Жел. 1 ревью 111 закрыт: `required = false` краснит s16 в 3 из 3 прогонов |
| WP-99 | done | Стенд и замер с `MEASURE` есть. Адаптер выдаёт `Idle{background:true}` после `result` при живом `local_agent`; ядро на этом взводит `background_silence` (покрыто `watchdog.rs`). Жел. 1–3 |
| WP-100 | done | Аудит: кода не нужно. 4 теста на фейковом драйвере (S1–S4), гайд, ссылки. Расхождения с текстом плана (`--continue-from`, S4 через `gate close`, S3 отдельным агентом) записаны в журнал и соответствуют коду |
| WP-101 | todo | — |
| WP-102…WP-108 | todo | — |

Закрыто 14 пакетов из 22.

## Проверено

- **Гейт.** Взят из `checks.md`: HEAD `7134d4c` совпадает с текущим, pass, 36 s (в цикле 111 было 30–35 s, рост в
  пределах шума), nextest 557/557. Бюджет по tokei: ядро 10930 / 18000, claude 966, codex 1164, всего 17527 / 30000.
  Файлы ≤ 400 строк (самый большой в диффе — `stream.rs`, 291). `todo!`/`unimplemented!` нет, русского в продукте и
  в сообщениях коммитов нет, новых зависимостей нет.
- **Мутации.** Каждую откатывал через `git checkout`, после каждой проверял `git status`.
  - `if self.agent` → `if false && …`: краснеет `a_background_subagent_keeps_the_session_idle_with_background…`.
  - `!self.open ||` в `activity` → `false`: краснеет тот же тест.
  - `Some(401 | 403)` → `Some(401)` вместе с выключенным `AUTH.contains`: краснеет `a_logged_out_retry…`.
  - Строка `("subtype", …)` → `None`: краснеет `an_unknown_failure_keeps_the_words…`.
  - `part.rs:157` `required` → `false`: s16 красный в 3 из 3 прогонов. Без мутации s16 зелёный 3 из 3.
  - `machine.rs` `(0, _) => from` → `None`: краснеет `manual_recovery::a_run_that_failed_on_an_attempt_is_continued…`.
  - Мутация `local_agent` → `!tasks.is_empty()` выжила (Жел. 2).
- **Флейки.** Затронутые тесты (8 штук: manual_recovery ×4, subagent, logged_out, s16, unknown) прогнаны 10 раз
  параллельно с полным `cargo nextest run --workspace`: 10/10 зелёные. Полный прогон рядом тоже зелёный: 567/567,
  59.7 s под нагрузкой.
- **Вживую, без модели.** Демон в `/tmp/aae-rev112.*` с изолированными `HOME`/`XDG_*`, проект `shop`, exec-формула
  `false`. Пройдено:
  - `run start` → `failed`;
  - `run list --state failed` и `--limit 1 --json` (`attempts[]` с `kind`, `failure_kind`, `session` есть);
  - `run show` (`class: agent`, `reason`);
  - `run cancel` упавшего → `409 rejected`, как в гайде;
  - `gate close bead:shop/bd-1 --reason abandoned` → `gate list` показывает гейт → `gate open`;
  - `run start … --continue-from run_1` на exec-формуле → `rejected` («no session to continue»), что согласуется с
    фразой гайда «first step must be `ai`»;
  - `run list --parent run_1` работает.

  Флаги CLI из гайда (`--continue-from`, `--state`, `--limit`, `--parent`, `--reason`) существуют. Демон остановлен.
- **Документы и код.** contracts 4.6 (claude), experiments «A background subagent after the result» (с `MEASURE`) и
  `guide/recovery.md` сверены с кодом. Классы `agent|check|hollow_done|infrastructure` совпадают с contracts §1. Ни
  одно утверждение не ослаблено, истории в `docs/` нет. Ссылка `src=.loop/cycles/112/...` в `MEASURE` встречается и
  раньше (цикл 98).
- Решения владельца не тронуты, допущения не менялись.

## Нарушения правил

Нарушений в продукте нет. Есть замечание по `.loop`: код стенда WP-99 лежит в `notes/` и попадёт в коммит цикла
(Жел. 3).

## Следующий шаг

WP-101: скилл восстановления, плагин Claude Code и Codex, размер S. Он опирается на готовый `docs/guide/recovery.md`
и помещается в один цикл; если останется бюджет, можно взять Жел. 1–2 (небольшие тесты в `aae-claude`). WP-102
(`await`, M, критичный) брать отдельным циклом с полным бюджетом: он задевает конфиг, движок, API, CLI и документы.
