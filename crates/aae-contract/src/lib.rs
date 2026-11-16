//! The contracts of the core of aae (docs/design/contracts.md): the config, the driver and tracker interfaces, the
//! event envelope and the registry of child processes. The engine over them is `aae-core`.

#![deny(clippy::disallowed_methods)]

pub mod config;
pub mod cron;
pub mod driver;
mod event;
pub mod orphans;
pub mod template;
pub mod tracker;

pub use event::{SCHEMA, envelope, parse_rfc3339_ms, rfc3339_ms};

pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(test)]
mod tests;
