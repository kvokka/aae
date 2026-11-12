# WP-13 — план

Трейт `Driver` в `aae-core/src/driver.rs` по contracts.md 4.1–4.5, кроме того, что зависит от ещё не написанного:
- `split(EffectiveConfig, SessionPlan) -> Split` и `Fingerprint` — появляются с WP-12 (эффективный конфиг); сейчас
  определять `EffectiveConfig` — код на вырост.
- `Capabilities.versions` — с адаптерами (WP-27) и `doctor` (WP-35): потребителя нет.
- `live` — в трейте (Option), сценарный драйвер отдаёт `None` (сырого потока у него нет).
- serde на типах драйвера — нет: JSON-представление `Failure` пишет WP-14 вместе с событием `run.attempt_failed`.

Типы: `ProcessPart`, `SessionPart`, `SessionRef` — newtype над `Value`; `InstanceHandle`; `PromptKey` (UUID v5,
пространство имён фиксируется в contracts 4.2); `Answer`, `Transcript`, `Health`, `Capabilities`; `Failure`,
`FailureKind` с областью по умолчанию (таблица 4.4), `Scope`, `Source`; `DriverEvent { instance, session, seq,
event: Normalized }`; `EventSink = mpsc::UnboundedSender<DriverEvent>`.

Сценарный драйвер `crates/aae-testkit`: очередь сценариев; `send_prompt` берёт следующий, сам отвечает
`prompt.acked{key}` и проигрывает шаги (`Wait`, `Emit`, `Exit`); `respond` проигрывает следующий сценарий
(продолжение после запроса). Зависание = сценарий без `turn.ended`. `cancel_turn` обрывает проигрывание и шлёт
`turn.ended{cancelled}`. `transcript` — по отправленным ключам и завершённым ходам. pid сценарного экземпляра —
`i32::MAX` (больше `pid_max`): процесса с таким pid нет, убить чужое ядро не сможет.

Тест сценариев: задержка (управляемые часы), конец хода без исхода, ошибка провайдера, зависание, обрыв посреди
хода, запрос разрешения + ответ, событие вне хода, фоновая задача, отмена, transcript, resume неизвестной сессии.
