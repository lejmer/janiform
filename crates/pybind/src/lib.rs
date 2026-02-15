use chrono::{DateTime, Utc};
use janiform_core::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLoader,
    ExchangeCalendarResolveError, MarketState, TimestampEventInputRecord,
    TimestampEventPreprocessingError, TimestampEventPreprocessingOutput,
    TimestampEventSessionFilter,
};
use janiform_core::seasonality::{
    IntradaySeasonalityEstimateOutput, IntradaySeasonalityEstimationError,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel,
    estimate_intraday_seasonality as estimate_intraday_seasonality_core,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyMapping, PyModule};

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
    module.add_function(pyo3::wrap_pyfunction!(
        estimate_intraday_seasonality,
        module
    )?)?;
    Ok(())
}
