use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use crate::SimulationExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(validate_coarse_simulation_path, module)?)?;

    Ok(())
}

#[pyfunction]
fn validate_coarse_simulation_path(
    py: Python<'_>,
    path_payload: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let coarse_path: janiform::simulation::CoarseSimulationPath =
        serde_json::from_value(py_any_to_json_value(path_payload)?).map_err(|source| {
            SimulationExecutionError::new_err(format!(
                "failed to deserialize coarse simulation path: {source}"
            ))
        })?;

    coarse_path.validate().map_err(|source| {
        SimulationExecutionError::new_err(format!("invalid coarse simulation path: {source}"))
    })?;

    let validated_payload = serde_json::to_value(&coarse_path).map_err(|source| {
        SimulationExecutionError::new_err(format!(
            "failed to serialize validated coarse simulation path: {source}"
        ))
    })?;

    json_value_to_py_object(py, &validated_payload)
}
