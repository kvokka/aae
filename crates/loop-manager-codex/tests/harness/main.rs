//! The integration tests of the codex adapter, as modules of one test binary: one build and one link for all of
//! them, while nextest still runs each test in a process of its own.

mod codex;
mod common;
mod contract;
mod models;
mod subagents;
