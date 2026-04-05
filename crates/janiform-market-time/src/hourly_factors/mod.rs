//! Hourly factor module entrypoint.

mod contract;
mod provider;

pub use contract::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
    DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, DEFAULT_SEASONALITY_AGGREGATION_METHOD,
    DEFAULT_SEASONALITY_INTERPOLATION_METHOD, DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
    DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT, DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractDiagnostics,
    HourlyFactorContractError, HourlyFactorContractOutput, HourlyFactorContractPayload,
    HourlyFactorMaterializationMetadata, HourlyNewsIntensityMaterializationDiagnostics,
    HourlyNewsIntensityMaterializationMetadata, HourlyNewsIntensityMaterializationOutput,
    HourlySeasonalityMaterializationDiagnostics, HourlySeasonalityMaterializationMetadata,
    HourlySeasonalityMaterializationOutput, build_hourly_factor_contract,
    load_hourly_factor_contract, materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors,
};
pub use provider::{
    HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD, HourlyFactorProviderError,
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
    provide_hourly_factor_contract,
};
