use janiform::calendar::ExchangeCalendarRegistry;
use pyo3::prelude::*;

#[pyclass(module = "janiform._native", name = "CalendarRegistry")]
pub(crate) struct PyCalendarRegistry {
    pub(crate) registry: ExchangeCalendarRegistry,
}
