use serde::Serialize;

/// Audit metadata emitted with hourly factor materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorMaterializationMetadata {
    pub seasonality_materialization_method: String,
    pub news_intensity_materialization_method: String,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for hourly factor interval coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorContractDiagnostics {
    pub interval_count: usize,
    pub interval_duration_nanos: i64,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
    pub skipped_local_hour_interval_count: usize,
    pub repeated_local_hour_interval_count: usize,
}

/// Canonical hourly factor output contract for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyFactorContractOutput {
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
    pub s_h: Vec<f64>,
    pub nu_h: Vec<f64>,
    pub metadata: HourlyFactorMaterializationMetadata,
    pub diagnostics: HourlyFactorContractDiagnostics,
}

/// Audit metadata emitted with hourly seasonality materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlySeasonalityMaterializationMetadata {
    pub interpolation_method: &'static str,
    pub aggregation_method: &'static str,
    pub sparse_fallback_policy: &'static str,
    pub sparse_hour_minimum_observation_count: usize,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for hourly seasonality materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlySeasonalityMaterializationDiagnostics {
    pub source_point_count: usize,
    pub interval_count: usize,
    pub interval_duration_nanos: i64,
    pub sparse_interval_count: usize,
    pub interpolated_interval_count: usize,
    pub fallback_interval_count: usize,
    pub skipped_local_hour_interval_count: usize,
    pub repeated_local_hour_interval_count: usize,
}

/// Canonical hourly seasonality output for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlySeasonalityMaterializationOutput {
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
    pub s_h: Vec<f64>,
    pub metadata: HourlySeasonalityMaterializationMetadata,
    pub diagnostics: HourlySeasonalityMaterializationDiagnostics,
}

/// Audit metadata emitted with hourly news-intensity materialization outputs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyNewsIntensityMaterializationMetadata {
    pub materialization_method: &'static str,
    pub component_aggregation_policy: &'static str,
    pub arrival_intensity_policy: &'static str,
    pub known_news_calendar_anchor_policy: &'static str,
    pub surprise_news_intensity_control_policy: &'static str,
    pub known_news_calendar_anchor_count: usize,
    pub surprise_baseline_intensity: f64,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
    pub timezone_transition_policy: &'static str,
    pub spring_forward_policy: &'static str,
    pub fall_back_policy: &'static str,
}

/// Deterministic diagnostics for hourly news-intensity materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyNewsIntensityMaterializationDiagnostics {
    pub interval_count: usize,
    pub interval_duration_nanos: i64,
    pub known_component_non_zero_interval_count: usize,
    pub surprise_component_non_zero_interval_count: usize,
    pub combined_component_non_zero_interval_count: usize,
    pub known_scheduled_arrival_count: u64,
    pub surprise_multiplier_override_interval_count: usize,
    pub skipped_local_hour_interval_count: usize,
    pub repeated_local_hour_interval_count: usize,
}

/// Canonical hourly news-intensity output for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyNewsIntensityMaterializationOutput {
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
    pub known_component_nu_h: Vec<f64>,
    pub surprise_component_nu_h: Vec<f64>,
    pub nu_h: Vec<f64>,
    pub metadata: HourlyNewsIntensityMaterializationMetadata,
    pub diagnostics: HourlyNewsIntensityMaterializationDiagnostics,
}
