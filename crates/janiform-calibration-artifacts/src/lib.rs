//! Calibration artifact contracts and persistence surfaces.

#![forbid(unsafe_code)]

mod artifact_registry;
mod checkpointing;
pub mod contracts;
mod stage_outputs;

pub use artifact_registry::*;
pub use checkpointing::*;
pub use contracts::*;
pub use stage_outputs::*;
