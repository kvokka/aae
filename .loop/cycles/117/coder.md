# Цикл 117 — coder
STATUS: done

## Взято в работу
WP-106 (мост GitHub и тест без модели) целиком; попутно Жел. 1–4 ревью 116.

## Сделано (коммиты)
- `3ad8191` WP-106: мост и e2e.
  - `demos/3-review-gate/bridge.sh <project>`: следит за `aae --json events`. `run.awaiting` будит мост, есть и
    перечитывание `run list --awaiting` раз в INTERVAL — оно закрывает гонку старта. GitHub опрашивается, только пока
    кто-то ждёт `review`. Root PR — последняя непустая строка stdout шага `pr`, остальные PR — по ветке
    `feature/<bead>` в каждом репо. Правила: merged → `pass` с `{merged_by}`; closed → `run cancel`
    «closed without a merge: <url>»; комментарий автора из `inputs.feedback_from` без EYES → `fail` с
    «review: <url комментария>». Ключ идемпотентности `bridge-<run>-<round>`.
  - `run.sh` запускает мост (лог `.aae/bridge.log`) и гасит его после `run wait`. Экспортирует `GH_CONFIG_DIR` до
    подмены `XDG_CONFIG_HOME` — НАХОДКА: без этого `gh` в exec-шагах и в мосте не видел логина.
  - Ещё одна НАХОДКА: `jq -r … | tail -1` по stdout с завершающим `\n` давал пустую строку, поэтому PR в `RESULT.md`
    был бы пустым. Это исправлено в run.sh и в мосте.
  - Тест `crates/aae/tests/e2e/review_gate.rs`, три сценария (≈9 s): пак, формулы, `aae.toml` и `root/agents`
    берутся из каталога демо; настоящий codex ходит на фейковые Responses API; фейковый `gh`
    (`crates/aae/tests/e2e/fake_gh.sh`) работает поверх локальных bare-remote; мост — сам `bridge.sh`.
    - (а) merge → `pass` → `merge`, `merged_by`, репо вернулись на `main`;
    - (б) комментарий постороннего ничего не меняет; комментарий разрешённого автора → `collect` → `address` → снова
      `await` в раунде 2; в feedback-файле только разрешённый комментарий; после EYES повторного возврата нет;
    - (в) пустой `feedback_from` — комментарий не действует; close → `cancelled` → `cleanup` прошёл, удалённая ветка
      удалена; упавший `feature` (planner `finish fail`) чистку не запускает.
  - `common::command`: каталог `dir/path`, если он есть, идёт первым в PATH демона.
  - Документы: README демо 3 (раздел «The bridge», «Run» под ход с мостом), `demos/README.md`,
    `docs/acceptance.md` (строка 19 и крит. 20), `docs/api/frontends.md` («Humans and runs»).
  - Хук reviser `no-github`: регэксп `(^|[^A-Za-z0-9_.-])gh([^A-Za-z0-9_.-]|$)` ловит путь, обратную кавычку,
    `$(…)`, `&&`, строку и массив; `high.md`, `gh.md` и `gh-dash` не трогает.
- `af8e184`: `cleanup` делает `git reset --hard origin/<base>` (Жел. 4), README обновлён.
- Мутации (обе откачены): без фильтра автора красные (б) и (в); без фильтра EYES красный (б).
- Гейт: `scripts/loop/checks 117` — pass, 36 s, 593/593.

## Не получилось и почему
—

## Начато и не закончено (файлы, точка остановки, следующий шаг)
— Следующий пакет — WP-107 (живой прогон демо 3).

## Замечания предыдущего ревью: что закрыто, что нет
- Обязательных не было.
- Закрыты: Жел. 1 (README и run.sh описывают ход с мостом), Жел. 2 (хук), Жел. 3 (acceptance.md; загрузку пака
  демо 3 теперь проверяет и `mise run ci` через e2e), Жел. 4 (сброс на `origin/<base>`).
- Не закрыто: Жел. 5 — перенос из ревью 115, вне плана.
- Хук reviser тестами не покрыт: регэксп проверен только вручную через jq.

## Контекст на момент отчёта: ~135k
