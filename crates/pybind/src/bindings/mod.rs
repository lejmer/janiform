mod calendar;
mod hourly;
mod pricing;
mod regression;
mod seasonality;
mod simulation;
mod stylized_facts;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    seasonality::register_functions(module)?;
    hourly::register_functions(module)?;
    pricing::register_functions(module)?;
    regression::register_functions(module)?;
    simulation::register_functions(module)?;
    stylized_facts::register_functions(module)?;
    Ok(())
}
