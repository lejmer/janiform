use janiform_core::pricing::{
    VanillaOptionPricingExecutionInput,
    run_vanilla_option_pricing as run_vanilla_option_pricing_core,
};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::PricingExecutionError;

#[pyfunction]
fn run_vanilla_option_pricing(py: Python<'_>, execution_input_json: &str) -> PyResult<String> {
    let execution_input: VanillaOptionPricingExecutionInput =
        serde_json::from_str(execution_input_json).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize vanilla option pricing execution input: {source}"
            ))
        })?;
    let run_result = py
        .allow_threads(|| run_vanilla_option_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    serde_json::to_string(&run_result).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize vanilla option pricing run result: {source}"
        ))
    })
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_vanilla_option_pricing, module)?)?;
    Ok(())
}
