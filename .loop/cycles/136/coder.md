# Цикл 136 — coder
STATUS: done

## Взято в работу
- «Обязательно до продолжения» ревью 135: флейк теста `dcg` (BrokenPipe) и гонка `thread/resume` в адаптере codex.
- «Желательно» 3 и 4 ревью 135.
- WP-142 (пак `devloop`: цикл implement/review по эпику) — по плану expert
  (`.loop/cycles/136/notes/wp142-plan.md`, эскизы — `notes/wp142-sketch/`).

## Сделано (коммиты)
- `06af450` WP-141: хелпер `guard()` в `crates/loop-manager/tests/e2e/dcg.rs` не падает на записи в закрытую трубу
  (хук без `dcg` выходит, не читая вход). 30 прогонов `test(/^dcg::/)` подряд — все зелёные.
- `204b76e` codex: `resume_session` повторяет `thread/resume`, пока codex отвечает «retry thread/resume after the
  thread is closed», в пределах того же `UNLOAD_WAIT` (10 s). Тест на сценарном фейке app-server (sh-скрипт):
  `crates/loop-manager-codex/src/tests.rs` `a_resume_refused_while_the_thread_closes_is_retried`; без правки падает.
  Контрактный набор codex на 0.170.0 зелёный (48/48, `codex::` демона 5/5), `a_thread_resumed…` — 15/15 по одному.
  `docs/design/contracts.md` (строка Resume); `docs/harness-updates.md` — строка лога 0.170.0: набор зелёный,
  настоящего хода не было, диапазон `TESTED` не расширен (по процедуре нужен реальный ход модели — без владельца
  нельзя; НАХОДКА в журнале). «Желательно» 3: `config.md` — версия 0.170.0 у фразы об ответах хуков codex.
- `9ead29b` «Желательно» 4: стенд `Bed` и помощники — в `crates/loop-manager/tests/e2e/context_budget/bed.rs`
  (`context_budget.rs` 396 → 242 строки); `context_budget::` 11/11.
- `377810d` WP-142 (продукт): статус бида — текст трекера, включая статус из политики проекта (`rework`):
  `contracts.md` 5.2, rustdoc `Bead::status`, `tracker-br.md`; контрактный тест `br`
  `a_status_of_the_project_policy_passes_release_and_close_as_text` (строгая политика: claim → rework → release даёт
  `open` без исполнителя; claim → rework → close даёт `closed`). Мутация (release без `-s open`) — тест красный.
- `20bfdae` WP-142 (пак): формула `epic` (`git mv` из `task.toml`): `code` → `tests` (exec, всегда exit 0, выжимка с
  `RESULT/HEAD/UNCOMMITTED/LOG` идёт ревьюеру как `steps.tests.data.stdout`, лог — `target/devloop/ci.log`) →
  `review` (`max_attempts = 2`, оценка и вердикт — `finish --data`, ревью — комментарий `REVIEW` в эпик) → `gate`
  (`back_to = "code"`: `rework` при возврате, коммит только `devloop/.beads`, push после каждого ревью, проход при
  `complete` и ≥ 9, 75 — push не вышел), `rounds = 50`, `last_comments = 8`. `bead_failed_runs = 1`.
  `.beads/policy.yaml` (strict, шесть статусов). `[settings]`: `RUSTUP_TOOLCHAIN/RUSTUP_HOME/CARGO_HOME` ссылками,
  `BR_OUTPUT_FORMAT=toon`, `native.claude.promptCacheTtl/subagentPromptCacheTtl = "5m"`; `inherit_env` — только на
  `tests` и `gate`. Аддоны `context-budget` (кодер `cut = "pass"`) и `dcg` — локальным путём, в `agent.toml` кодера и
  ревьюера. Роли кодера и ревьюера переписаны (эпик и комментарии, разбор чужого незакоммиченного, итог комментарием
  `CODER`, вопрос — `QUESTION` + `needs-owner`, правило «правка пака ждёт владельца» убрано), субагенты — пути
  `target/devloop/notes/`. Удалены `check-report.sh`, `check-review.sh`, `.loop/runs`. README пака переписан
  (subcoder). Решение 150 в `decisions.md` (раздел Process). `selftest.sh` переписан на заглушки `br`/`mise`;
  мутации `>= 9 → >= 8` в `gate.sh` и «всегда pass» в `tests.sh` им убиваются. `config check` чистый.
- `33a9440` WP-142: rustdoc `Bead::status` без имени `br` (иначе `scripts/core-boundary.sh` красный). Этот коммит я
  один раз поправил `--amend` (перенос строки rustdoc) сразу после создания, до любого пуша.

## Гейт
`scripts/loop/checks 136` отказался запускаться («есть незакоммиченные изменения вне .loop») — из-за чужой правки
`.claude/settings.json`, как в циклах 133–135. Поэтому `mise run ci` прогнан напрямую на HEAD `33a9440`: exit 0,
41 s, nextest 701/701, `devloop selftest: ok`, `config check` чистый. Итог — `notes/ci-head.txt`, лог — `notes/ci.log`.
Первый прогон (на `20bfdae`) был красным только из-за `core-boundary` — исправлено в `33a9440`.

## Не получилось и почему
- Диапазон `TESTED` codex не расширен до 0.170.0: нужен реальный ход модели (журнал).
- Живьём пак не запускался (пакет запрещает). Не проверено без живой сессии: TTL кеша claude, передача `--data`
  ревьюером, поведение хуков старого лупа в сессиях пака.

## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет.

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательно 1 (флейк `dcg`) — закрыто, `06af450`.
- Обязательно 2 (гонка resume codex) — закрыто, `204b76e`; диапазон не расширен (нужен реальный ход).
- Желательно 3, 4 — закрыты. 1, 2 — ждут владельца. 5 — законно оставлено (демо, `decisions.md`).

## Журнал
НАХОДКА про codex 0.170.0 и `TESTED`; НАХОДКА — 8 ограничений пака (br/продукт); ВОПРОС — кеш 5 минут vs 1 час
(принято 5 минут).

## Контекст на момент отчёта: ~150k
