//! Interval factor grid module entrypoint.

mod contract;
mod provider;

pub use contract::{
    DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD, DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
    DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, DEFAULT_SEASONALITY_AGGREGATION_METHOD,
    DEFAULT_SEASONALITY_INTERPOLATION_METHOD, DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
    DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT,
    DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY, INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
    INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY, INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
    INTERVAL_FACTOR_GRID_SCHEMA_VERSION, INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
    INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
    INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, IntervalFactorGrid,
    IntervalFactorGridBuildInput, IntervalFactorGridDiagnostics, IntervalFactorGridError,
    IntervalFactorGridMetadata, IntervalFactorGridPayload,
    IntervalNewsIntensityMaterializationDiagnostics, IntervalNewsIntensityMaterializationInput,
    IntervalNewsIntensityMaterializationMetadata, IntervalNewsIntensityMaterializationOutput,
    IntervalSeasonalityMaterializationDiagnostics, IntervalSeasonalityMaterializationInput,
    IntervalSeasonalityMaterializationMetadata, IntervalSeasonalityMaterializationOutput,
    IntervalSeasonalityMaterializationPolicy, IntervalSeasonalitySourceSignal,
    build_interval_factor_grid, load_interval_factor_grid,
    materialize_interval_news_intensity_factors, materialize_interval_seasonality_factors,
};
pub use provider::{
    INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    INTERVAL_FACTOR_GRID_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD,
    IntervalFactorGridProviderError, IntervalFactorGridProviderRequest,
    IntervalNewsIntensityFactorSource, IntervalSeasonalityFactorSource,
    provide_interval_factor_grid,
};
