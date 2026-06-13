//! Shared interval factor grid provider used by pricing and simulation consumers.

mod constants;
mod error;
mod request;
mod service;
#[cfg(test)]
mod tests;

pub use constants::{
    INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    INTERVAL_FACTOR_GRID_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD,
};
pub use error::IntervalFactorGridProviderError;
pub use request::{
    IntervalFactorGridProviderRequest, IntervalNewsIntensityFactorSource,
    IntervalSeasonalityFactorSource,
};
pub use service::provide_interval_factor_grid;
