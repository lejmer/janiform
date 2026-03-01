use chrono::{DateTime, Utc};

/// Seasonality source accepted by the shared hourly factor provider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HourlySeasonalityFactorSource<'a> {
    /// Use already materialized hourly values.
    Explicit {
        hourly_factors: &'a [f64],
        materialization_method: &'a str,
    },

    /// Materialize hourly values from timestamped seasonality samples.
    Materialized {
        source_timestamp_utc_nanos: &'a [i64],
        source_seasonality_factors: &'a [f64],
        interpolation_method: &'a str,
        aggregation_method: &'a str,
        sparse_fallback_policy: &'a str,
        sparse_hour_minimum_observation_count: usize,
    },
}

/// News intensity source accepted by the shared hourly factor provider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HourlyNewsIntensityFactorSource<'a> {
    /// Use already materialized hourly values.
    Explicit {
        hourly_factors: &'a [f64],
        materialization_method: &'a str,
    },

    /// Materialize hourly values from known-news calendar anchors and surprise controls.
    Materialized {
        known_scheduled_event_timestamps_utc_nanos: &'a [i64],
        surprise_baseline_intensity: f64,
        surprise_deterministic_time_multiplier_by_interval_index: &'a [(u64, f64)],
    },
}

/// Request model for the shared hourly factor provider.
#[derive(Debug, Clone, PartialEq)]
pub struct HourlyFactorProviderRequest<'a> {
    pub exchange_code: &'a str,
    pub range_start_utc: DateTime<Utc>,
    pub range_end_utc: DateTime<Utc>,
    pub seasonality_source: Option<HourlySeasonalityFactorSource<'a>>,
    pub news_intensity_source: Option<HourlyNewsIntensityFactorSource<'a>>,
}
