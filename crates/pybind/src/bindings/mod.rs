mod calendar;
mod hourly;
mod regression;
mod seasonality;
mod simulation;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    seasonality::register_functions(module)?;
    hourly::register_functions(module)?;
    regression::register_functions(module)?;
    simulation::register_functions(module)?;
    Ok(())
}
