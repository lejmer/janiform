use serde::{Deserialize, Serialize};

/// Audit metadata emitted with interval factor grid materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalFactorGridMetadata {
    pub seasonality_materialization_method: String,
    pub news_intensity_materialization_method: String,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for interval factor grid interval coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalFactorGridDiagnostics {
    pub interval_count: usize,
    pub interval_duration_nanos: i64,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
    pub skipped_exchange_local_interval_count: usize,
    pub repeated_exchange_local_interval_count: usize,
}

/// Canonical interval factor grid output contract for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntervalFactorGrid {
    pub schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_duration_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub seasonality_factor: Vec<f64>,
    pub known_news_intensity: Vec<f64>,
    pub surprise_news_intensity: Vec<f64>,
    pub news_intensity: Vec<f64>,
    pub metadata: IntervalFactorGridMetadata,
    pub diagnostics: IntervalFactorGridDiagnostics,
}

/// Audit metadata emitted with interval seasonality materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalSeasonalityMaterializationMetadata {
    pub interpolation_method: &'static str,
    pub aggregation_method: &'static str,
    pub sparse_fallback_policy: &'static str,
    pub sparse_interval_minimum_observation_count: usize,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for interval seasonality materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalSeasonalityMaterializationDiagnostics {
    pub source_point_count: usize,
    pub interval_count: usize,
    pub interval_duration_nanos: i64,
    pub sparse_interval_count: usize,
    pub interpolated_interval_count: usize,
    pub fallback_interval_count: usize,
    pub skipped_exchange_local_interval_count: usize,
    pub repeated_exchange_local_interval_count: usize,
}

/// Canonical interval seasonality output for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntervalSeasonalityMaterializationOutput {
    pub schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_duration_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub seasonality_factor: Vec<f64>,
    pub metadata: IntervalSeasonalityMaterializationMetadata,
    pub diagnostics: IntervalSeasonalityMaterializationDiagnostics,
}

/// Audit metadata emitted with interval news-intensity materialization outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntervalNewsIntensityMaterializationMetadata {
    pub materialization_method: &'static str,
    pub component_aggregation_policy: &'static str,
    pub arrival_intensity_policy: &'static str,
    pub known_news_calendar_anchor_policy: &'static str,
    pub surprise_news_intensity_control_policy: &'static str,
    pub known_news_calendar_anchor_count: usize,
    pub surprise_open_baseline_intensity: f64,
    pub surprise_closed_baseline_intensity: f64,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for interval news-intensity materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalNewsIntensityMaterializationDiagnostics {
    pub interval_count: usize,
    pub interval_duration_nanos: i64,
    pub known_component_non_zero_interval_count: usize,
    pub surprise_component_non_zero_interval_count: usize,
    pub combined_component_non_zero_interval_count: usize,
    pub known_scheduled_arrival_count: u64,
    pub surprise_multiplier_override_interval_count: usize,
    pub skipped_exchange_local_interval_count: usize,
    pub repeated_exchange_local_interval_count: usize,
}

/// Canonical interval news-intensity output for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntervalNewsIntensityMaterializationOutput {
    pub schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_duration_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub known_news_intensity: Vec<f64>,
    pub surprise_news_intensity: Vec<f64>,
    pub news_intensity: Vec<f64>,
    pub metadata: IntervalNewsIntensityMaterializationMetadata,
    pub diagnostics: IntervalNewsIntensityMaterializationDiagnostics,
}
