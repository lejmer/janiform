pub(crate) mod workflow;

use pyo3::prelude::{Bound, PyResult};
use pyo3::types::PyModule;

pub(crate) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    workflow::register_functions(module)?;
    Ok(())
}
