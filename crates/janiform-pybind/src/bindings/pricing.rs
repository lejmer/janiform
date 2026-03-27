use janiform::pricing::{
    VanillaOptionPricingExecutionInput, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingIntervalFactors,
    run_vanilla_option_pricing as run_vanilla_option_pricing_core,
};
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::PyAny;
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use crate::PricingExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::PyHourlyFactorContractResult;

#[pyfunction]
fn run_vanilla_option_pricing(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    hourly_factor_contract: &PyHourlyFactorContractResult,
    optional_output_selection: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let request: VanillaOptionPricingRequest =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize vanilla option pricing request: {source}"
            ))
        })?;
    let optional_output_selection: VanillaOptionPricingOptionalOutputSelection =
        serde_json::from_value(py_any_to_json_value(optional_output_selection)?).map_err(
            |source| {
                PricingExecutionError::new_err(format!(
                    "failed to deserialize vanilla option pricing optional output selection: {source}"
                ))
            },
        )?;
    let interval_factors = hourly_factor_contract
        .output
        .s_h
        .iter()
        .zip(hourly_factor_contract.output.known_component_nu_h.iter())
        .zip(hourly_factor_contract.output.surprise_component_nu_h.iter())
        .map(|((seasonality_factor, known_component_nu_h), surprise_component_nu_h)| {
            VanillaOptionPricingIntervalFactors::try_new(
                *seasonality_factor,
                *known_component_nu_h,
                *surprise_component_nu_h,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;
    let execution_input =
        VanillaOptionPricingExecutionInput::try_new(request, interval_factors, optional_output_selection)
            .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;
    let run_result = py
        .allow_threads(|| run_vanilla_option_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    let serialized_run_result = serde_json::to_value(&run_result).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize vanilla option pricing run result: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_run_result)
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_vanilla_option_pricing, module)?)?;
    Ok(())
}
