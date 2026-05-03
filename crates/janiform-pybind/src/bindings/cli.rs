use std::ffi::OsString;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::{Bound, PyResult, Python};
use pyo3::types::{PyAnyMethods, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use crate::panic_boundary::panic_to_pyresult;

#[pyfunction]
fn run_cli(py: Python<'_>) -> PyResult<u8> {
    let arguments = py
        .import("sys")?
        .getattr("argv")?
        .extract::<Vec<String>>()?
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<OsString>>();

    panic_to_pyresult("running Rust CLI", PyRuntimeError::new_err, || {
        Ok(py.detach(move || janiform_cli::run_exit_code(arguments) as u8))
    })
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_cli, module)?)?;
    Ok(())
}
