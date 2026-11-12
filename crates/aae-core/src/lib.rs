//! Core of aae: the single writer of the runtime state (spec §4, §5).

mod clock;
pub mod config;
pub mod cron;
pub mod driver;
mod effects;
mod journal;
pub mod orphans;
mod process;
pub mod run;
mod store;
pub mod template;
mod version;
mod writer;

pub use clock::Clock;
pub use journal::{Journal, ReadError, SCHEMA, parse_rfc3339_ms, rfc3339_ms};
pub use store::open;
pub use version::untested;
pub use writer::{BoxFuture, CoreHandle, Error, Event, Executor, Handler, Tx, start};

#[cfg(test)]
mod tests;
