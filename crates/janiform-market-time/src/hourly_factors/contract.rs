//! Canonical hourly factor contract and deterministic interval indexing.

mod builder;
mod constants;
mod error;
mod interval_indexing;
mod loader;
mod news_intensity;
mod seasonality;
#[cfg(test)]
mod tests;
mod types;

pub use builder::{HourlyFactorContractBuildInput, build_hourly_factor_contract};
pub use constants::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
    DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, DEFAULT_SEASONALITY_AGGREGATION_METHOD,
    DEFAULT_SEASONALITY_INTERPOLATION_METHOD, DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
    DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT, DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
};
pub use error::HourlyFactorContractError;
pub use loader::{HourlyFactorContractPayload, load_hourly_factor_contract};
pub use news_intensity::{
    HourlyNewsIntensityMaterializationInput, materialize_hourly_news_intensity_factors,
};
pub use seasonality::{
    HourlySeasonalityMaterializationInput, HourlySeasonalityMaterializationPolicy,
    HourlySeasonalitySourceSignal, materialize_hourly_seasonality_factors,
};
pub use types::{
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata, HourlyNewsIntensityMaterializationDiagnostics,
    HourlyNewsIntensityMaterializationMetadata, HourlyNewsIntensityMaterializationOutput,
    HourlySeasonalityMaterializationDiagnostics, HourlySeasonalityMaterializationMetadata,
    HourlySeasonalityMaterializationOutput,
};
