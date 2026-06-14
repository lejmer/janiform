//! Canonical interval factor grid contract and deterministic interval indexing.

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

pub use builder::{IntervalFactorGridBuildInput, build_interval_factor_grid};
pub use constants::{
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
    INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
};
pub use error::IntervalFactorGridError;
pub use loader::{IntervalFactorGridPayload, load_interval_factor_grid};
pub use news_intensity::{
    IntervalNewsIntensityMaterializationInput, materialize_interval_news_intensity_factors,
};
pub use seasonality::{
    IntervalSeasonalityMaterializationInput, IntervalSeasonalityMaterializationPolicy,
    IntervalSeasonalitySourceSignal, materialize_interval_seasonality_factors,
};
pub use types::{
    IntervalFactorGrid, IntervalFactorGridDiagnostics, IntervalFactorGridMetadata,
    IntervalNewsIntensityMaterializationDiagnostics, IntervalNewsIntensityMaterializationMetadata,
    IntervalNewsIntensityMaterializationOutput, IntervalSeasonalityMaterializationDiagnostics,
    IntervalSeasonalityMaterializationMetadata, IntervalSeasonalityMaterializationOutput,
};
