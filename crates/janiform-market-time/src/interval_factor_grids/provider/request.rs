use chrono::{DateTime, Utc};

/// Seasonality source accepted by the shared interval factor grid provider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntervalSeasonalityFactorSource<'a> {
    /// Use already materialized interval factors.
    Explicit {
        interval_factors: &'a [f64],
        materialization_method: &'a str,
    },

    /// Materialize interval factors from timestamped seasonality samples.
    Materialized {
        source_timestamp_utc_nanos: &'a [i64],
        source_seasonality_factors: &'a [f64],
        interpolation_method: &'a str,
        aggregation_method: &'a str,
        sparse_fallback_policy: &'a str,
        sparse_interval_minimum_observation_count: usize,
    },
}

/// News intensity source accepted by the shared interval factor grid provider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntervalNewsIntensityFactorSource<'a> {
    /// Use already materialized interval factors.
    Explicit {
        interval_factors: &'a [f64],
        materialization_method: &'a str,
    },

    /// Materialize interval factors from known-news calendar anchors and surprise controls.
    Materialized {
        known_scheduled_event_timestamps_utc_nanos: &'a [i64],
        surprise_open_baseline_intensity: f64,
        surprise_closed_baseline_intensity: f64,
        surprise_deterministic_time_multiplier_by_interval_index: &'a [(u64, f64)],
    },
}

/// Request model for the shared interval factor grid provider.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalFactorGridProviderRequest<'a> {
    pub exchange_code: &'a str,
    pub range_start_utc: DateTime<Utc>,
    pub range_end_utc: DateTime<Utc>,
    pub interval_duration_nanos: i64,
    pub seasonality_source: Option<IntervalSeasonalityFactorSource<'a>>,
    pub news_intensity_source: Option<IntervalNewsIntensityFactorSource<'a>>,
}
