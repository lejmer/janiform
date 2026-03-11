//! Calibration-stage orchestration entrypoints and shared adapters.

mod adapters;
mod artifact_registry;
mod checkpointing;
mod contracts;
mod execution_bridge;
mod market_data_preprocessing;
mod objective;
mod optimizer_adapter;
mod staged_runner;
mod stages;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub use adapters::*;
pub use artifact_registry::*;
pub use checkpointing::*;
pub use contracts::*;
pub use execution_bridge::*;
pub use market_data_preprocessing::*;
pub use objective::*;
pub use optimizer_adapter::*;
pub use staged_runner::*;
pub use stages::hawkes::*;
pub use stages::return_components::*;
pub use stages::seasonality::*;
