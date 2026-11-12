# WP-14b — план (кодер, цикл 9)

1. Схема v3: `attempts.token_sha256`, `attempts.outcome` (JSON), `attempts.prompts` (число промптов попытки,
   замечаний = prompts − 1). Индекс по `token_sha256`.
2. Токен: 32 случайных байта (`getrandom`), hex (64 знака) — без base64-зависимости; contracts 6.3 правится.
   Сырой токен — только в `SessionPlan` (поле `token`) и в полезной нагрузке эффекта `launch`; в состоянии — SHA-256.
3. `Msg::Finish{run, token, outcome}` → ответ `{"answer": accepted|stale_attempt|run_terminal|invalid}`;
   принимается в `starting|running|requires_action` текущей попытки; повтор заменяет исход.
4. `turn_settled`: исход есть → `finishing` + `run.finishing` (решающее); нет → как было. Остаток шагового
   таймаута ставится на паузу (`paused_ms`), а не сбрасывается — возвращается при `continue`/`remark`.
5. Решения: `Continue{message}`, `Accept`, `Veto{reason}`, `Remark{message}`. Финиш: accept/без решения →
   `passed` (pass) или `failed{agent}` (fail); veto → `failed{check}` (на заявленном fail — `failed{agent}`);
   remark/continue → следующий промпт той же попытки (эффект `prompt`, `PromptKey` n+1), исход сброшен.
   Потолок 10 замечаний: 11-е → `failed{check|hollow_done, remarks_ceiling}`.
6. Тесты на сценарном драйвере: pass → passed; повтор финишера; устаревший токен; замечание → второй промпт →
   passed; вето; continue после пустого хода; потолок замечаний.
7. Потом, если хватит контекста: лимиты параллелизма (оркестратор/агент) — поля конфига + `config.md`.
