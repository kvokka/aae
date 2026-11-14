//! The steps of a scripted scenario.

use aae_core::driver::Normalized;
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

/// One step of a scenario.
#[derive(Clone, Debug)]
pub enum Step {
    Wait(Duration),
    /// Waits for the test to notify: a moment set by the test, not by virtual time racing real I/O.
    Await(Arc<Notify>),
    Emit(Normalized),
    /// As the first step: the prompt is not acknowledged, the scenario plays before `prompt.acked`.
    NoAck,
    /// The process dies: `exited` is sent and every later call on the instance fails with `harness_crash`.
    Exit {
        code: Option<i32>,
        signal: Option<i32>,
    },
    /// The process dies without a word: no `exited`, only its health turns `Dead`.
    Vanish,
}
