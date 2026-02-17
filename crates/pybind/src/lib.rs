use chrono::{DateTime, Utc};
use janiform_core::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLoader,
    ExchangeCalendarResolveError, MarketState, TimestampEventInputRecord,
    TimestampEventPreprocessingError, TimestampEventPreprocessingOutput,
    TimestampEventSessionFilter,
};
use janiform_core::hourly_factors::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, HourlyFactorContractError,
    HourlyFactorContractOutput, build_hourly_factor_contract as build_hourly_factor_contract_core,
};
use janiform_core::seasonality::{
    DayOfWeekSeasonalityEstimateOutput, DayOfWeekSeasonalityEstimationError,
    DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityEstimateOutput,
    DayOfYearSeasonalityEstimationError, DayOfYearSeasonalityEstimatorSettings,
    DeterministicSeasonalityCompositionError, DeterministicSeasonalityCompositionOutput,
    IntradaySeasonalityEstimateOutput, IntradaySeasonalityEstimationError,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel, ScheduledEventOverlayDefinition,
    SeasonalityDiagnosticsReportError, SeasonalityDiagnosticsReportInput,
    compose_deterministic_seasonality_signal as compose_deterministic_seasonality_signal_core,
    estimate_day_of_week_seasonality as estimate_day_of_week_seasonality_core,
    estimate_day_of_year_seasonality as estimate_day_of_year_seasonality_core,
    estimate_intraday_seasonality as estimate_intraday_seasonality_core,
    generate_seasonality_diagnostics_report as generate_seasonality_diagnostics_report_core,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyMapping, PyModule};
use serde_json::to_string as serialize_to_json_string;

create_exception!(_native, CalendarInvalidTimezoneError, PyException);
create_exception!(_native, CalendarMissingExchangeError, PyException);
create_exception!(_native, CalendarRegistryLoadError, PyException);
create_exception!(_native, CalendarTimestampParseError, PyValueError);
create_exception!(_native, SeasonalityInputValidationError, PyValueError);

#[pyclass(module = "janiform._native", name = "CalendarRegistry")]
struct PyCalendarRegistry {
    registry: ExchangeCalendarRegistry,
}

#[pyclass(
    module = "janiform._native",
    name = "TimestampEventPreprocessingResult"
)]
#[derive(Clone)]
struct PyTimestampEventPreprocessingResult {
    output: TimestampEventPreprocessingOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "IntradaySeasonalityEstimateResult"
)]
#[derive(Clone)]
struct PyIntradaySeasonalityEstimateResult {
    output: IntradaySeasonalityEstimateOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "DayOfWeekSeasonalityEstimateResult"
)]
#[derive(Clone)]
struct PyDayOfWeekSeasonalityEstimateResult {
    output: DayOfWeekSeasonalityEstimateOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "DayOfYearSeasonalityEstimateResult"
)]
#[derive(Clone)]
struct PyDayOfYearSeasonalityEstimateResult {
    output: DayOfYearSeasonalityEstimateOutput,
}

#[pyclass(
    module = "janiform._native",
    name = "DeterministicSeasonalityCompositionResult"
)]
#[derive(Clone)]
struct PyDeterministicSeasonalityCompositionResult {
    output: DeterministicSeasonalityCompositionOutput,
}

#[pyclass(module = "janiform._native", name = "HourlyFactorContractResult")]
#[derive(Clone)]
struct PyHourlyFactorContractResult {
    output: HourlyFactorContractOutput,
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

#[pymethods]
impl PyHourlyFactorContractResult {
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
    fn s_h(&self) -> Vec<f64> {
        self.output.s_h.clone()
    }

    #[getter]
    fn nu_h(&self) -> Vec<f64> {
        self.output.nu_h.clone()
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
    fn interval_boundary_policy(&self) -> &str {
        self.output.metadata.interval_boundary_policy
    }

    #[getter]
    fn interval_alignment_policy(&self) -> &str {
        self.output.metadata.interval_alignment_policy
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
impl PyCalendarRegistry {
    #[new]
    fn new(registry_json: &str) -> PyResult<Self> {
        let registry = ExchangeCalendarRegistryLoader::load_from_json(registry_json)
            .map_err(map_registry_load_error)?;
        Ok(Self { registry })
    }

    fn resolve_market_state(&self, exchange_code: &str, timestamp_utc: &str) -> PyResult<String> {
        let timestamp = parse_timestamp_utc(timestamp_utc)?;
        let market_state = self
            .registry
            .resolve_market_state_at_utc(exchange_code, timestamp)
            .map_err(map_resolve_error)?;
        Ok(match market_state {
            MarketState::Open => "open".to_string(),
            MarketState::Closed => "closed".to_string(),
        })
    }

    #[pyo3(signature = (exchange_code, event_records, timestamp_field_name = "timestamp", session_filter = "open_only", sparse_hour_event_threshold = 1))]
    fn preprocess_timestamp_events(
        &self,
        py: Python<'_>,
        exchange_code: &str,
        event_records: &Bound<'_, PyAny>,
        timestamp_field_name: &str,
        session_filter: &str,
        sparse_hour_event_threshold: usize,
    ) -> PyResult<PyTimestampEventPreprocessingResult> {
        let parsed_records = parse_timestamp_event_records(event_records, timestamp_field_name)?;
        let parsed_session_filter = parse_session_filter(session_filter)?;

        let output = py
            .allow_threads(|| {
                self.registry.preprocess_timestamp_events(
                    exchange_code,
                    parsed_records,
                    parsed_session_filter,
                    sparse_hour_event_threshold,
                )
            })
            .map_err(map_preprocessing_error)?;

        Ok(PyTimestampEventPreprocessingResult { output })
    }
}

fn parse_timestamp_utc(raw_timestamp: &str) -> PyResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw_timestamp)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| {
            CalendarTimestampParseError::new_err(
                "timestamp_utc must be an RFC3339 timestamp with timezone offset",
            )
        })
}

fn map_registry_load_error(error: ExchangeCalendarRegistryLoadError) -> PyErr {
    CalendarRegistryLoadError::new_err(error.to_string())
}

fn map_resolve_error(error: ExchangeCalendarResolveError) -> PyErr {
    match error {
        ExchangeCalendarResolveError::MissingExchangeId { exchange_code } => {
            CalendarMissingExchangeError::new_err(format!(
                "missing exchange id `{exchange_code}` in registry"
            ))
        }
        ExchangeCalendarResolveError::InvalidTimezoneName {
            exchange_code,
            timezone_name,
        } => CalendarInvalidTimezoneError::new_err(format!(
            "invalid timezone `{timezone_name}` configured for exchange `{exchange_code}`"
        )),
    }
}

fn map_preprocessing_error(error: TimestampEventPreprocessingError) -> PyErr {
    match error {
        TimestampEventPreprocessingError::InvalidSparseHourEventThreshold {
            sparse_hour_event_threshold,
        } => SeasonalityInputValidationError::new_err(format!(
            "sparse_hour_event_threshold must be at least 1, received {sparse_hour_event_threshold}"
        )),
        TimestampEventPreprocessingError::TimestampOutOfRange {
            source_record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {source_record_position} has a timestamp outside the supported nanosecond range"
        )),
        TimestampEventPreprocessingError::CalendarResolve { source } => map_resolve_error(source),
    }
}

fn parse_session_filter(raw_session_filter: &str) -> PyResult<TimestampEventSessionFilter> {
    match raw_session_filter {
        "all" => Ok(TimestampEventSessionFilter::All),
        "open_only" => Ok(TimestampEventSessionFilter::OpenOnly),
        "closed_only" => Ok(TimestampEventSessionFilter::ClosedOnly),
        _ => Err(SeasonalityInputValidationError::new_err(format!(
            "unsupported session filter value: {raw_session_filter}"
        ))),
    }
}

#[pyfunction]
#[pyo3(signature = (exchange_local_minute_of_day, *, minutes_per_bin = 1, smoothing_kernel = "gaussian", smoothing_bandwidth_bins = 3.0, minimum_bin_events = 1, pseudocount = 1e-6))]
fn estimate_intraday_seasonality(
    py: Python<'_>,
    exchange_local_minute_of_day: &Bound<'_, PyAny>,
    minutes_per_bin: u16,
    smoothing_kernel: &str,
    smoothing_bandwidth_bins: f64,
    minimum_bin_events: usize,
    pseudocount: f64,
) -> PyResult<PyIntradaySeasonalityEstimateResult> {
    let minute_of_day_values: Vec<u16> = exchange_local_minute_of_day.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_minute_of_day must be an iterable of integer minute-of-day values",
        )
    })?;
    let parsed_smoothing_kernel = parse_intraday_smoothing_kernel(smoothing_kernel)?;
    let estimator_settings = IntradaySeasonalityEstimatorSettings {
        minutes_per_bin,
        smoothing_kernel: parsed_smoothing_kernel,
        smoothing_bandwidth_bins,
        minimum_bin_events,
        pseudocount,
    };

    let output = py
        .allow_threads(|| {
            estimate_intraday_seasonality_core(&minute_of_day_values, estimator_settings)
        })
        .map_err(map_intraday_estimation_error)?;

    Ok(PyIntradaySeasonalityEstimateResult { output })
}

#[pyfunction]
#[pyo3(signature = (exchange_local_day_of_week, *, minimum_bucket_events = 1, pseudocount = 1e-6))]
fn estimate_day_of_week_seasonality(
    py: Python<'_>,
    exchange_local_day_of_week: &Bound<'_, PyAny>,
    minimum_bucket_events: usize,
    pseudocount: f64,
) -> PyResult<PyDayOfWeekSeasonalityEstimateResult> {
    let day_of_week_values: Vec<u8> = exchange_local_day_of_week.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_week must be an iterable of integer day-of-week values",
        )
    })?;
    let estimator_settings = DayOfWeekSeasonalityEstimatorSettings {
        minimum_bucket_events,
        pseudocount,
    };

    let output = py
        .allow_threads(|| {
            estimate_day_of_week_seasonality_core(&day_of_week_values, estimator_settings)
        })
        .map_err(map_day_of_week_estimation_error)?;

    Ok(PyDayOfWeekSeasonalityEstimateResult { output })
}

#[pyfunction]
#[pyo3(signature = (exchange_local_day_of_year, *, minimum_bucket_events = 1, pseudocount = 1e-6))]
fn estimate_day_of_year_seasonality(
    py: Python<'_>,
    exchange_local_day_of_year: &Bound<'_, PyAny>,
    minimum_bucket_events: usize,
    pseudocount: f64,
) -> PyResult<PyDayOfYearSeasonalityEstimateResult> {
    let day_of_year_values: Vec<u16> = exchange_local_day_of_year.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_year must be an iterable of integer day-of-year values",
        )
    })?;
    let estimator_settings = DayOfYearSeasonalityEstimatorSettings {
        minimum_bucket_events,
        pseudocount,
    };

    let output = py
        .allow_threads(|| {
            estimate_day_of_year_seasonality_core(&day_of_year_values, estimator_settings)
        })
        .map_err(map_day_of_year_estimation_error)?;

    Ok(PyDayOfYearSeasonalityEstimateResult { output })
}

#[pyfunction]
#[pyo3(signature = (timestamp_utc_nanos, exchange_local_minute_of_day, exchange_local_day_of_week, exchange_local_day_of_year, intraday_log_component_by_bin, day_of_week_log_component, day_of_year_log_component, *, intraday_minutes_per_bin = 1, scheduled_event_overlays = None))]
fn compose_deterministic_seasonality_signal(
    py: Python<'_>,
    timestamp_utc_nanos: &Bound<'_, PyAny>,
    exchange_local_minute_of_day: &Bound<'_, PyAny>,
    exchange_local_day_of_week: &Bound<'_, PyAny>,
    exchange_local_day_of_year: &Bound<'_, PyAny>,
    intraday_log_component_by_bin: &Bound<'_, PyAny>,
    day_of_week_log_component: &Bound<'_, PyAny>,
    day_of_year_log_component: &Bound<'_, PyAny>,
    intraday_minutes_per_bin: u16,
    scheduled_event_overlays: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyDeterministicSeasonalityCompositionResult> {
    let timestamp_utc_nanos_values: Vec<i64> = timestamp_utc_nanos.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "timestamp_utc_nanos must be an iterable of integer UTC nanosecond values",
        )
    })?;
    let minute_of_day_values: Vec<u16> = exchange_local_minute_of_day.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_minute_of_day must be an iterable of integer minute-of-day values",
        )
    })?;
    let day_of_week_values: Vec<u8> = exchange_local_day_of_week.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_week must be an iterable of integer day-of-week values",
        )
    })?;
    let day_of_year_values: Vec<u16> = exchange_local_day_of_year.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_year must be an iterable of integer day-of-year values",
        )
    })?;
    let intraday_log_values: Vec<f64> = intraday_log_component_by_bin.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "intraday_log_component_by_bin must be an iterable of float values",
        )
    })?;
    let day_of_week_log_values: Vec<f64> = day_of_week_log_component.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "day_of_week_log_component must be an iterable of float values",
        )
    })?;
    let day_of_year_log_values: Vec<f64> = day_of_year_log_component.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "day_of_year_log_component must be an iterable of float values",
        )
    })?;
    let overlay_definitions = parse_scheduled_event_overlay_definitions(scheduled_event_overlays)?;

    let output = py
        .allow_threads(|| {
            compose_deterministic_seasonality_signal_core(
                &timestamp_utc_nanos_values,
                &minute_of_day_values,
                &day_of_week_values,
                &day_of_year_values,
                intraday_minutes_per_bin,
                &intraday_log_values,
                &day_of_week_log_values,
                &day_of_year_log_values,
                &overlay_definitions,
            )
        })
        .map_err(map_deterministic_composition_error)?;

    Ok(PyDeterministicSeasonalityCompositionResult { output })
}

#[pyfunction]
#[pyo3(signature = (dataset_id, preprocessed_events, intraday_estimate, day_of_week_estimate, day_of_year_estimate, composition, *, estimator_version = "seasonality_estimator.v1"))]
fn generate_seasonality_diagnostics_report(
    py: Python<'_>,
    dataset_id: &str,
    preprocessed_events: &PyTimestampEventPreprocessingResult,
    intraday_estimate: &PyIntradaySeasonalityEstimateResult,
    day_of_week_estimate: &PyDayOfWeekSeasonalityEstimateResult,
    day_of_year_estimate: &PyDayOfYearSeasonalityEstimateResult,
    composition: &PyDeterministicSeasonalityCompositionResult,
    estimator_version: &str,
) -> PyResult<String> {
    let report_output = py
        .allow_threads(|| {
            generate_seasonality_diagnostics_report_core(SeasonalityDiagnosticsReportInput {
                dataset_id,
                input_schema_version: preprocessed_events.output.input_schema_version,
                preprocessed_schema_version: preprocessed_events.output.schema_version,
                exchange_code: &preprocessed_events.output.exchange_code,
                exchange_timezone_name: &preprocessed_events.output.exchange_timezone_name,
                source_record_positions: &preprocessed_events.output.source_record_positions,
                timestamp_utc_nanos: &preprocessed_events.output.timestamp_utc_nanos,
                exchange_local_minute_of_day: &preprocessed_events
                    .output
                    .exchange_local_minute_of_day,
                exchange_local_day_of_week: &preprocessed_events.output.exchange_local_day_of_week,
                exchange_local_day_of_year: &preprocessed_events.output.exchange_local_day_of_year,
                market_state_codes: &preprocessed_events.output.market_state_codes,
                intraday_estimate: &intraday_estimate.output,
                day_of_week_estimate: &day_of_week_estimate.output,
                day_of_year_estimate: &day_of_year_estimate.output,
                composition: &composition.output,
                estimator_version,
            })
        })
        .map_err(map_seasonality_diagnostics_report_error)?;

    serialize_to_json_string(&report_output).map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "failed to serialize seasonality diagnostics report to json",
        )
    })
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, seasonality_hourly_factors, news_intensity_hourly_factors, *, seasonality_materialization_method = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, news_intensity_materialization_method = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD))]
fn build_hourly_factor_contract(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    seasonality_hourly_factors: &Bound<'_, PyAny>,
    news_intensity_hourly_factors: &Bound<'_, PyAny>,
    seasonality_materialization_method: &str,
    news_intensity_materialization_method: &str,
) -> PyResult<PyHourlyFactorContractResult> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;
    let parsed_seasonality_hourly_factors: Vec<f64> =
        seasonality_hourly_factors.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(
                "seasonality_hourly_factors must be an iterable of float values",
            )
        })?;
    let parsed_news_intensity_hourly_factors: Vec<f64> =
        news_intensity_hourly_factors.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(
                "news_intensity_hourly_factors must be an iterable of float values",
            )
        })?;

    let output = py
        .allow_threads(|| {
            build_hourly_factor_contract_core(
                &calendar_registry.registry,
                exchange_code,
                parsed_range_start_utc,
                parsed_range_end_utc,
                &parsed_seasonality_hourly_factors,
                &parsed_news_intensity_hourly_factors,
                seasonality_materialization_method,
                news_intensity_materialization_method,
            )
        })
        .map_err(map_hourly_factor_contract_error)?;

    Ok(PyHourlyFactorContractResult { output })
}

fn parse_scheduled_event_overlay_definitions(
    scheduled_event_overlays: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<ScheduledEventOverlayDefinition>> {
    let Some(raw_scheduled_event_overlays) = scheduled_event_overlays else {
        return Ok(Vec::new());
    };
    if raw_scheduled_event_overlays.is_none() {
        return Ok(Vec::new());
    }

    let overlay_iterator = raw_scheduled_event_overlays.try_iter().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "scheduled_event_overlays must be an iterable of mapping records",
        )
    })?;

    let mut overlays: Vec<ScheduledEventOverlayDefinition> = Vec::new();
    for (overlay_position, overlay_record_result) in overlay_iterator.enumerate() {
        let overlay_record = overlay_record_result?;
        let overlay_mapping = overlay_record.downcast::<PyMapping>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} must be a mapping"
            ))
        })?;

        let overlay_id_value = overlay_mapping
            .contains("overlay_id")?
            .then(|| overlay_mapping.get_item("overlay_id"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'overlay_id'"
                ))
            })??;
        let overlay_id: String = overlay_id_value.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} field 'overlay_id' must be a string"
            ))
        })?;

        let start_timestamp_value = overlay_mapping
            .contains("start_timestamp_utc")?
            .then(|| overlay_mapping.get_item("start_timestamp_utc"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'start_timestamp_utc'"
                ))
            })??;
        let start_timestamp_utc = parse_timestamp_utc_from_python(
            &start_timestamp_value,
            overlay_position,
            "start_timestamp_utc",
        )?;
        let start_timestamp_utc_nanos =
            start_timestamp_utc
                .timestamp_nanos_opt()
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(format!(
                        "scheduled_event_overlay at position {overlay_position} field 'start_timestamp_utc' is outside the supported nanosecond range"
                    ))
                })?;

        let end_timestamp_value = overlay_mapping
            .contains("end_timestamp_utc")?
            .then(|| overlay_mapping.get_item("end_timestamp_utc"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'end_timestamp_utc'"
                ))
            })??;
        let end_timestamp_utc = parse_timestamp_utc_from_python(
            &end_timestamp_value,
            overlay_position,
            "end_timestamp_utc",
        )?;
        let end_timestamp_utc_nanos = end_timestamp_utc.timestamp_nanos_opt().ok_or_else(|| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} field 'end_timestamp_utc' is outside the supported nanosecond range"
            ))
        })?;

        let log_adjustment_value = overlay_mapping
            .contains("log_adjustment")?
            .then(|| overlay_mapping.get_item("log_adjustment"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'log_adjustment'"
                ))
            })??;
        let log_adjustment: f64 = log_adjustment_value.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} field 'log_adjustment' must be a float"
            ))
        })?;

        let precedence = if overlay_mapping.contains("precedence")? {
            let precedence_value = overlay_mapping.get_item("precedence")?;
            precedence_value.extract::<u16>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} field 'precedence' must be an integer in the range 0..65535"
                ))
            })?
        } else {
            0_u16
        };
        let enabled = if overlay_mapping.contains("enabled")? {
            let enabled_value = overlay_mapping.get_item("enabled")?;
            enabled_value.extract::<bool>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} field 'enabled' must be a boolean"
                ))
            })?
        } else {
            true
        };
        let source_label = if overlay_mapping.contains("source_label")? {
            let source_label_value = overlay_mapping.get_item("source_label")?;
            source_label_value.extract::<String>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} field 'source_label' must be a string"
                ))
            })?
        } else {
            "configuration".to_string()
        };

        overlays.push(ScheduledEventOverlayDefinition {
            overlay_id,
            start_timestamp_utc_nanos,
            end_timestamp_utc_nanos,
            log_adjustment,
            precedence,
            enabled,
            source_label,
        });
    }

    Ok(overlays)
}

fn parse_intraday_smoothing_kernel(
    raw_smoothing_kernel: &str,
) -> PyResult<IntradaySmoothingKernel> {
    match raw_smoothing_kernel {
        "gaussian" => Ok(IntradaySmoothingKernel::Gaussian),
        "uniform" => Ok(IntradaySmoothingKernel::Uniform),
        _ => Err(SeasonalityInputValidationError::new_err(format!(
            "unsupported intraday smoothing kernel value: {raw_smoothing_kernel}"
        ))),
    }
}

fn map_intraday_estimation_error(error: IntradaySeasonalityEstimationError) -> PyErr {
    match error {
        IntradaySeasonalityEstimationError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "intraday seasonality estimation requires at least one accepted event",
            )
        }
        IntradaySeasonalityEstimationError::InvalidMinutesPerBin { minutes_per_bin } => {
            SeasonalityInputValidationError::new_err(format!(
                "minutes_per_bin must be a positive divisor of 1440, received {minutes_per_bin}"
            ))
        }
        IntradaySeasonalityEstimationError::InvalidSmoothingBandwidthBins {
            smoothing_bandwidth_bins,
        } => SeasonalityInputValidationError::new_err(format!(
            "smoothing_bandwidth_bins must be a finite positive value, received {smoothing_bandwidth_bins}"
        )),
        IntradaySeasonalityEstimationError::InvalidMinimumBinEvents { minimum_bin_events } => {
            SeasonalityInputValidationError::new_err(format!(
                "minimum_bin_events must be at least 1, received {minimum_bin_events}"
            ))
        }
        IntradaySeasonalityEstimationError::InvalidPseudocount { pseudocount } => {
            SeasonalityInputValidationError::new_err(format!(
                "pseudocount must be a finite positive value, received {pseudocount}"
            ))
        }
        IntradaySeasonalityEstimationError::InvalidMinuteOfDay {
            record_position,
            minute_of_day,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local minute-of-day value {minute_of_day}"
        )),
        IntradaySeasonalityEstimationError::UnstableNormalization => {
            SeasonalityInputValidationError::new_err(
                "intraday seasonality estimation produced a non-positive normalization constant",
            )
        }
    }
}

fn map_day_of_week_estimation_error(error: DayOfWeekSeasonalityEstimationError) -> PyErr {
    match error {
        DayOfWeekSeasonalityEstimationError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "day-of-week seasonality estimation requires at least one accepted event",
            )
        }
        DayOfWeekSeasonalityEstimationError::InvalidMinimumBucketEvents {
            minimum_bucket_events,
        } => SeasonalityInputValidationError::new_err(format!(
            "minimum_bucket_events must be at least 1, received {minimum_bucket_events}"
        )),
        DayOfWeekSeasonalityEstimationError::InvalidPseudocount { pseudocount } => {
            SeasonalityInputValidationError::new_err(format!(
                "pseudocount must be a finite positive value, received {pseudocount}"
            ))
        }
        DayOfWeekSeasonalityEstimationError::InvalidDayOfWeek {
            record_position,
            day_of_week,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-week value {day_of_week}"
        )),
        DayOfWeekSeasonalityEstimationError::UnstableNormalization => {
            SeasonalityInputValidationError::new_err(
                "day-of-week seasonality estimation produced a non-positive normalization constant",
            )
        }
    }
}

fn map_day_of_year_estimation_error(error: DayOfYearSeasonalityEstimationError) -> PyErr {
    match error {
        DayOfYearSeasonalityEstimationError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "day-of-year seasonality estimation requires at least one accepted event",
            )
        }
        DayOfYearSeasonalityEstimationError::InvalidMinimumBucketEvents {
            minimum_bucket_events,
        } => SeasonalityInputValidationError::new_err(format!(
            "minimum_bucket_events must be at least 1, received {minimum_bucket_events}"
        )),
        DayOfYearSeasonalityEstimationError::InvalidPseudocount { pseudocount } => {
            SeasonalityInputValidationError::new_err(format!(
                "pseudocount must be a finite positive value, received {pseudocount}"
            ))
        }
        DayOfYearSeasonalityEstimationError::InvalidDayOfYear {
            record_position,
            day_of_year,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-year value {day_of_year}"
        )),
        DayOfYearSeasonalityEstimationError::UnstableNormalization => {
            SeasonalityInputValidationError::new_err(
                "day-of-year seasonality estimation produced a non-positive normalization constant",
            )
        }
    }
}

fn map_deterministic_composition_error(error: DeterministicSeasonalityCompositionError) -> PyErr {
    match error {
        DeterministicSeasonalityCompositionError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "deterministic seasonality composition requires at least one accepted event",
            )
        }
        DeterministicSeasonalityCompositionError::MismatchedInputLengths {
            timestamp_utc_nanos_count,
            minute_of_day_count,
            day_of_week_count,
            day_of_year_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "composition inputs must have matching lengths, received timestamp_utc_nanos={timestamp_utc_nanos_count}, minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidIntradayMinutesPerBin {
            intraday_minutes_per_bin,
        } => SeasonalityInputValidationError::new_err(format!(
            "intraday_minutes_per_bin must be a positive divisor of 1440, received {intraday_minutes_per_bin}"
        )),
        DeterministicSeasonalityCompositionError::InvalidIntradayComponentLength {
            expected_intraday_bin_count,
            actual_intraday_bin_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "intraday component length mismatch, expected {expected_intraday_bin_count} bins and received {actual_intraday_bin_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfWeekComponentLength {
            expected_day_of_week_bucket_count,
            actual_day_of_week_bucket_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "day-of-week component length mismatch, expected {expected_day_of_week_bucket_count} buckets and received {actual_day_of_week_bucket_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfYearComponentLength {
            expected_day_of_year_bucket_count,
            actual_day_of_year_bucket_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "day-of-year component length mismatch, expected {expected_day_of_year_bucket_count} buckets and received {actual_day_of_year_bucket_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidMinuteOfDay {
            record_position,
            minute_of_day,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local minute-of-day value {minute_of_day}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfWeek {
            record_position,
            day_of_week,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-week value {day_of_week}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfYear {
            record_position,
            day_of_year,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-year value {day_of_year}"
        )),
        DeterministicSeasonalityCompositionError::EmptyOverlayId { overlay_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled event overlay at position {overlay_position} is missing a non-empty overlay_id"
            ))
        }
        DeterministicSeasonalityCompositionError::DuplicateOverlayId { overlay_id } => {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled event overlay id '{overlay_id}' is duplicated and must be unique"
            ))
        }
        DeterministicSeasonalityCompositionError::InvalidOverlayWindow {
            overlay_position,
            start_timestamp_utc_nanos,
            end_timestamp_utc_nanos,
        } => SeasonalityInputValidationError::new_err(format!(
            "scheduled event overlay at position {overlay_position} has an invalid window where start={start_timestamp_utc_nanos} and end={end_timestamp_utc_nanos}"
        )),
        DeterministicSeasonalityCompositionError::InvalidOverlayLogAdjustment {
            overlay_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "scheduled event overlay at position {overlay_position} has a non-finite log_adjustment"
        )),
        DeterministicSeasonalityCompositionError::InvalidSpecialOverlayLogComponent {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality composition produced a non-finite special overlay log component at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::InvalidCombinedLogComponent {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality composition produced a non-finite combined log component at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::UnstableCombinedFactor { record_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "deterministic seasonality composition produced a non-finite combined factor at record position {record_position}"
            ))
        }
        DeterministicSeasonalityCompositionError::InvalidNormalizationInputFactor {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality normalization received a non-finite or non-positive factor at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::InvalidNormalizationInputLogComponent {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality normalization received a non-finite log component at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::UnstableNormalizationConstant => {
            SeasonalityInputValidationError::new_err(
                "deterministic seasonality normalization produced a non-positive normalization constant",
            )
        }
        DeterministicSeasonalityCompositionError::IdentifiabilityGuardViolation {
            anchored_mean_factor,
            target_mean_factor,
            mean_anchor_tolerance,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality normalization failed identifiability guard, anchored mean={anchored_mean_factor}, target mean={target_mean_factor}, tolerance={mean_anchor_tolerance}"
        )),
    }
}

fn map_hourly_factor_contract_error(error: HourlyFactorContractError) -> PyErr {
    match error {
        HourlyFactorContractError::MissingExchangeId { exchange_code } => {
            CalendarMissingExchangeError::new_err(format!(
                "missing exchange id `{exchange_code}` in registry"
            ))
        }
        HourlyFactorContractError::InvalidTimezoneName {
            exchange_code,
            timezone_name,
        } => CalendarInvalidTimezoneError::new_err(format!(
            "invalid timezone `{timezone_name}` configured for exchange `{exchange_code}`"
        )),
        HourlyFactorContractError::EmptyExchangeCode => {
            SeasonalityInputValidationError::new_err("exchange_code must be a non-empty string")
        }
        HourlyFactorContractError::EmptySeasonalityMaterializationMethod => {
            SeasonalityInputValidationError::new_err(
                "seasonality_materialization_method must be a non-empty string",
            )
        }
        HourlyFactorContractError::EmptyNewsIntensityMaterializationMethod => {
            SeasonalityInputValidationError::new_err(
                "news_intensity_materialization_method must be a non-empty string",
            )
        }
        HourlyFactorContractError::TimestampOutOfRange { field } => {
            SeasonalityInputValidationError::new_err(format!(
                "{field} must be within the supported nanosecond timestamp range"
            ))
        }
        HourlyFactorContractError::NonIncreasingRange {
            range_start_utc_nanos,
            range_end_utc_nanos,
        } => SeasonalityInputValidationError::new_err(format!(
            "range_start_utc must be earlier than range_end_utc, received start={range_start_utc_nanos}, end={range_end_utc_nanos}"
        )),
        HourlyFactorContractError::RangeBoundaryNotHourAligned {
            field,
            timestamp_utc_nanos,
        } => SeasonalityInputValidationError::new_err(format!(
            "{field} must be aligned to an exact UTC hour boundary, received {timestamp_utc_nanos}"
        )),
        HourlyFactorContractError::FactorLengthMismatch {
            expected_interval_count,
            seasonality_factor_count,
            news_intensity_factor_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "hourly factor length mismatch, expected {expected_interval_count} intervals and received s_h={seasonality_factor_count}, nu_h={news_intensity_factor_count}"
        )),
        HourlyFactorContractError::InvalidSeasonalityFactor {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "s_h must contain finite positive values, found {value} at interval index {interval_index}"
        )),
        HourlyFactorContractError::InvalidNewsIntensityFactor {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "nu_h must contain finite non-negative values, found {value} at interval index {interval_index}"
        )),
    }
}

fn map_seasonality_diagnostics_report_error(error: SeasonalityDiagnosticsReportError) -> PyErr {
    match error {
        SeasonalityDiagnosticsReportError::EmptyDatasetId => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty dataset_id",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyEstimatorVersion => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty estimator_version",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyInputSchemaVersion => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty input_schema_version",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyPreprocessedSchemaVersion => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty preprocessed_schema_version",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyExchangeCode => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty exchange_code",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyExchangeTimezoneName => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty exchange_timezone_name",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires at least one accepted input event",
            )
        }
        SeasonalityDiagnosticsReportError::MismatchedInputLengths {
            timestamp_utc_nanos_count,
            source_record_position_count,
            minute_of_day_count,
            day_of_week_count,
            day_of_year_count,
            market_state_code_count,
            combined_log_component_count,
            combined_factor_count,
            special_log_component_count,
            special_overlay_flag_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report inputs must have matching lengths, received timestamp_utc_nanos={timestamp_utc_nanos_count}, source_record_positions={source_record_position_count}, minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}, market_state_codes={market_state_code_count}, combined_log_component={combined_log_component_count}, combined_factor={combined_factor_count}, special_log_component={special_log_component_count}, special_overlay_flags={special_overlay_flag_count}"
        )),
        SeasonalityDiagnosticsReportError::InputEventCountMismatch {
            component_name,
            expected_input_event_count,
            actual_input_event_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' input_event_count mismatch, expected {expected_input_event_count} and received {actual_input_event_count}"
        )),
        SeasonalityDiagnosticsReportError::ComponentLengthMismatch {
            component_name,
            expected_bucket_count,
            actual_bucket_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' length mismatch, expected {expected_bucket_count} buckets and received {actual_bucket_count}"
        )),
        SeasonalityDiagnosticsReportError::InvalidComponentPseudocount {
            component_name,
            pseudocount,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' has invalid pseudocount value {pseudocount}"
        )),
        SeasonalityDiagnosticsReportError::InvalidComponentFactor {
            component_name,
            bucket_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' has non-finite or non-positive factor at bucket position {bucket_position}"
        )),
        SeasonalityDiagnosticsReportError::InvalidCombinedFactor { record_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality diagnostics report received non-finite or non-positive combined factor at record position {record_position}"
            ))
        }
        SeasonalityDiagnosticsReportError::InvalidCombinedLogComponent { record_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality diagnostics report received non-finite combined log component at record position {record_position}"
            ))
        }
        SeasonalityDiagnosticsReportError::InvalidSpecialOverlayFlag {
            record_position,
            overlay_active_flag,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report received invalid special overlay active flag {overlay_active_flag} at record position {record_position}"
        )),
        SeasonalityDiagnosticsReportError::EmptyBatchInput => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics batch report requires at least one dataset input",
            )
        }
        SeasonalityDiagnosticsReportError::DuplicateDatasetId { dataset_id } => {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality diagnostics batch report received duplicate dataset id '{dataset_id}'"
            ))
        }
    }
}

fn parse_timestamp_event_records(
    event_records: &Bound<'_, PyAny>,
    timestamp_field_name: &str,
) -> PyResult<Vec<TimestampEventInputRecord>> {
    let iterator = event_records.try_iter().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "event_records must be an iterable of mapping records",
        )
    })?;
    let mut parsed_records: Vec<TimestampEventInputRecord> = Vec::new();
    for (record_position, record_value_result) in iterator.enumerate() {
        let record_value = record_value_result?;
        let record_mapping = record_value.downcast::<PyMapping>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "event record at position {record_position} must be a mapping"
            ))
        })?;
        let timestamp_value = record_mapping
            .contains(timestamp_field_name)?
            .then(|| record_mapping.get_item(timestamp_field_name))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "event record at position {record_position} is missing required field '{timestamp_field_name}'"
                ))
            })??;
        let timestamp_utc = parse_timestamp_utc_from_python(
            &timestamp_value,
            record_position,
            timestamp_field_name,
        )?;
        parsed_records.push(TimestampEventInputRecord {
            source_record_position: record_position as u64,
            timestamp_utc,
        });
    }

    Ok(parsed_records)
}

fn parse_timestamp_utc_from_python(
    timestamp_value: &Bound<'_, PyAny>,
    record_position: usize,
    timestamp_field_name: &str,
) -> PyResult<DateTime<Utc>> {
    if let Ok(timestamp_text) = timestamp_value.extract::<String>() {
        return parse_timestamp_utc_from_text(
            &timestamp_text,
            record_position,
            timestamp_field_name,
        );
    }

    if let Ok(timezone_value) = timestamp_value.getattr("tzinfo") {
        if timezone_value.is_none() {
            return Err(SeasonalityInputValidationError::new_err(format!(
                "event record at position {record_position} has a naive timestamp in field '{timestamp_field_name}'"
            )));
        }

        if let Ok(timestamp_text) = timestamp_value.call_method0("isoformat") {
            let timestamp_text_value: String = timestamp_text.extract()?;
            return parse_timestamp_utc_from_text(
                &timestamp_text_value,
                record_position,
                timestamp_field_name,
            );
        }
    }

    Err(SeasonalityInputValidationError::new_err(format!(
        "event record at position {record_position} has unsupported timestamp value type in field '{timestamp_field_name}'"
    )))
}

fn parse_timestamp_utc_from_text(
    raw_timestamp: &str,
    record_position: usize,
    timestamp_field_name: &str,
) -> PyResult<DateTime<Utc>> {
    let timestamp_text = raw_timestamp.trim();
    let normalized_timestamp = timestamp_text
        .strip_suffix('Z')
        .map(|timestamp_without_z| format!("{timestamp_without_z}+00:00"))
        .unwrap_or_else(|| timestamp_text.to_string());

    DateTime::parse_from_rfc3339(&normalized_timestamp)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "event record at position {record_position} has invalid timestamp in field '{timestamp_field_name}': '{timestamp_text}'"
            ))
        })
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__doc__", "Rust bindings for Janiform.")?;
    module.add(
        "CalendarRegistryLoadError",
        module.py().get_type::<CalendarRegistryLoadError>(),
    )?;
    module.add(
        "CalendarMissingExchangeError",
        module.py().get_type::<CalendarMissingExchangeError>(),
    )?;
    module.add(
        "CalendarInvalidTimezoneError",
        module.py().get_type::<CalendarInvalidTimezoneError>(),
    )?;
    module.add(
        "CalendarTimestampParseError",
        module.py().get_type::<CalendarTimestampParseError>(),
    )?;
    module.add(
        "SeasonalityInputValidationError",
        module.py().get_type::<SeasonalityInputValidationError>(),
    )?;
    module.add_class::<PyCalendarRegistry>()?;
    module.add_class::<PyTimestampEventPreprocessingResult>()?;
    module.add_class::<PyIntradaySeasonalityEstimateResult>()?;
    module.add_class::<PyDayOfWeekSeasonalityEstimateResult>()?;
    module.add_class::<PyDayOfYearSeasonalityEstimateResult>()?;
    module.add_class::<PyDeterministicSeasonalityCompositionResult>()?;
    module.add_class::<PyHourlyFactorContractResult>()?;
    module.add_function(pyo3::wrap_pyfunction!(
        estimate_intraday_seasonality,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        estimate_day_of_week_seasonality,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        estimate_day_of_year_seasonality,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        compose_deterministic_seasonality_signal,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        generate_seasonality_diagnostics_report,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        build_hourly_factor_contract,
        module
    )?)?;
    Ok(())
}
