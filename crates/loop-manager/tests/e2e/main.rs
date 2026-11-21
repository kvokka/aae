//! The end-to-end tests of `loop-manager`, as modules of one test binary: one build and one link for all of them, while
//! nextest still runs each test in a process of its own.

#[path = "../common/mod.rs"]
mod common;

mod agent_mail;
mod api;
mod await_step;
mod bridge;
mod claude;
mod cli;
mod cli_local;
mod codex;
mod context_budget;
mod dcg;
mod doctor;
mod doctor_version;
mod exec_env;
mod git_cache;
mod guides;
mod lessons;
mod mcp;
mod replies;
mod review_gate;
mod role_prompt;
mod run_card;
mod step_if;
mod stop;
mod tracker;
mod wait;
mod worktree;
