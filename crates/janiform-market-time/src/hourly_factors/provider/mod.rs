//! Shared hourly factor provider used by pricing and simulation consumers.

mod constants;
mod error;
mod request;
mod service;
#[cfg(test)]
mod tests;

pub use constants::{
    HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD,
};
pub use error::HourlyFactorProviderError;
pub use request::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};
pub use service::provide_hourly_factor_contract;
