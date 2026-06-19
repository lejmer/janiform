mod calendar;
mod calibration;
mod cli;
mod events;
mod fit;
mod forecast;
mod foundation;
mod interval_factor_grid;
mod pricing;
mod regression;
mod simulation;
mod stylized_facts;

use pyo3::prelude::{Bound, PyResult};
use pyo3::types::PyModule;

pub(crate) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    calibration::register_functions(module)?;
    cli::register_functions(module)?;
    fit::register_functions(module)?;
    forecast::register_functions(module)?;
    foundation::register_functions(module)?;
    interval_factor_grid::register_functions(module)?;
    pricing::register_functions(module)?;
    regression::register_functions(module)?;
    simulation::register_functions(module)?;
    stylized_facts::register_functions(module)?;
    Ok(())
}
