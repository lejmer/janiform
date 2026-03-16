//! Shared exogenous news process configuration and deterministic intensity model.
//!
//! This module defines one typed configuration payload that is reusable by
//! simulation, pricing, and calibration artifact persistence.

mod configuration;
mod dispatch;
mod errors;
mod interface;
mod process;
mod sampling;

pub use configuration::*;
pub use dispatch::*;
pub use errors::*;
pub use interface::*;
pub use process::*;

#[cfg(test)]
mod tests;
