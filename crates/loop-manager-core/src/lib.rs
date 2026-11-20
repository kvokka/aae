//! Core of loop-manager: the single writer of the runtime state (docs/architecture.md, docs/design/event-model.md).

#![deny(clippy::disallowed_methods)]

mod clock;
mod effects;
mod journal;
mod process;
pub mod run;
mod store;
mod version;
mod writer;

pub use clock::Clock;
pub use journal::{Journal, JournalHealth, ReadError, SCHEMA, parse_rfc3339_ms, rfc3339_ms};
pub use loop_manager_contract::{BoxFuture, config, driver, orphans, template, toon, tracker};
pub use store::open;
pub use version::{supported_version, tested_version};
pub use writer::{CoreHandle, Error, Event, Executor, Handler, Tx, start};

#[cfg(test)]
mod tests;
