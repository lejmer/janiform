use chrono::{DateTime, Utc};
use janiform_core::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLoader,
    ExchangeCalendarResolveError, MarketState,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;

create_exception!(_native, CalendarInvalidTimezoneError, PyException);
create_exception!(_native, CalendarMissingExchangeError, PyException);
create_exception!(_native, CalendarRegistryLoadError, PyException);
create_exception!(_native, CalendarTimestampParseError, PyValueError);

#[pyclass(module = "janiform._native", name = "CalendarRegistry")]
struct PyCalendarRegistry {
    registry: ExchangeCalendarRegistry,
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
    module.add_class::<PyCalendarRegistry>()?;
    Ok(())
}
