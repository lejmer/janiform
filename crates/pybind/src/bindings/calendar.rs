use chrono::{DateTime, Utc};
use janiform_core::calendar::{
    ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLoader,
    ExchangeCalendarResolveError, MarketState, TimestampEventInputRecord,
    TimestampEventPreprocessingError, TimestampEventSessionFilter,
};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyMapping};

use crate::types::{PyCalendarRegistry, PyTimestampEventPreprocessingResult};
use crate::{
    CalendarInvalidTimezoneError, CalendarMissingExchangeError, CalendarRegistryLoadError,
    CalendarTimestampParseError, SeasonalityInputValidationError,
};

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

pub(super) fn parse_timestamp_utc(raw_timestamp: &str) -> PyResult<DateTime<Utc>> {
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

pub(super) fn parse_timestamp_utc_from_python(
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
