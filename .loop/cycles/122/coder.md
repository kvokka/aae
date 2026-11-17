# Цикл 122 — coder
STATUS: done

## Взято в работу
- WP-112 (обяз. 1 ревью 121): довести WIP — причина, 20/20 под нагрузкой, мутация, коммит, гейт.
- WP-111 (второй S): плагин скиллов в текущих форматах обоих харнессов.

## Сделано (коммиты)
- `7f11037` WP-112. **Причина.** Обходы `/proc` (`process::wait_gone`, `sweep` в `run/exec.rs`, `listens_alone` и
  `sweep` в `run/service_proc.rs`) шли через `spawn_blocking`. Стенд chaos — current-thread рантайм с паузой часов:
  обход на blocking-потоке заканчивается в момент реального времени, и задача просыпается между теми задачами, что
  планировщик успел опросить к этому моменту. Под нагрузкой обход длиннее, порядок другой — повтор сида запускает
  харнессы в другом порядке (`instance-14` против `instance-15` в журнале, `the steps differ`). **Правка.**
  `process::walk`: на current-thread рантайме обход на месте, иначе `spawn_blocking`. Rustdoc исправлен по замечанию
  ревью: `None` — только при панике на blocking-потоке, на месте паника идёт дальше. event-model.md описывает это.
  **Доказательство** (busy-loop на все 9 ядер, load ~10; ряды и вывод падения — `notes/wp112.md`):
  - база `a2c3736` без правки, 3 повтора: 1/20 упал (`seed 10513819777643417995: the journal differs`,
    `"instance-14"` против `"instance-15"`);
  - мутация (ветка `CurrentThread` в `walk` удалена), 30 повторов: 10/10 упали (`journal differs` / `steps differ`);
  - правка, 30 повторов: 0/10; правка, 3 повтора: 0/20 подряд.
- `4b83553` WP-111. Codex: `harness-plugin/plugin.json` (portable, `$schema` agent-plugins 1.0.0) и
  `.agents/plugins/marketplace.json` (`source: {source: local, path: ./harness-plugin}`, `policy`, `category`).
  Манифесты Claude Code — без изменений (`marketplace.json` в корне и `plugin.json` плагина, оба в `.claude-plugin`).
  `.codex-plugin/` нет. Тест `the_manifests_are_what_claude_and_codex_install` проверяет все четыре манифеста: имена,
  версии = версии крейта, оба маркетплейса указывают на один каталог, нет `.codex-plugin/`. Мутации: версия в
  корневом `plugin.json` → красный; создан `harness-plugin/.codex-plugin/` → красный; путь в codex-маркетплейсе →
  красный. Документ: architecture.md (абзац про `harness-plugin/`); команды в recovery.md не изменились.
  **Вживую, изолированные HOME / CODEX_HOME / CLAUDE_CONFIG_DIR, без модели:**
  - codex 0.169.0 на копии без манифестов Claude (оба удалены): `marketplace add` → `plugin add aae@aae` →
    `plugin list`: `aae@aae installed, enabled 0.1.0`, маркетплейс `.agents/plugins/marketplace.json`, в кэше
    корневой `plugin.json`;
  - codex на полной копии: `plugin list` называет маркетплейс `.agents/plugins/marketplace.json`, не legacy;
  - Claude Code 2.1.326: `plugin validate` корня и `harness-plugin` — passed; `marketplace add` + `install aae@aae`
    — installed, enabled.
- Гейт `scripts/loop/checks 122` на HEAD `7f11037`: pass, 40 s, nextest 618/618 за 28.8 s, на тихой машине (Жел. 1
  ревью 121 — время переснято).

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
—

## Замечания предыдущего ревью: что закрыто, что нет
- Обяз. 1 — закрыт: причина, вывод падения до правки, 20/20 под нагрузкой, мутация, rustdoc `walk`, коммит, гейт;
  `git status --porcelain -- . ':!.loop'` пуст.
- Жел. 1 — гейт переснят на тихой машине: 40 s.
- Жел. 2 (печать `[1]` как `1` в explain) и Жел. 3 (таблица development.md) — не трогал.

## Контекст на момент отчёта: ~85k
