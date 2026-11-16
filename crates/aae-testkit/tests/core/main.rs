//! The tests of the core on the scripted driver, as modules of one test binary: one build and one link for all of
//! them, while nextest still runs each test in a process of its own. The chaos stand is a binary of its own.

#[path = "../common/mod.rs"]
mod common;

mod agent_limit;
mod agent_prompt;
mod anthropic;
mod auth;
mod bead_events;
mod beads;
mod children;
mod deciding;
mod drain;
mod drain_rounds;
mod drain_steps;
mod driver_events;
mod epic;
mod exec;
mod finisher;
mod gates;
mod hooks;
mod instances;
mod mcp;
mod model_gate;
mod old_instances;
mod openai;
mod recovery;
mod requests;
mod restart_decisions;
mod returns;
mod root;
mod runs;
mod service_ports;
mod services;
mod step_repeat;
mod step_repeat_exits;
mod step_repeat_more;
mod steps;
mod trigger_scope;
mod triggers;
mod watchdog;
mod worktree;
