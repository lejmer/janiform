//! Hourly factor module entrypoint.

mod contract;
mod provider;

pub use contract::*;
pub use provider::{
    HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD, HourlyFactorProviderError,
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
    provide_hourly_factor_contract,
};
