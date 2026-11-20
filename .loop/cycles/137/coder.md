# Цикл 137 — coder
STATUS: done

## Взято в работу
- Ревью 136, «Обязательно 1»: русские заголовки в промптах пака `devloop`.
- Ревью 136, «Желательно» 1 (тест предела повтора `thread/resume` в codex) и 3 (первая фраза `devloop/README.md`).

## Сделано (коммиты)
- `2abe7f4` WP-142: заголовки отчётов в `devloop/agents/coder/prompt.md` и `devloop/agents/reviewer/prompt.md`
  снова английские (как на базе `d92f5bc`: «Mandatory before going on», «Next steps», «Done (commits)», …); оба
  промпта прямо говорят, что текст комментария по-русски, а заголовки — по-английски («the headings, in English,
  exactly so»). `git grep -lP '[\x{0400}-\x{04FF}]' -- devloop` пуст. В README «Nothing here is part of the
  product» → «Nothing here ships with the binary».
- `a45ef90` codex: ожидание закрытия треда — поле `unload_wait` адаптера (10 s, как было константой `UNLOAD_WAIT`);
  фейк app-server в `crates/loop-manager-codex/src/tests.rs` вынесен в `refusing_codex(dir, always)`, новый тест
  `a_resume_refused_past_the_unload_wait_fails`: фейк отказывает всегда, `unload_wait` = 300 ms, ответ — ошибка
  отказа, а не зависание (таймаут теста 5 s). Мутация «убрать `&& Instant::now() < until`» убивается (тест красный
  по таймауту); тесты крейта прогнаны 5 раз подряд — 49/49 каждый раз.

## Гейт
`scripts/loop/checks 137` отказывает: «есть незакоммиченные изменения вне .loop» — это чужая правка
`.claude/settings.json` (глубина субагентов), шестой цикл подряд; не моя, харнесс трогать нельзя. Поэтому
`mise run ci` запущен напрямую на HEAD `a45ef90`: exit 0, 41 s, nextest 702/702, `budget-selftest: ok`,
`devloop selftest: ok`. Итог — `.loop/cycles/137/notes/ci-head.txt`, лог — `notes/ci.log`. `checks.md` нет.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
— Все пакеты плана закрыты. Дальше нужен ответ владельца на открытые вопросы в журнале (кеш промптов 5m/1h,
`soft`/`hard` для codex, прогон codex 0.170.0 на модели для `TESTED`, правка `.claude/settings.json`, переезд на
`devloop`).

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательно 1 (русский в `devloop/`) — закрыто, `2abe7f4`.
- Желательно 1 (тест предела повтора resume) — закрыто, `a45ef90`, мутацией подтверждено.
- Желательно 2 (`--amend`) — принято; в этом цикле без amend, правки — отдельными коммитами.
- Желательно 3 (README) — закрыто, `2abe7f4`.
- Находка: в индексе лежат файлы `.loop/cycles/136/notes/wp142-sketch/*.sh` с шебангом без бита исполнения —
  pre-commit хук `check-shebang-scripts-are-executable` на них падает, если коммитить весь индекс. Я коммитил
  только свои пути (`git commit -- <пути>`); записано в журнал.

## Контекст на момент отчёта: ~45k
