//! Shared workflow orchestration for Janiform entrypoints.
//!
//! This crate owns workflow execution UX: start summaries, progress labels,
//! completion summaries, and the lifecycle points where those views are emitted.

#![forbid(unsafe_code)]
pub mod error;
pub mod fit;
pub mod pricing;
pub mod seasonality;
pub mod simulation;
pub mod terminal;
pub mod validation;

mod artifact_io;
mod machine_events;
pub mod market_data_progress;
mod model;
mod price_coordinates;
mod progress;

pub use machine_events::write_machine_event_to_stderr;
