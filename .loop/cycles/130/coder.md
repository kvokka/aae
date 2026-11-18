# Цикл 130 — coder
STATUS: done

## Взято в работу
- WP-129 (M): тесты не видят окружение внешнего loop-manager.
- WP-130 (S): таймаут ai-шага по умолчанию 4 h, exec — 45 m.
- WP-131 (S): claude 2.1.327 в проверенный диапазон (живой ход критерия 2 на haiku разрешён владельцем).
- WP-132 (S): заметка про `CLAUDE_CODE_ENTRYPOINT`, строка roadmap про mcp_agent_mail, сверка PreCompact.

## Сделано (коммиты)
- `f25d097` WP-129:
  - `loop_manager_testkit::without_outer_session` (`crates/loop-manager-testkit/src/outer.rs`) убирает унаследованные
    `LOOP_MANAGER_*` (кроме `LOOP_MANAGER_INSTANCE`), `BEADS_DIR`, `BD_ACTOR`, `BD_ISSUE_PREFIX`;
  - в `crates/loop-manager/tests/common`: `EXE`, `program(..)`, `loop_manager()`; через них идут все запуски бинаря, хука
    `context-budget`, `br` и `timeout` в e2e; ручные `env_remove` из `wait.rs` и `context_budget.rs` убраны;
  - раннер acceptance: `daemon.rs` `command()` и `br init`/`git init` в `c20_demo1.rs`, `c20_demo2.rs`;
  - сторож в `mise run ci`: `CARGO_BIN_EXE_loop-manager` вне `crates/loop-manager/tests/common` — ошибка;
  - регрессионный тест `cli::the_cli_tests_pass_inside_a_session_of_an_outer_loop_manager` (перезапуск своего бинаря с
    `--exact` двух тестов `cli.rs` под живым посторонним `UnixListener`, `RUN=run_outer`, `ATTEMPT_TOKEN=bogus`,
    `BEADS_DIR=/nonexistent`, проверка «2 passed»). **До правки красный**: оба теста «the daemon did not come up»
    за 60 s. После — зелёный;
  - доки: правило в `docs/agents/rules-tests.md` («How tests are written»), строка гейта в `docs/guide/development.md`.
  - Руками: `LOOP_MANAGER_SOCKET=<живой сокет python-листенера> LOOP_MANAGER_RUN=run_outer
    LOOP_MANAGER_ATTEMPT_TOKEN=bogus BEADS_DIR=/nonexistent BD_ACTOR=outer mise run ci` — exit 0, nextest 659/659
    за 29 s. Тестов в `nextest list --workspace`: 668 → 669 (+1).
- `081fbc3` WP-130: `AI_STEP_TIMEOUT_MS = 4 h`, `EXEC_STEP_TIMEOUT_MS = 45 m` (`run/render.rs`, `exec_step.rs`,
  `events.rs`); тесты: `watchdog::a_turn_past_the_step_timeout_is_a_runaway` теперь ждёт 04:00 без `timeout`,
  `the_step_timeout_pauses_between_turns` задаёт `timeout = "45m"` явно, новый
  `exec::an_exec_step_without_a_timeout_has_45_minutes` (строка `runs.timeout_ms`). Доки: contracts 6.1, config.md
  (пример формулы: умолчание у ai и у exec), frontends.md.
- `30fe549`, `c4bbc3d` WP-131: claude уже стоял 2.1.327 (`mise upgrade claude` — «up to date», `claude --version` =
  2.1.327). `cargo nextest run -p loop-manager-claude` 45/45, `-p loop-manager -E 'test(/^claude::/)'` 6/6. Живой ход
  критерия 2: `cargo build -p loop-manager && cargo run -p loop-manager-acceptance -- 2` — строка claude (haiku) **pass,
  4.6 s**, весь прогон 329 s. codex не трогался: `codex` на PATH подменён заглушкой `exit 1`, его строка — FAIL
  «parked» за 300 s без вызовов модели (у раннера нет фильтра по харнессу; запись в журнале). Диапазон
  `TESTED` → 2.1.327 в `crates/loop-manager-claude/src/lib.rs` и `docs/guide/install.md`, строка в журнале
  `docs/harness-updates.md` (время хода из строки убрал вторым коммитом: замер без маркера MEASURE).
- `29b9601` WP-132: заметка после абзаца про headless в `docs/guide/install.md` (метка, #97074 как замер сообщества,
  неподтверждённость нашим замером, как задать в `env` агента, `cli` не работает, условия вендора — на пользователе,
  скрытый запрос на имя сессии и `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`, почему не `DISABLE_NONESSENTIAL_TRAFFIC`;
  маркеры MEASURE на оба замера); ссылка из допущения в `docs/agents/assumptions.md` (строка «Subscription policy»);
  строка «Agent mail addon» в `docs/roadmap.md` (Low). PreCompact: `experiments.md:475`,
  `addon-development.md:92,204`, `addons/context-budget/README.md:42`, `config.md:56` говорят одно (в субагенте есть
  `agent_id`) — правок не нужно.
- Гейт: `scripts/loop/checks 130` на HEAD `29b9601` — pass, 39 s, nextest 660/660.

## Не получилось и почему
Нет. Находки для владельца — в журнале (нет фильтра по харнессу в раннере acceptance; `LOOP_MANAGER_SOCKET` слушает
и демон, а `frontends.md` говорит только о клиентах).
## Начато и не закончено (файлы, точка остановки, следующий шаг)
Нет. Следующие по плану: WP-133, WP-135, затем WP-134.
## Замечания предыдущего ревью: что закрыто, что нет
«Обязательно» — нет. «Желательно» (артикль в `retired.rs`, `checks.rs:319`) не брал: эти файлы пакеты цикла не трогали.
## Контекст на момент отчёта: ~165k
