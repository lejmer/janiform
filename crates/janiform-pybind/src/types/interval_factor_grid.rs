use janiform::model::factors::interval_factor_grid::{
    IntervalFactorGrid, IntervalNewsIntensityMaterializationOutput,
    IntervalSeasonalityMaterializationOutput,
};
use janiform::time::calendar::ExchangeCalendarRegistry;
use pyo3::{pyclass, pymethods};

#[pyclass(
    module = "janiform._native",
    name = "IntervalFactorGridContractResult",
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct PyIntervalFactorGridContractResult {
    pub(crate) output: IntervalFactorGrid,
    pub(crate) calendar_registry: Option<ExchangeCalendarRegistry>,
}

#[pyclass(
    module = "janiform._native",
    name = "IntervalSeasonalityMaterializationResult",
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct PyIntervalSeasonalityMaterializationResult {
    pub(crate) output: IntervalSeasonalityMaterializationOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "IntervalNewsIntensityMaterializationResult",
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct PyIntervalNewsIntensityMaterializationResult {
    pub(crate) output: IntervalNewsIntensityMaterializationOutput,
}

#[pymethods]
impl PyIntervalFactorGridContractResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
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
    fn request_start_utc_nanos(&self) -> i64 {
        self.output.request_start_utc_nanos
    }

    #[getter]
    fn request_end_utc_nanos(&self) -> i64 {
        self.output.request_end_utc_nanos
    }

    #[getter]
    fn interval_duration_nanos(&self) -> i64 {
        self.output.interval_duration_nanos
    }

    #[getter]
    fn interval_indices(&self) -> Vec<u64> {
        self.output.interval_indices.clone()
    }

    #[getter]
    fn interval_start_utc_nanos(&self) -> Vec<i64> {
        self.output.interval_start_utc_nanos.clone()
    }

    #[getter]
    fn interval_end_utc_nanos(&self) -> Vec<i64> {
        self.output.interval_end_utc_nanos.clone()
    }

    #[getter]
    fn interval_start_exchange_local(&self) -> Vec<String> {
        self.output.interval_start_exchange_local.clone()
    }

    #[getter]
    fn interval_end_exchange_local(&self) -> Vec<String> {
        self.output.interval_end_exchange_local.clone()
    }

    #[getter]
    fn seasonality_factor(&self) -> Vec<f64> {
        self.output.seasonality_factor.clone()
    }

    #[getter]
    fn known_news_intensity(&self) -> Vec<f64> {
        self.output.known_news_intensity.clone()
    }

    #[getter]
    fn surprise_news_intensity(&self) -> Vec<f64> {
        self.output.surprise_news_intensity.clone()
    }

    #[getter]
    fn news_intensity(&self) -> Vec<f64> {
        self.output.news_intensity.clone()
    }

    #[getter]
    fn interval_count(&self) -> usize {
        self.output.diagnostics.interval_count
    }

    #[getter]
    fn first_interval_start_utc_nanos(&self) -> i64 {
        self.output.diagnostics.first_interval_start_utc_nanos
    }

    #[getter]
    fn last_interval_end_utc_nanos(&self) -> i64 {
        self.output.diagnostics.last_interval_end_utc_nanos
    }

    #[getter]
    fn skipped_exchange_local_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .skipped_exchange_local_interval_count
    }

    #[getter]
    fn repeated_exchange_local_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .repeated_exchange_local_interval_count
    }

    #[getter]
    fn interval_boundary_policy(&self) -> &str {
        self.output.metadata.interval_boundary_policy
    }

    #[getter]
    fn interval_alignment_policy(&self) -> &str {
        self.output.metadata.interval_alignment_policy
    }

    #[getter]
    fn timezone_transition_policy(&self) -> &str {
        self.output.metadata.timezone_transition_policy
    }

    #[getter]
    fn spring_forward_policy(&self) -> &str {
        self.output.metadata.spring_forward_policy
    }

    #[getter]
    fn fall_back_policy(&self) -> &str {
        self.output.metadata.fall_back_policy
    }

    #[getter]
    fn seasonality_materialization_method(&self) -> &str {
        &self.output.metadata.seasonality_materialization_method
    }

    #[getter]
    fn news_intensity_materialization_method(&self) -> &str {
        &self.output.metadata.news_intensity_materialization_method
    }
}

#[pymethods]
impl PyIntervalSeasonalityMaterializationResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
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
    fn request_start_utc_nanos(&self) -> i64 {
        self.output.request_start_utc_nanos
    }

    #[getter]
    fn request_end_utc_nanos(&self) -> i64 {
        self.output.request_end_utc_nanos
    }

    #[getter]
    fn interval_duration_nanos(&self) -> i64 {
        self.output.interval_duration_nanos
    }

    #[getter]
    fn interval_indices(&self) -> Vec<u64> {
        self.output.interval_indices.clone()
    }

    #[getter]
    fn interval_start_utc_nanos(&self) -> Vec<i64> {
        self.output.interval_start_utc_nanos.clone()
    }

    #[getter]
    fn interval_end_utc_nanos(&self) -> Vec<i64> {
        self.output.interval_end_utc_nanos.clone()
    }

    #[getter]
    fn interval_start_exchange_local(&self) -> Vec<String> {
        self.output.interval_start_exchange_local.clone()
    }

    #[getter]
    fn interval_end_exchange_local(&self) -> Vec<String> {
        self.output.interval_end_exchange_local.clone()
    }

    #[getter]
    fn seasonality_factor(&self) -> Vec<f64> {
        self.output.seasonality_factor.clone()
    }

    #[getter]
    fn source_point_count(&self) -> usize {
        self.output.diagnostics.source_point_count
    }

    #[getter]
    fn interval_count(&self) -> usize {
        self.output.diagnostics.interval_count
    }

    #[getter]
    fn sparse_interval_count(&self) -> usize {
        self.output.diagnostics.sparse_interval_count
    }

    #[getter]
    fn interpolated_interval_count(&self) -> usize {
        self.output.diagnostics.interpolated_interval_count
    }

    #[getter]
    fn fallback_interval_count(&self) -> usize {
        self.output.diagnostics.fallback_interval_count
    }

    #[getter]
    fn skipped_exchange_local_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .skipped_exchange_local_interval_count
    }

    #[getter]
    fn repeated_exchange_local_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .repeated_exchange_local_interval_count
    }

    #[getter]
    fn interpolation_method(&self) -> &str {
        self.output.metadata.interpolation_method
    }

    #[getter]
    fn aggregation_method(&self) -> &str {
        self.output.metadata.aggregation_method
    }

    #[getter]
    fn sparse_fallback_policy(&self) -> &str {
        self.output.metadata.sparse_fallback_policy
    }

    #[getter]
    fn sparse_interval_minimum_observation_count(&self) -> usize {
        self.output
            .metadata
            .sparse_interval_minimum_observation_count
    }

    #[getter]
    fn interval_boundary_policy(&self) -> &str {
        self.output.metadata.interval_boundary_policy
    }

    #[getter]
    fn interval_alignment_policy(&self) -> &str {
        self.output.metadata.interval_alignment_policy
    }

    #[getter]
    fn timezone_transition_policy(&self) -> &str {
        self.output.metadata.timezone_transition_policy
    }

    #[getter]
    fn spring_forward_policy(&self) -> &str {
        self.output.metadata.spring_forward_policy
    }

    #[getter]
    fn fall_back_policy(&self) -> &str {
        self.output.metadata.fall_back_policy
    }
}

#[pymethods]
impl PyIntervalNewsIntensityMaterializationResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
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
    fn request_start_utc_nanos(&self) -> i64 {
        self.output.request_start_utc_nanos
    }

    #[getter]
    fn request_end_utc_nanos(&self) -> i64 {
        self.output.request_end_utc_nanos
    }

    #[getter]
    fn interval_duration_nanos(&self) -> i64 {
        self.output.interval_duration_nanos
    }

    #[getter]
    fn interval_indices(&self) -> Vec<u64> {
        self.output.interval_indices.clone()
    }

    #[getter]
    fn interval_start_utc_nanos(&self) -> Vec<i64> {
        self.output.interval_start_utc_nanos.clone()
    }

    #[getter]
    fn interval_end_utc_nanos(&self) -> Vec<i64> {
        self.output.interval_end_utc_nanos.clone()
    }

    #[getter]
    fn interval_start_exchange_local(&self) -> Vec<String> {
        self.output.interval_start_exchange_local.clone()
    }

    #[getter]
    fn interval_end_exchange_local(&self) -> Vec<String> {
        self.output.interval_end_exchange_local.clone()
    }

    #[getter]
    fn known_news_intensity(&self) -> Vec<f64> {
        self.output.known_news_intensity.clone()
    }

    #[getter]
    fn surprise_news_intensity(&self) -> Vec<f64> {
        self.output.surprise_news_intensity.clone()
    }

    #[getter]
    fn news_intensity(&self) -> Vec<f64> {
        self.output.news_intensity.clone()
    }

    #[getter]
    fn interval_count(&self) -> usize {
        self.output.diagnostics.interval_count
    }

    #[getter]
    fn known_component_non_zero_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .known_component_non_zero_interval_count
    }

    #[getter]
    fn surprise_component_non_zero_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .surprise_component_non_zero_interval_count
    }

    #[getter]
    fn combined_component_non_zero_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .combined_component_non_zero_interval_count
    }

    #[getter]
    fn known_scheduled_arrival_count(&self) -> u64 {
        self.output.diagnostics.known_scheduled_arrival_count
    }

    #[getter]
    fn surprise_multiplier_override_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .surprise_multiplier_override_interval_count
    }

    #[getter]
    fn skipped_exchange_local_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .skipped_exchange_local_interval_count
    }

    #[getter]
    fn repeated_exchange_local_interval_count(&self) -> usize {
        self.output
            .diagnostics
            .repeated_exchange_local_interval_count
    }

    #[getter]
    fn materialization_method(&self) -> &str {
        self.output.metadata.materialization_method
    }

    #[getter]
    fn component_aggregation_policy(&self) -> &str {
        self.output.metadata.component_aggregation_policy
    }

    #[getter]
    fn arrival_intensity_policy(&self) -> &str {
        self.output.metadata.arrival_intensity_policy
    }

    #[getter]
    fn known_news_calendar_anchor_policy(&self) -> &str {
        self.output.metadata.known_news_calendar_anchor_policy
    }

    #[getter]
    fn surprise_news_intensity_control_policy(&self) -> &str {
        self.output.metadata.surprise_news_intensity_control_policy
    }

    #[getter]
    fn known_news_calendar_anchor_count(&self) -> usize {
        self.output.metadata.known_news_calendar_anchor_count
    }

    #[getter]
    fn surprise_open_baseline_intensity(&self) -> f64 {
        self.output.metadata.surprise_open_baseline_intensity
    }

    #[getter]
    fn surprise_closed_baseline_intensity(&self) -> f64 {
        self.output.metadata.surprise_closed_baseline_intensity
    }

    #[getter]
    fn interval_boundary_policy(&self) -> &str {
        self.output.metadata.interval_boundary_policy
    }

    #[getter]
    fn interval_alignment_policy(&self) -> &str {
        self.output.metadata.interval_alignment_policy
    }

    #[getter]
    fn timezone_transition_policy(&self) -> &str {
        self.output.metadata.timezone_transition_policy
    }

    #[getter]
    fn spring_forward_policy(&self) -> &str {
        self.output.metadata.spring_forward_policy
    }

    #[getter]
    fn fall_back_policy(&self) -> &str {
        self.output.metadata.fall_back_policy
    }
}
