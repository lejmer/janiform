use crate::hourly_factors::{
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
};

/// Stable provider method identifier when `s_h` is materialized from source samples.
pub const HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD: &str =
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION;

/// Stable provider method identifier when `nu_h` is materialized from calendar policy.
pub const HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD: &str =
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION;
