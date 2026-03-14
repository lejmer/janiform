use janiform_core::pricing::{
    VanillaOptionPricingExecutionInput,
    run_vanilla_option_pricing as run_vanilla_option_pricing_core,
};
use pyo3::prelude::*;
use pyo3::types::PyAny;
use pyo3::types::PyModule;

use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::PricingExecutionError;

#[pyfunction]
fn run_vanilla_option_pricing(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let execution_input: VanillaOptionPricingExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize vanilla option pricing execution input: {source}"
            ))
        })?;
    let run_result = py
        .allow_threads(|| run_vanilla_option_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    let serialized_run_result = serde_json::to_value(&run_result).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize vanilla option pricing run result: {source}"
        ))
    })
    ?;

    json_value_to_py_object(py, &serialized_run_result)
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_vanilla_option_pricing, module)?)?;
    Ok(())
}
