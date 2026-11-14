# План WP-A (черновик кодера)

## Правило владельца
Прогон, ждавший решения по решающему событию (runs.waiting NOT NULL: preparing; finishing; interrupted ждёт политику;
running после хода без исхода), после рестарта продолжает ждать того же решения: эффект hooks:<run> остаётся и
исполняется заново, осиротевший процесс хука убивается по маркеру до повтора, дедлайн decide взводится заново на полный
срок от старта демона. Экземпляр харнесса остановлен (переподключения нет): решение, которому нужна живая сессия
(remark на finishing, continue на turn_ended_without_outcome), завершает попытку сбоем daemon_crash в транзакции
решения. starting / running (ход идёт) / requires_action — как сейчас: interrupted с daemon_crash.

## Правки
1. recover.rs:
   - active = state IN (starting, running, requires_action, finishing) AND NOT (state IN (running, finishing) AND
     waiting IS NOT NULL). Для active — как сейчас (удаление launch/prompt/respond и hooks:<id>, sweep exec, attempt_failed).
   - kept = waiting IS NOT NULL AND state IN (running, finishing): UPDATE attempts SET handle = NULL для текущей
     попытки (процесса больше нет; stop_instance всех экземпляров уже поставлен выше). session остаётся (resume).
   - Перевзвод decide для всех runs с waiting NOT NULL (кроме active): at = now + SUM(hooks[].timeout_ms) по всем
     эффектам key = hooks:<run> (они исполняются по очереди перед нашим) + DECIDE_SLACK; если эффекта нет — дедлайн
     не трогаем (решать некому, пусть сработает как есть).
   - claim: UPDATE deadlines SET at_ms = now + start_timeout WHERE id = <run>:state для runs в preparing (дедлайн
     state у preparing бывает только у ждущего claim).
   - sweep маркеров хуков — как в 5359011, до всего.
   - daemon.started.interrupted_runs = active.len() (kept не входят).
2. finish.rs next_prompt: если handle NULL (после рестарта) ИЛИ self.stopping (дренаж) — attempt_failed(DaemonCrash,
   Source::Core) вместо промпта. Сейчас ветка «нет handle» даёт Unknown; иначе она недостижима.
3. read.rs status.active_runs: + (state = 'interrupted' AND waiting IS NOT NULL) — дренаж ждёт решения политики;
   решение затем паркуется (held/stopping), счётчик падает до 0.
4. decide.rs Park: waited = 1 только если json_extract(failure,'$.kind') IN (auth_required, billing) AND gate.key =
   runs.account AND есть активное закрытие gate.key (until NULL или > now) после policy_gate.
5. Документы contracts 3.1/3.4/6.4, invariants Recovery, plugin.md.

## Тесты (testkit, paused clock; World::restart_after(idle) двигает tokio-часы между убийством и стартом)
- finishing exec: команда ровно 1 раз, passed, хук run.finishing исполнен дважды;
- простой > Σ таймаутов: finishing, preparing (decide), interrupted (decide), preparing claim (state);
- remark после рестарта → run.attempt_failed{daemon_crash}; continue — то же; fail/нет решения → hollow_done;
- сирота-хук run.finishing убит по маркеру до повтора (реальные процессы, hooks.rs);
- park account: с until в прошлом → attempts_ceiling (конечно); auth.rs тест с backoff 0 переписать на ненулевой.
