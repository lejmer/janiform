use crate::interval_factor_grids::{
    INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
};

/// Stable provider method identifier when `seasonality_factor` is materialized from source samples.
pub const INTERVAL_FACTOR_GRID_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD: &str =
    INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION;

/// Stable provider method identifier when `news_intensity` is materialized from calendar policy.
pub const INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD: &str =
    INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION;
