# WP-94 · группа C · сводка

## Сделано

- C1 фронт: `api/runs.rs` — убраны `AnswerRun`, обработчик `answer`, импорт `answer_from`, `requires_action` из
  `STATES` (10), шапка; `api/mod.rs` — маршрут `runs::answer` (в списке схем `AnswerRun` не был); `api/replies.rs` —
  поле `request`; `cli/commands.rs` — `RunCommand::Answer`, enum `Decision`, ветка исполнения, лишний импорт
  `ValueEnum`; `cli/mod.rs` — `Decision` из `pub use`, `run answer` из `OPERATIONS`.
- `docs/api/openapi.json` перегенерирован (`AAE_UPDATE_OPENAPI=1 cargo nextest run -p aae-front openapi`), −142 строки,
  `/answer` нет.
- `crates/aae/tests/e2e/replies.rs`: два случая `/answer` заменены одним — `POST /v1/runs/{checked}/cancel` на
  прогоне, дождавшемся `passed` (`until_state`), → 409 `rejected`. Число тестов модуля прежнее.
- C2 документы: `contracts.md` (§1.3, §3.2–3.4, §4.1–4.4, §4.6 claude/codex, §6.1, §6.2, §6.4, §10), `invariants.md`
  (15, восстановление), `config.md` (`human_timeout` удалён), `experiments.md:50`, `overview.md` (две строки),
  `frontends.md`, `first-formula.md`, `plugin.md`, `harness-updates.md` (claude 13, codex 13, перечень, последняя
  фраза), `rules-tests.md`, `README.md`.
- C3: новый подраздел `### Permissions` в `docs/design/config.md` (якорь `#permissions`), на него ссылаются
  `contracts.md` §4.6, `first-formula.md`, `frontends.md`, `overview.md`.
- C4: блок `[watchdog]` с комментарием удалён из обоих `demos/*/root/config.toml`; фраза про `human_timeout` в
  `demos/1-planner-executor/README.md` заменена на «ничего не ждёт человека, не разрешённое режимом отклоняется сразу».

## Расхождения с планом и решения по смыслу

- `docs/api/frontends.md:17` — «of an unknown run answers `invalid`» ловилась критерием grep `run answer`;
  переформулировано («answers `invalid` for an unknown run»), смысл тот же.
- `adapters.md:64` и `contracts.md:460` («never from the answer to a request») — о конце хода, не о разрешениях;
  оставлены.
- `plugin.md:8`: «answer a permission request» заменено на «remark on a turn», чтобы перечень остался из трёх.
- `experiments.md:50`: факт эксперимента оставлен, литерал состояния убран («to the waiting state»), добавлено, что
  адаптер этим не пользуется (`--permission-prompts none`). Строка содержит `--permission-prompt-tool stdio` как факт
  эксперимента.
- `contracts.md`: переносы абзацев (RunView, 3.3, 4.1, 4.4) выровнены по ширине 120.
- `cargo fmt -p aae` форматирует весь крейт, включая e2e-файлы группы B; содержательно их не менял.

## Проверка

- `cargo clippy -p aae-front --all-targets -D warnings` — чисто.
- `cargo nextest run -p aae-front` — 7/7.
- `cargo nextest run -p aae -E 'test(/replies/)'` — 2/2 (на текущем дереве, адаптеры B собрались).
- `git grep -nE 'requires_action|human_timeout|run answer' -- docs README.md demos crates/aae-front
  ':!docs/agents/decisions.md'` — пусто. Новые строки прозы в docs ≤ 120.
- Демо не запускались.
