//! The end-to-end tests of `aae`, as modules of one test binary: one build and one link for all of them, while
//! nextest still runs each test in a process of its own.

#[path = "../common/mod.rs"]
mod common;

mod api;
mod await_step;
mod bridge;
mod claude;
mod cli;
mod codex;
mod doctor;
mod git_cache;
mod mcp;
mod replies;
mod review_gate;
mod role_prompt;
mod step_if;
mod stop;
mod tracker;
mod wait;
mod worktree;
