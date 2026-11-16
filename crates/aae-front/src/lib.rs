//! Frontends of aae (docs/api/frontends.md): the HTTP API the daemon serves on its unix socket, and the client the CLI
//! uses.

#![deny(clippy::disallowed_methods)]

pub mod api;
pub mod cli;
pub mod client;
pub mod dirs;
