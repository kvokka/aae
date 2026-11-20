SCORE: 7.0/10
VERDICT: continue
BLOCKERS: 2

# Ревью цикла 135

Дифф `8195710..d92f5bc`: 3 коммита, 16 файлов, +593/−71.
- `a0d9b7c` — WP-141, аддон `dcg`.
- `aa0cbcf` — «Желательно» 1–4 и 6 из ревью 134.
- `d92f5bc` — WP-140, `context-budget` для codex.

Зависимостей в `Cargo.toml`/`Cargo.lock` нет. В `mise.toml` добавлен `dcg` 0.16.0 — версию назвал владелец в пакете.
Незакоммиченное вне `.loop` — только чужая правка `.claude/settings.json`: глубина субагентов 2 → 4, как и в цикле 134.
Поэтому `scripts/loop/checks 135` отказывается запускаться и `checks.md` за цикл нет. Кодеру это не вменяю: ни
закоммитить, ни откатить файл харнесса он не может. Защиту скрипта я не обходил. Гейт прогнал сам один раз,
`mise run ci` напрямую на HEAD, и он **красный из-за флейка** (пункт 2).

## Обязательно до продолжения

1. **Флейк в новом тесте WP-141: `dcg::without_dcg_on_the_path_every_command_is_refused_and_the_agent_is_told_why`.**
   - Где: `crates/loop-manager/tests/e2e/dcg.rs:73-78`.
   - Что происходит: в ветке «нет `dcg`» хук `addons/dcg/hooks/guard` выходит с кодом 2, не читая stdin. Тест пишет вход
     в трубу после `spawn` и делает `unwrap()`. Если хук успел выйти раньше, запись получает
     `Os { code: 32, kind: BrokenPipe }`, и тест падает на `dcg.rs:78:10`.
   - Частота: 8 падений из 15 прогонов фильтра `context_budget|dcg` подряд и ещё 1 из 5 при прогоне базы между
     мутациями. В гейте кодера он прошёл случайно.
   - Как починить: в `guard()` не падать на ошибке записи (`let _ = ...write_all(...)`; тест проверяет код выхода и
     stderr) или сначала читать вход в хуке. Первое проще и продукт не меняет.
   - Как проверить: `for i in $(seq 1 30); do cargo nextest run -p loop-manager -E 'test(/^dcg::/)' ...; done` — ни
     одного падения.
2. **Гейт на HEAD `d92f5bc` красный: флейк в адаптере codex.**
   - Упал `loop-manager-codex::harness codex::a_thread_resumed_on_its_live_app_server_gets_the_new_attempt_token`
     (`crates/loop-manager-codex/tests/harness/codex.rs:214`). Ошибка от codex на `thread/resume`: `thread … is
     closing; retry thread/resume after the thread is closed` (`-32600`). Адаптер превращает её в `Failure` вида
     `Unknown`.
   - Лог: `.loop/cycles/135/notes/review-ci.log`, итог — `notes/review-ci-head.txt`. nextest 176/177 до остановки:
     после падения прогон отменён.
   - Этот цикл код не трогал. Вероятная причина — установлен codex 0.170.0, а в `docs/harness-updates.md`
     проверенный диапазон кончается на 0.168.0. По этой ошибке `unsubscribe` уже вернул `notLoaded`, а поток ещё
     закрывается. Значит, это гонка в `resume_session` (`crates/loop-manager-codex/src/lib.rs:237-251`), то есть в
     продукте, а не только в тесте: возобновление сессии на живом app-server может упасть.
   - Повторить мне не удалось: 15 прогонов теста по одному и 6 прогонов под нагрузкой — все зелёные. Падение редкое,
     но «флейк — дефект».
   - Что сделать: в `resume_session` повторять `thread/resume` на ответ «is closing» в пределах того же
     `UNLOAD_WAIT`. Нужен тест (фейк или контрактный сценарий) и строка 0.170.x в таблице `docs/harness-updates.md`
     после зелёного контрактного набора.

## Желательно

1. По умолчанию `soft`/`hard` (300000/420000) на codex не работают. Окно моделей codex — 272000, сжатие не позже
   244800. Поэтому агент codex с настройками по умолчанию заметки не получит и будет остановлен около 245k. Это
   расходится с духом решения 149: «за `soft` агент получает заметку». README это честно описывает, кодер вынес
   ВОПРОС в журнал и до ответа взял вариант (а). Ждёт владельца, не кодера.
2. Живьём (на настоящем codex) ничего из WP-140 не проверено. Всё взято из исходников codex:
   - что `PreCompact` с `continue:false` сохраняет контекст и завершает ход;
   - что codex отвергает `continue:false` в `PreToolUse`;
   - что `additionalContext` из `PostToolUse` доходит до модели;
   - потолок сжатия 90% окна;
   - собственный `transcript_path` у дочернего потока.

   В README и `experiments.md` об этом сказано («by its source; not measured»). Когда владелец разрешит прогон
   codex, это первые кандидаты на замер с маркером `MEASURE`.
3. `docs/design/config.md:63-65`: новое предложение об ответах хуков codex стоит в абзаце с пометкой «by its source
   at 0.166.0», а проверено на исходниках `main d5a478e` / 0.170.0. Версию стоит уточнить.
4. `crates/loop-manager/tests/e2e/context_budget.rs` — 396 строк из 400. Следующая правка тестов claude упрётся в
   предел. Удобно заранее вынести общую часть `Bed` в `context_budget/bed.rs`, как уже вынесен `codex.rs`.
5. Из ревью 134 остались «Желательно» 5 (пример демо 4) и 7 (длинные строки в `decisions.md` и README демо 4). Кодер
   правильно их не тронул: `demos/` и `decisions.md` правятся только по просьбе владельца.

## Пакеты плана

| пакет | статус | чего не хватает |
|---|---|---|
| WP-136…WP-139, WP-143 | done | — (закрыты в прошлых циклах) |
| WP-140 `context-budget` для codex | done | Поведение и тесты — по всем случаям WP-139. README «On codex» перечисляет 7 расхождений, журнал — РЕШЕНИЕ/ВОПРОС/НАХОДКА. Гейт на HEAD красный по чужому флейку (обязательно 2) |
| WP-141 аддон `dcg` | done, но с дефектом | тест «нет `dcg`» флейкает на BrokenPipe (обязательно 1) |
| WP-142 пак `devloop` | todo | весь пакет: критичный, M, начинать с плана expert |

## Проверено

- **Гейт.** `scripts/loop/checks 135` отказался запускаться («есть незакоммиченные изменения вне .loop») из-за
  `.claude/settings.json`. Тогда `mise run ci` напрямую на HEAD `d92f5bc`, один раз: **FAIL**, 13 s до отмены. nextest
  177/699: 176 pass, 1 fail (обязательно 2). У кодера на том же HEAD было: exit 0, 40 s, 699/699
  (`notes/ci-head.txt`). Время в норме: в цикле 134 было 44 s.
- **Строки (tokei, по логу кодера):** ядро 12233 / 18000, аддоны 363 / 1500, всего 19192 / 30000. Файлы ≤ 400 строк:
  `budget` — 186, `context_budget.rs` — 396, `codex.rs` — 232, `dcg.rs` — 134. Тестов 699.
- **Мутации** (фильтр `context_budget|dcg`, 14 тестов; после каждой мутации `git checkout --`, дерево чистое). Каждая
  мутация краснит 1–3 теста:
  - `*spawn_agent` убран из списка инструментов субагента;
  - `transcript()` без ветки codex;
  - дамп без `function_call`/`custom_tool_call`;
  - дамп без `output_text`;
  - убран `cut_off` в `PreCompact` для codex;
  - `ends` всегда `true`;
  - счёт по `total_token_usage`;
  - `deny` по умолчанию с `continue`;
  - манифест без `native.codex.model_auto_compact_token_limit`;
  - `dcg`: `matcher = "Shell"`;
  - `dcg`: `exec dcg` → `exit 0`.

  Мутации по пунктам ревью 134 теперь тоже убиваются: отсечка в `PreToolUse` (3 падения), запись `.base`, `elif
  true` для прогона без бида, неверный `cut` → `exit 1`.
- **Флейки.** База фильтра `context_budget|dcg` — 20 прогонов: `dcg::without_dcg…` падал 9 раз. Тесты
  `context_budget::*` ни разу не упали. Тест codex `a_thread_resumed…`: 15 прогонов по одному и 6 под нагрузкой
  (параллельно с `-p loop-manager-codex -p loop-manager-claude`, 92 теста) — все зелёные.
- **Вживую, без модели.** Изолированные `HOME`/`XDG_*` в `mktemp -d`, конфиг с обоими аддонами, агент codex с
  `params = { soft = 180000, hard = 240000 }`:
  - `loop-manager config check` чистый;
  - `config explain` показывает `native.codex = {"model_auto_compact_token_limit":240000}` (число, не строка), хук
    `dcg` (`PreToolUse`, matcher `Bash`) и хуки `context-budget`.
  - `addons/dcg/hooks/guard` руками на входе codex (`git clean -fdx`): deny с причиной в протоколе харнесса.
  - Без `dcg` в PATH: exit 2 и понятная причина в stderr.
- **Документы.** `docs/` сверены с кодом. Русского в диффе нет, истории в `docs/` нет. `decisions.md` и
  `assumptions.md` не тронуты.
- **Предыдущее ревью.** Обязательных пунктов не было. «Желательно» 1–4 и 6 закрыты и подтверждены мутациями; 5 и 7
  законно оставлены.

## Нарушения правил

- Кодер правил не нарушал.
- Харнесс, ждёт владельца: третий цикл подряд (133–135) в дереве висит чужая незакоммиченная правка
  `.claude/settings.json` (`CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` 2 → 4). Из-за неё `scripts/loop/checks` не работает и
  гейт цикла не фиксируется в `checks.md`.

## Следующий шаг

1. Сначала оба обязательных пункта, это S:
   - флейк `dcg.rs` — несколько строк;
   - повтор `thread/resume` на «is closing» в `crates/loop-manager-codex/src/lib.rs` с тестом; строка codex 0.170.x
     в `docs/harness-updates.md`, если контрактный набор codex зелёный.
2. Затем WP-142 (пак `devloop`, M, критичный), с плана expert. На один цикл это ~150k. Если обязательные пункты съели
   заметную часть бюджета, WP-142 лучше начать с нового цикла.
