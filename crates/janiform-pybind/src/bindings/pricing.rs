mod events;
mod output;
mod request;
mod workflow;

use pyo3::prelude::{Bound, PyResult};
use pyo3::types::PyModule;

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    workflow::register_functions(module)
}
