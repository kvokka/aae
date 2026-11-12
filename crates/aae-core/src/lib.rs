//! Core of aae: the single writer of the runtime state (spec §4, §5).

mod clock;
mod effects;
mod store;
mod writer;

pub use clock::Clock;
pub use store::open;
pub use writer::{BoxFuture, CoreHandle, Error, Event, Executor, Handler, Tx, start};

#[cfg(test)]
mod tests;
