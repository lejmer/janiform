use janiform::calendar::TimestampEventPreprocessingOutput;
use janiform::seasonality::{
    DayOfWeekSeasonalityEstimateOutput, DayOfYearSeasonalityEstimateOutput,
    DeterministicSeasonalityCompositionOutput, IntradaySeasonalityEstimateOutput,
};
use pyo3::prelude::*;

#[pyclass(
    module = "janiform._native",
    name = "TimestampEventPreprocessingResult"
)]
#[derive(Clone)]
pub(crate) struct PyTimestampEventPreprocessingResult {
    pub(crate) output: TimestampEventPreprocessingOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "IntradaySeasonalityEstimateResult"
)]
#[derive(Clone)]
pub(crate) struct PyIntradaySeasonalityEstimateResult {
    pub(crate) output: IntradaySeasonalityEstimateOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "DayOfWeekSeasonalityEstimateResult"
)]
#[derive(Clone)]
pub(crate) struct PyDayOfWeekSeasonalityEstimateResult {
    pub(crate) output: DayOfWeekSeasonalityEstimateOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "DayOfYearSeasonalityEstimateResult"
)]
#[derive(Clone)]
pub(crate) struct PyDayOfYearSeasonalityEstimateResult {
    pub(crate) output: DayOfYearSeasonalityEstimateOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "DeterministicSeasonalityCompositionResult"
)]
#[derive(Clone)]
pub(crate) struct PyDeterministicSeasonalityCompositionResult {
    pub(crate) output: DeterministicSeasonalityCompositionOutput,
}

#[pymethods]
impl PyTimestampEventPreprocessingResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn input_schema_version(&self) -> &str {
        self.output.input_schema_version
    }

    #[getter]
    fn exchange_code(&self) -> &str {
        &self.output.exchange_code
    }

    #[getter]
    fn exchange_timezone_name(&self) -> &str {
        &self.output.exchange_timezone_name
    }

    #[getter]
    fn source_record_positions(&self) -> Vec<u64> {
        self.output.source_record_positions.clone()
    }

    #[getter]
    fn timestamp_utc_nanos(&self) -> Vec<i64> {
        self.output.timestamp_utc_nanos.clone()
    }

    #[getter]
    fn exchange_local_minute_of_day(&self) -> Vec<u16> {
        self.output.exchange_local_minute_of_day.clone()
    }

    #[getter]
    fn exchange_local_day_of_week(&self) -> Vec<u8> {
        self.output.exchange_local_day_of_week.clone()
    }

    #[getter]
    fn exchange_local_day_of_year(&self) -> Vec<u16> {
        self.output.exchange_local_day_of_year.clone()
    }

    #[getter]
    fn market_state_codes(&self) -> Vec<u8> {
        self.output.market_state_codes.clone()
    }

    #[getter]
    fn missing_hour_bucket_utc_nanos(&self) -> Vec<i64> {
        self.output.missing_hour_bucket_utc_nanos.clone()
    }

    #[getter]
    fn sparse_hour_bucket_utc_nanos(&self) -> Vec<i64> {
        self.output.sparse_hour_bucket_utc_nanos.clone()
    }

    #[getter]
    fn input_record_count(&self) -> usize {
        self.output.diagnostics.input_record_count
    }

    #[getter]
    fn validated_record_count(&self) -> usize {
        self.output.diagnostics.validated_record_count
    }

    #[getter]
    fn accepted_record_count(&self) -> usize {
        self.output.diagnostics.accepted_record_count
    }

    #[getter]
    fn filtered_record_count(&self) -> usize {
        self.output.diagnostics.filtered_record_count
    }

    #[getter]
    fn first_event_timestamp_utc_nanos(&self) -> Option<i64> {
        self.output.diagnostics.first_event_timestamp_utc_nanos
    }

    #[getter]
    fn last_event_timestamp_utc_nanos(&self) -> Option<i64> {
        self.output.diagnostics.last_event_timestamp_utc_nanos
    }

    #[getter]
    fn hour_bucket_count(&self) -> usize {
        self.output.diagnostics.hour_bucket_count
    }

    #[getter]
    fn missing_hour_bucket_count(&self) -> usize {
        self.output.diagnostics.missing_hour_bucket_count
    }

    #[getter]
    fn sparse_hour_bucket_count(&self) -> usize {
        self.output.diagnostics.sparse_hour_bucket_count
    }
}

#[pymethods]
impl PyIntradaySeasonalityEstimateResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn minutes_per_bin(&self) -> u16 {
        self.output.minutes_per_bin
    }

    #[getter]
    fn smoothing_kernel(&self) -> &str {
        self.output.smoothing_kernel.as_str()
    }

    #[getter]
    fn smoothing_bandwidth_bins(&self) -> f64 {
        self.output.smoothing_bandwidth_bins
    }

    #[getter]
    fn minimum_bin_events(&self) -> usize {
        self.output.minimum_bin_events
    }

    #[getter]
    fn pseudocount(&self) -> f64 {
        self.output.pseudocount
    }

    #[getter]
    fn bin_start_minute_of_day(&self) -> Vec<u16> {
        self.output.bin_start_minute_of_day.clone()
    }

    #[getter]
    fn event_count_by_bin(&self) -> Vec<u64> {
        self.output.event_count_by_bin.clone()
    }

    #[getter]
    fn sparse_bin_flags(&self) -> Vec<u8> {
        self.output.sparse_bin_flags.clone()
    }

    #[getter]
    fn smoothed_density_by_bin(&self) -> Vec<f64> {
        self.output.smoothed_density_by_bin.clone()
    }

    #[getter]
    fn intraday_log_component_by_bin(&self) -> Vec<f64> {
        self.output.intraday_log_component_by_bin.clone()
    }

    #[getter]
    fn intraday_factor_by_bin(&self) -> Vec<f64> {
        self.output.intraday_factor_by_bin.clone()
    }

    #[getter]
    fn input_event_count(&self) -> usize {
        self.output.diagnostics.input_event_count
    }

    #[getter]
    fn bin_count(&self) -> usize {
        self.output.diagnostics.bin_count
    }

    #[getter]
    fn zero_event_bin_count(&self) -> usize {
        self.output.diagnostics.zero_event_bin_count
    }

    #[getter]
    fn sparse_bin_count(&self) -> usize {
        self.output.diagnostics.sparse_bin_count
    }
}

#[pymethods]
impl PyDayOfWeekSeasonalityEstimateResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn minimum_bucket_events(&self) -> usize {
        self.output.minimum_bucket_events
    }

    #[getter]
    fn pseudocount(&self) -> f64 {
        self.output.pseudocount
    }

    #[getter]
    fn day_of_week_number(&self) -> Vec<u8> {
        self.output.day_of_week_number.clone()
    }

    #[getter]
    fn event_count_by_day_of_week(&self) -> Vec<u64> {
        self.output.event_count_by_day_of_week.clone()
    }

    #[getter]
    fn sparse_bucket_flags(&self) -> Vec<u8> {
        self.output.sparse_bucket_flags.clone()
    }

    #[getter]
    fn fallback_bucket_flags(&self) -> Vec<u8> {
        self.output.fallback_bucket_flags.clone()
    }

    #[getter]
    fn day_of_week_log_component(&self) -> Vec<f64> {
        self.output.day_of_week_log_component.clone()
    }

    #[getter]
    fn day_of_week_factor(&self) -> Vec<f64> {
        self.output.day_of_week_factor.clone()
    }

    #[getter]
    fn input_event_count(&self) -> usize {
        self.output.diagnostics.input_event_count
    }

    #[getter]
    fn bucket_count(&self) -> usize {
        self.output.diagnostics.bucket_count
    }

    #[getter]
    fn zero_event_bucket_count(&self) -> usize {
        self.output.diagnostics.zero_event_bucket_count
    }

    #[getter]
    fn sparse_bucket_count(&self) -> usize {
        self.output.diagnostics.sparse_bucket_count
    }

    #[getter]
    fn fallback_bucket_count(&self) -> usize {
        self.output.diagnostics.fallback_bucket_count
    }
}

#[pymethods]
impl PyDayOfYearSeasonalityEstimateResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn minimum_bucket_events(&self) -> usize {
        self.output.minimum_bucket_events
    }

    #[getter]
    fn pseudocount(&self) -> f64 {
        self.output.pseudocount
    }

    #[getter]
    fn day_of_year_number(&self) -> Vec<u16> {
        self.output.day_of_year_number.clone()
    }

    #[getter]
    fn event_count_by_day_of_year(&self) -> Vec<u64> {
        self.output.event_count_by_day_of_year.clone()
    }

    #[getter]
    fn sparse_bucket_flags(&self) -> Vec<u8> {
        self.output.sparse_bucket_flags.clone()
    }

    #[getter]
    fn fallback_bucket_flags(&self) -> Vec<u8> {
        self.output.fallback_bucket_flags.clone()
    }

    #[getter]
    fn day_of_year_log_component(&self) -> Vec<f64> {
        self.output.day_of_year_log_component.clone()
    }

    #[getter]
    fn day_of_year_factor(&self) -> Vec<f64> {
        self.output.day_of_year_factor.clone()
    }

    #[getter]
    fn input_event_count(&self) -> usize {
        self.output.diagnostics.input_event_count
    }

    #[getter]
    fn bucket_count(&self) -> usize {
        self.output.diagnostics.bucket_count
    }

    #[getter]
    fn zero_event_bucket_count(&self) -> usize {
        self.output.diagnostics.zero_event_bucket_count
    }

    #[getter]
    fn sparse_bucket_count(&self) -> usize {
        self.output.diagnostics.sparse_bucket_count
    }

    #[getter]
    fn fallback_bucket_count(&self) -> usize {
        self.output.diagnostics.fallback_bucket_count
    }
}

#[pymethods]
impl PyDeterministicSeasonalityCompositionResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn intraday_minutes_per_bin(&self) -> u16 {
        self.output.intraday_minutes_per_bin
    }

    #[getter]
    fn intraday_bin_index_by_event(&self) -> Vec<u16> {
        self.output.intraday_bin_index_by_event.clone()
    }

    #[getter]
    fn intraday_log_component_by_event(&self) -> Vec<f64> {
        self.output.intraday_log_component_by_event.clone()
    }

    #[getter]
    fn day_of_week_log_component_by_event(&self) -> Vec<f64> {
        self.output.day_of_week_log_component_by_event.clone()
    }

    #[getter]
    fn day_of_year_log_component_by_event(&self) -> Vec<f64> {
        self.output.day_of_year_log_component_by_event.clone()
    }

    #[getter]
    fn special_log_component_by_event(&self) -> Vec<f64> {
        self.output.special_log_component_by_event.clone()
    }

    #[getter]
    fn special_overlay_active_flags_by_event(&self) -> Vec<u8> {
        self.output.special_overlay_active_flags_by_event.clone()
    }

    #[getter]
    fn active_overlay_ids_by_event(&self) -> Vec<String> {
        self.output.active_overlay_ids_by_event.clone()
    }

    #[getter]
    fn combined_log_component_by_event(&self) -> Vec<f64> {
        self.output.combined_log_component_by_event.clone()
    }

    #[getter]
    fn combined_factor_by_event(&self) -> Vec<f64> {
        self.output.combined_factor_by_event.clone()
    }

    #[getter]
    fn input_event_count(&self) -> usize {
        self.output.diagnostics.input_event_count
    }

    #[getter]
    fn normalization_policy(&self) -> &str {
        self.output
            .diagnostics
            .normalization_metadata
            .normalization_policy
    }

    #[getter]
    fn fit_window_event_count(&self) -> usize {
        self.output
            .diagnostics
            .normalization_metadata
            .fit_window_event_count
    }

    #[getter]
    fn target_mean_factor(&self) -> f64 {
        self.output
            .diagnostics
            .normalization_metadata
            .target_mean_factor
    }

    #[getter]
    fn pre_normalization_mean_factor(&self) -> f64 {
        self.output
            .diagnostics
            .normalization_metadata
            .pre_normalization_mean_factor
    }

    #[getter]
    fn normalization_constant(&self) -> f64 {
        self.output
            .diagnostics
            .normalization_metadata
            .normalization_constant
    }

    #[getter]
    fn normalization_log_offset(&self) -> f64 {
        self.output
            .diagnostics
            .normalization_metadata
            .normalization_log_offset
    }

    #[getter]
    fn post_normalization_mean_factor(&self) -> f64 {
        self.output
            .diagnostics
            .normalization_metadata
            .post_normalization_mean_factor
    }

    #[getter]
    fn mean_anchor_tolerance(&self) -> f64 {
        self.output
            .diagnostics
            .normalization_metadata
            .mean_anchor_tolerance
    }

    #[getter]
    fn overlays_enabled(&self) -> bool {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .overlays_enabled
    }

    #[getter]
    fn configured_overlay_count(&self) -> usize {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .configured_overlay_count
    }

    #[getter]
    fn enabled_overlay_count(&self) -> usize {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .enabled_overlay_count
    }

    #[getter]
    fn active_overlay_event_count(&self) -> usize {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .active_overlay_event_count
    }

    #[getter]
    fn overlay_precedence_policy(&self) -> &str {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .precedence_policy
    }

    #[getter]
    fn overlay_merge_policy(&self) -> &str {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .merge_policy
    }

    #[getter]
    fn overlay_window_boundary_policy(&self) -> &str {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .window_boundary_policy
    }

    #[getter]
    fn configured_overlay_ids_in_order(&self) -> Vec<String> {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .configured_overlay_ids_in_order
            .clone()
    }

    #[getter]
    fn applied_overlay_ids_in_order(&self) -> Vec<String> {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .applied_overlay_ids_in_order
            .clone()
    }

    #[getter]
    fn overlay_source_labels_in_order(&self) -> Vec<String> {
        self.output
            .diagnostics
            .overlay_provenance_metadata
            .overlay_source_labels_in_order
            .clone()
    }
}
