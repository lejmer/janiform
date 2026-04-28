//! Shared workflow orchestration for Janiform entrypoints.

#![forbid(unsafe_code)]

pub mod error;
pub mod fit;
pub mod pricing;
pub mod simulation;

mod machine_events;
mod model;
mod progress;

pub use machine_events::write_machine_event_to_stderr;
