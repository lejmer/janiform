use janiform::foundation::hashing::fnv1a_64_hex_lowercase;
use pyo3::prelude::{Bound, PyResult};
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

#[pyfunction]
fn compute_fnv1a_64_hex_lowercase(payload: &[u8]) -> String {
    fnv1a_64_hex_lowercase(payload)
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(compute_fnv1a_64_hex_lowercase, module)?)?;
    Ok(())
}
