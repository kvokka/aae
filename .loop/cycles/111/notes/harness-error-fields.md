# Структурные поля ошибок harness'ов

Версии: `@anthropic-ai/claude-agent-sdk@0.3.326` (package.json `claudeCodeVersion` 2.1.326); codex repo tag `rust-v0.168.0`, commit 9480ec100c3edee9fc69172e8e80a4730444094d.
Ссылки claude: `sdk.d.ts` в пакете (строки). Сам CLI (cli.js) в пакете НЕ поставляется (нативный бинарь), поэтому поведение env смотрено по doc-комментариям d.ts и по bridge.mjs/sdk.mjs.

## A. claude

### A1. SDKResultMessage (sdk.d.ts:6396 `SDKResultMessage = SDKResultSuccess | SDKResultError`)
- Success: `type:'result'; subtype:'success'; is_error: boolean; api_error_status?: number|null (6383); result: string; stop_reason: string|null; terminal_reason?: TerminalReason; structured_output?; deferred_tool_use?`
- Error (6197, SDKResultError): `subtype: 'error_during_execution' | 'error_max_turns' | 'error_max_budget_usd' | 'error_max_structured_output_retries'; is_error: boolean; errors: string[]; stop_reason: string|null; terminal_reason?: TerminalReason; startup_failure_reason?: SDKStartupFailureReason; invalid_config_file?: {path:string; is_global:boolean; backup_searched:boolean; backup_path?:string}`. Поля `api_error_status` у error-варианта в d.ts НЕТ.
- TerminalReason (sdk.d.ts:10558), 19 значений:
  'blocking_limit' | 'rapid_refill_breaker' | 'prompt_too_long' | 'image_error' | 'model_error' | 'api_error' | 'malformed_tool_use_exhausted' | 'aborted_streaming' | 'aborted_tools' | 'stop_hook_prevented' | 'hook_stopped' | 'tool_deferred' | 'max_turns' | 'background_requested' | 'completed' | 'budget_exhausted' | 'structured_output_retry_exhausted' | 'tool_deferred_unavailable' | 'turn_setup_failed'
- SDKStartupFailureReason (sdk.d.ts:6536), 23 значения:
  'org_pin_api_key_conflict' | 'provider_not_allowed' | 'org_verify_failed' | 'org_pin_mismatch' | 'managed_settings_invalid' | 'remote_settings_required_unavailable' | 'gateway_signin_required' | 'gateway_access_denied' | 'proxy_invalid' | 'temp_dir_unusable' | 'cwd_unavailable' | 'shell_tool_missing' | 'session_held_by_background' | 'worktree_resume_refused' | 'worktree_unverified' | 'cli_version_too_old' | 'bypass_root' | 'org_config_required_unavailable' | 'org_config_refused' | 'sandbox_required_unavailable' | 'initialize_not_first' | 'config_file_invalid' | 'managed_settings_not_delivered'
- CLAUDE_CODE_STARTUP_FAILURE_RESULTS (doc на `startup_failure_reason`, sdk.d.ts ~6237): "Set on the zeroed `error_during_execution` result a stream-json run writes before exiting on a known startup failure; `errors` carries the failure message. Failures that otherwise end with stderr alone write that result only when the host sets CLAUDE_CODE_STARTUP_FAILURE_RESULTS, except org_config_required_unavailable, org_config_refused and config_file_invalid, which every stream-json run writes. Absent on every other result, on startup failures without a known cause, and from older producers."
  Т.е. env меняет вывод так: при startup-сбое вместо голого stderr в stdout пишется `{"type":"result","subtype":"error_during_execution", is_error:true, zeroed usage/cost, errors:[msg], startup_failure_reason:"<value>"}`. Отдельного message type/subtype нет. Env в реестре переменных: core-y6bhrwmk.mjs (`CLAUDE_CODE_STARTUP_FAILURE_RESULTS`, булев). Реализацию (cli.js) не видно.
- В bridge.mjs (проброс result для Remote Control) встречаются ещё поля result, которых нет в d.ts: `api_error_code`, `api_error_type`, `api_error_origin`, `api_error` (рядом с `api_error_status`, только в success-ветке). Не типизированы в 0.3.326 — не опираться.

### A2. SDKAssistantMessage.error (sdk.d.ts:3889 `error?: SDKAssistantMessageError`; тип 3972)
'authentication_failed' | 'oauth_org_not_allowed' | 'account_on_hold' | 'verification_required' | 'billing_error' | 'rate_limit' | 'overloaded' | 'invalid_request' | 'model_not_found' | 'server_error' | 'unknown' | 'max_output_tokens' | 'cloud_credential_error'   (13)
- В d.ts assistant-сообщение НЕ несёт `api_error_status` (поля нет; есть только `error`, `request_id`, `parent_tool_use_id`, `message`). `api_error_status` — только в SDKResultSuccess.
- Тот же тип в `StopFailureHookInput.error` (sdk.d.ts:10414, + `error_details?: string`).
- bridge.mjs: таблица `{stalled, error, api_error, "api_error:<каждое из 13 значений>"}` — внутренние метки; подтверждает набор из 13.

### A3. rate_limit_event (sdk.d.ts:6166 SDKRateLimitEvent `{type:'rate_limit_event'; rate_limit_info: SDKRateLimitInfo; uuid; session_id}`; SDKRateLimitInfo 6180)
- status: 'allowed' | 'allowed_warning' | 'rejected'
- rateLimitType?: 'five_hour' | 'seven_day' | 'seven_day_opus' | 'seven_day_sonnet' | 'seven_day_overage_included' | 'overage'
- errorCode?: 'credits_required'  (единственное значение)
- overageStatus?: 'allowed' | 'allowed_warning' | 'rejected'
- overageDisabledReason?: 'overage_not_provisioned' | 'org_level_disabled' | 'org_level_disabled_until' | 'out_of_credits' | 'seat_tier_level_disabled' | 'member_level_disabled' | 'seat_tier_zero_credit_limit' | 'group_zero_credit_limit' | 'member_zero_credit_limit' | 'org_service_level_disabled' | 'no_limits_configured' | 'fetch_error' | 'unknown'
- limitScope?: 'service' | 'channel' | 'group_pool'
- прочее: resetsAt?, utilization?, overageResetsAt?, isUsingOverage?, overageInUse?, overageEnabled?, surpassedThreshold?: number/bool, canUserPurchaseCredits?, hasChargeableSavedPaymentMethod?

### A4. system/api_retry (sdk.d.ts:3858 SDKAPIRetryMessage)
`{type:'system'; subtype:'api_retry'; attempt:number; max_retries:number; retry_delay_ms:number; error_status:number|null (null = ошибка соединения без HTTP-ответа); error: SDKAssistantMessageError; no_response?:{waited_ms:number; retry_wait_ms:number}; uuid; session_id}`
Тот же счётчик ретраев в control-progress (sdk.d.ts:5486, status 'started'|'api_retry').

### A5. oauth_org_not_allowed / cloud_credential_error
Только значения `SDKAssistantMessageError` (assistant.error, api_retry.error, StopFailureHookInput.error). Не `terminal_reason`, не `startup_failure_reason`, не result.subtype. (grep по d.ts: единственное вхождение — sdk.d.ts:3972.)

## B. codex (codex-rs/…, rust-v0.168.0)

### B1. CodexErrorInfo (app-server-protocol/src/protocol/v2/shared.rs:76; `#[serde(rename_all="camelCase")]`)
Unit-варианты -> JSON-строка: "contextWindowExceeded" "sessionBudgetExceeded" "usageLimitExceeded" "rateLimitExceeded" "flexUnavailable" "serverOverloaded" "modelNotFound" "policyCapacityUnavailable" "cyberPolicy" "misalignmentPolicyViolation" "tooManyDenials" "internalServerError" "unauthorized" "badRequest" "threadRollbackFailed" "sandboxError"
Варианты с полем -> объект `{"<variant>":{"httpStatusCode": <u16|null>}}`:
 httpConnectionFailed, responseStreamConnectionFailed, responseStreamDisconnected, responseTooManyFailedAttempts
 activeTurnNotSteerable -> `{"activeTurnNotSteerable":{"turnKind":"review"|"compact"}}` (NonSteerableTurnKind camelCase)
Other: `#[serde(untagged, serialize_with=…)]` сериализуется строкой "other"; десериализуется из любой строки/объекта (неизвестное -> Other).
Wire-примеры: `"codexErrorInfo":"usageLimitExceeded"`; `"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":401}}`.
Core-вариант `BioPolicy` и `InvalidPrompt` в v2 мапятся в `Other` (shared.rs:~160, :180), т.е. на wire их нет. Core-enum: protocol/src/protocol.rs:2081 (28 вариантов, snake_case в core-сериализации, protocol/src/codex_error_info.rs:19).

### B2. ErrorNotification (метод "error"; v2/notification.rs:62; common.rs:2019)
`{ error: TurnError, willRetry: bool, threadId: string, turnId: string }`; willRetry=true — транзиент, app-server сам ретраит, ход не прерывается.
TurnError (v2/thread_data.rs:439): `{ message: string, codexErrorInfo: CodexErrorInfo|null, additionalDetails?: string|null, misalignment?: MisalignmentErrorDetails|null }` (misalignment: errorType?: string|null, …).

### B3. Rate limits (v2/account.rs)
Нотификация `account/rateLimits/updated` (common.rs:2090; account.rs:687): `{ rateLimits: RateLimitSnapshot }` (sparse-обновление: мержить с account/rateLimits/read).
RateLimitSnapshot (account.rs:694), camelCase: limitId?:string|null, limitName?, normalModelSlug?, primary?: RateLimitWindow|null, secondary?: RateLimitWindow|null, credits?: CreditsSnapshot|null, individualLimit?: SpendControlLimitSnapshot|null, spendControlReached?: bool|null ("None = unavailable"), planType?: PlanType|null, rateLimitReachedType?: RateLimitReachedType|null.
RateLimitReachedType (account.rs:731; **snake_case** на wire): "rate_limit_reached" | "workspace_owner_credits_depleted" | "workspace_member_credits_depleted" | "workspace_owner_usage_limit_reached" | "workspace_member_usage_limit_reached"
RateLimitWindow: usedPercent:i32, windowDurationMins:i64|null, resetsAt:i64|null. CreditsSnapshot: hasCredits:bool, unlimited:bool, balance:string|null. SpendControlLimitSnapshot: limit:string, used:string, remainingPercent:i32, resetsAt:i64.
PlanType (protocol/src/account.rs:12): free, go, plus, pro, prolite(ProLite), promax, team, self_serve_business_prolite, self_serve_business_usage_based, business, ent26, enterprise_cbp_automation, … (список не дочитан до конца).

### B4. Turn / turn/completed
TurnStatus (v2/turn.rs:34, camelCase): "completed" | "interrupted" | "failed" | "inProgress".
`turn/completed` (common.rs:2052; turn.rs:611): `{ threadId, turn: Turn }`; Turn (thread_data.rs:387) имеет `status: TurnStatus` и `error: TurnError|null` ("Error associated with a failed or interrupted turn"). Т.е. упавший ход: status="failed", turn.error={message,codexErrorInfo,…}. Параллельно шла нотификация `error` (willRetry=false). Определение "влияет ли на статус": protocol.rs `CodexErrorInfo::affects_turn_status` (false для ThreadRollbackFailed, ActiveTurnNotSteerable).

### B5. JSON-RPC коды app-server (app-server/src/error_code.rs:3-7)
-32600 INVALID_REQUEST, -32601 METHOD_NOT_FOUND, -32602 INVALID_PARAMS, -32603 INTERNAL_ERROR, **-32001 OVERLOADED** ("server overloaded; retry later"): очередь запросов переполнена — in_process.rs:662/769 ("in-process app-server request queue is full"), feedback_processor.rs:104, mcp_event_stream.rs:72. Это ошибка ответа на JSON-RPC запрос (back-pressure), не ошибка хода/модели. Строка INPUT_TOO_LARGE_ERROR_CODE = "input_too_large" (data-код). server_draining_error: -32600 c data {"reason":"serverShuttingDown"}.

### B6. Откуда берётся CodexErrorInfo
Маппинг CodexErr -> CodexErrorInfo: protocol/src/error.rs:510 `to_codex_protocol_error`. HTTP-классификация: codex-api/src/api_bridge.rs:55-270 (`map_api_error_details`), codex-api/src/error.rs:81 (`parse_terminal_provider_error`).
- HTTP 401/403/408/504 (и любой статус, не перечисленный ниже, например 400 не-policy идёт в InvalidRequest): ветка `else` -> `CodexErr::UnexpectedStatus` (api_bridge.rs:~259) -> `HttpConnectionFailed{httpStatusCode: <статус>}` (error.rs:533). Специальной обработки 401/403/408/504 в CodexErrorInfo НЕТ; различать по httpStatusCode. 401 в core/src/client.rs (UnauthorizedRecovery, ~:1670) сначала пробует refresh токена, и лишь при провале refresh -> `RefreshTokenFailed` -> `Unauthorized` (error.rs:543). `Unauthorized` — только это.
- 403/400 с misalignment-телом -> MisalignmentPolicyViolation (api_bridge.rs:149); 400 -> InvalidRequest/InvalidImageRequest -> (в error.rs `_ => Other`? InvalidRequest не в списке BadRequest -> Other), bio/cyber/invalid_prompt по коду тела (api_bridge.rs:171-205).
- Код тела `model_not_found` при 400/403/404 -> ModelNotFound (error.rs:81-122, status сохраняется).
- 408 / транспортный таймаут: `TransportError::Timeout` -> `CodexErr::RequestTimeout` -> `Other` (нет httpStatusCode); HTTP-статус 408 -> UnexpectedStatus -> HttpConnectionFailed{408}.
- 500 -> InternalServerError. 503 + error.code `policy_capacity_unavailable` -> PolicyCapacityUnavailable; `server_is_overloaded` -> ServerOverloaded; `slow_down` -> RateLimitExceeded.
- 429: тело error.type `usage_limit_reached` -> UsageLimitReached -> `usageLimitExceeded` (+ rate_limit_reached_type из заголовков); error.type `usage_not_included` -> UsageNotIncluded -> `usageLimitExceeded`; error.type/code insufficient_quota | credit_balance_exhausted | organization_spend_limit_exceeded | project_spend_limit_exceeded | organization_usage_limit_exceeded -> QuotaExceeded -> `usageLimitExceeded`; прочее 429 -> RetryLimit -> `responseTooManyFailedAttempts{httpStatusCode:429}`; код `flex_unavailable` -> FlexUnavailable (`flexUnavailable`, http 429).
- "workspace out of credits": не отдельный код, а текст Display UsageLimitReachedError (protocol/src/error.rs:~771-777) при rate_limit_reached_type = workspace_owner/member_credits_depleted; CodexErrorInfo всё равно `usageLimitExceeded`. Структурно различимо только по `account/rateLimits/updated.rateLimits.rateLimitReachedType`.
- Прочие маппинги: SessionBudgetExceeded<-`budget_reached`; ConnectionFailed -> HttpConnectionFailed{status из reqwest, часто null}; ResponseStreamFailed -> ResponseStreamConnectionFailed; Sandbox -> `sandboxError`; InternalAgentDied/SessionConfiguredNotFirstEvent -> `internalServerError`; UnsupportedOperation/ThreadNotFound/AgentLimitReached/InvalidAudioInput -> `badRequest`; всё остальное -> `other`.
