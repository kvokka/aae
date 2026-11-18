# Цикл 131 — coder
STATUS: done

## Взято в работу
- Обязательные 1–3 ревью цикла 130.
- WP-133 (релиз: GitHub Release и crates.io).
- WP-135 (`max_writers`) — по плану expert, реализация субагентом.

## Сделано (коммиты)
- `900af8c` WP-129: `docs/api/frontends.md` — `LOOP_MANAGER_SOCKET` читает и `loop-manager daemon`; демону из сессии
  loop-manager нужен свой `--socket`, иначе он не стартует («another daemon is listening»).
- `6555e4f` флейк `bridge::the_port_opens_only_once…`: тест не делает TCP-connect к отпущенному порту, а смотрит,
  слушает ли порт сам мост (`/proc/<pid>/net/tcp`, LISTEN, inode среди fd моста). `listens()` проверена и в обратную
  сторону: после `ready()` она true. Вариант «у моста нет ни одного сокета» не годится: у tokio пара unix-сокетов
  для сигналов.
- `f387fde` флейк `api::…from_a_seq`: причина найдена воспроизведением (5 мин `--stress-duration` под двумя
  параллельными полными прогонами набора: 5 падений). Пришедшее событие — `run.state_changed` seq 7 (до `run.passed`):
  `journal_seq` — последний синхронизированный seq, а поток читает файл и видит пачку до `sync_data`. Тест теперь
  просит живой вид с `seq(run.passed) + 1`. Rustdoc `journal_seq` (и `openapi.json`) говорит, что поток может быть
  впереди. Дефекта продукта нет: обещание «вид закончившегося прогона с после его конца закрывается» выполняется.
  Проверка после правки: 3759 итераций stress за 190 s под той же нагрузкой — все зелёные.
- `392cf62` третий флейк, найденный той же нагрузкой: `claude::a_claude_killed_in_a_command_is_resumed…`
  (1 из 12 полных прогонов): командная строка `sh -c 'printf … > beads-dir; sleep 20'` содержит `sleep 20` с самого
  старта, и claude убивался до `printf`. Тест ждёт и файл.
- `cdf9899` WP-133: `release.yml` (проверка тег = версия воркспейса; сборка и критерий 1; упаковка
  `loop-manager-<target>.tar.gz` + `.sha256`; задание `github-release` с `contents: write` — создаёт релиз, если его
  нет, и `gh release upload --clobber`; задание `crates-io` — `cargo publish --workspace --locked` с `--exclude` уже
  опубликованных на этой версии крейтов по sparse-индексу), раздел «Releasing» в `development.md`, «Install a
  release» в `install.md`, «Installing» в README.

- `e2310a2` WP-135: ключ `max_writers` у `[rigs.<name>]` (в `[root]` не принимается, `0` — ошибка конфига).
  Логика — новый `crates/loop-manager-core/src/run/writers.rs` (65 строк), из `queue.rs::blocker` — один вызов
  после `concurrency`, проверяется только прогон в `queued`; причина ожидания — новое `waiting_for = "writers"`.
  Подсчёт по формуле целиком (нет `worktree`, есть exec-шаг или ai-шаг агента не scope `root`), с `preparing` до
  терминального, `parked`/`interrupted` держат место. Прогон в шаге `drain`/`formula` «одалживает» место своим
  потомкам по цепочке таких шагов (рекурсивный CTE) — взаимоблокировки drain при лимите 1 нет. `reload` теперь
  проходит очередь (`schedule`), чтобы снятый лимит сразу будил ждущих. `config explain --formula --rig` —
  строка `max_writers = 1  # rig`. Документы: config.md, contracts.md 1.3/6.1/6.2, решение 108, assumptions,
  openapi.json. Тесты: 7 core-тестов (`tests/core/writers.rs`), по одному в contract и front.
  План — expert (`notes/wp135-plan.md`), реализация — subcoder (`notes/wp135-impl.md`: мутации — все 5 из плана и 4
  дополнительные убиты), я прочитал diff ядра и документов и перепроверил тесты writers.
  Вопросы владельцу (expert записал в журнал, до ответа действует первый вариант): `[root]` без `max_writers`; прогон
  из сессии агента (`run start`) считает родителя (при лимите 1 ждёт его конца); правило одалживания в drain/formula.
  Известные неточности: прогон с `continue_from` от worktree-прогона считается писателем; если reload снизит лимит
  ниже числа живых, две семьи drain на одном риге могут ждать друг друга до конца одной.

## Проверки WP-133
- `cargo publish --workspace --dry-run --locked` — exit 0, все 9 крейтов упакованы и собраны, порядок cargo вывел сам
  (contract, addons, br, core, testkit, claude, codex, front, loop-manager); предупреждения только «already exists on
  crates.io index» для 0.0.1. Лог: `.loop/cycles/131/notes/publish-dry-run.log`.
- `actionlint` 1.7.7 (`mise exec actionlint@1.7.7 -- actionlint`, с shellcheck 0.11) — 0 ошибок.
- Логика пропуска опубликованных проверена локально: для `v0.0.1` — «every crate is on crates.io at 0.0.1 already»,
  для `v0.0.2` — `cargo publish --workspace --locked` без исключений.
- Имена ассетов против правил mise `github:` и cargo-binstall: заметка `.loop/cycles/131/notes/release-assets.md`
  (helper читал исходники mise и cargo-binstall через raw.githubusercontent.com: WebFetch был закрыт). mise узнаёт
  `linux`, `darwin`, `x86_64`, `aarch64`, отбрасывает `.sha256` как метаданные и сверяет архив с соседним
  `<asset>.sha256` (формат `sha256sum`); cargo-binstall без `[package.metadata.binstall]` пробует и безверсионное
  `{name}-{target}{archive-suffix}`, bin-dir — корень архива. Ссылки на документацию — в заметке.
- Не проверено вживую: настоящий тег не ставился (по плану), установка из настоящего релиза не делалась.

## Проверки цикла
- `scripts/loop/checks 131` на HEAD `e2310a2`: RESULT pass, ci 54 s, nextest 669/669.
- Флейки: после правок 12 полных прогонов набора (по два параллельно) + 3759 stress-итераций `api::…from_a_seq` — до
  правки claude-теста 1 падение claude (починено), остальное зелёное. Bridge входил в каждый полный прогон.

## Не получилось и почему
- Ничего из взятого.
## Начато и не закончено (файлы, точка остановки, следующий шаг)
- Нет. Следующий пакет плана — WP-134 (стартовый пак), теперь разблокирован WP-135.
- Коммиты флейков (`6555e4f`, `f387fde`, `392cf62`) без префикса `WP-NN`: это пункты ревью, не пакеты плана.
## Замечания предыдущего ревью: что закрыто, что нет
- Обязательные 1, 2, 3 — закрыты (коммиты выше).
- Желательные 1–6 — не трогал.
## Контекст на момент отчёта: ~170k
