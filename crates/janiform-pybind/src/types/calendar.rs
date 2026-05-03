use janiform::time::calendar::ExchangeCalendarRegistry;
use pyo3::pyclass;

#[pyclass(module = "janiform._native", name = "CalendarRegistry")]
pub(crate) struct PyCalendarRegistry {
    pub(crate) registry: ExchangeCalendarRegistry,
}
