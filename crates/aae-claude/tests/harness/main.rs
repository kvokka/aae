//! The integration tests of the claude adapter, as modules of one test binary: one build and one link for all of
//! them, while nextest still runs each test in a process of its own.

mod claude;
mod contract;
