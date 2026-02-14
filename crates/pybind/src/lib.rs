use pyo3::prelude::*;
use pyo3::types::PyModule;

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__doc__", "Rust bindings for Janiform.")?;
    Ok(())
}
