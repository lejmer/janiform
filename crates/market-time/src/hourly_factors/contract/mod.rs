//! Canonical hourly factor contract and deterministic interval indexing.

mod builder;
mod constants;
mod error;
mod interval_indexing;
mod news_intensity;
mod seasonality;
#[cfg(test)]
mod tests;
mod types;

pub use builder::build_hourly_factor_contract;
pub use constants::*;
pub use error::HourlyFactorContractError;
pub use news_intensity::materialize_hourly_news_intensity_factors;
pub use seasonality::materialize_hourly_seasonality_factors;
pub use types::*;
