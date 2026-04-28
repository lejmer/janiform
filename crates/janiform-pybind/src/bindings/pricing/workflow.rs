use janiform::pricing::{
    HighLevelPricingMaturityResolution, HighLevelPricingMaturityResolutionRequest,
    PricingProgressEvent,
    derive_activity_compressed_pricing_interval_schedule as derive_activity_compressed_pricing_interval_schedule_core,
    resolve_high_level_pricing_maturity_inputs as resolve_high_level_pricing_maturity_inputs_core,
    run_vanilla_option_grid_batch_pricing as run_vanilla_option_grid_batch_pricing_core,
    run_vanilla_option_grid_batch_pricing_with_observer as run_vanilla_option_grid_batch_pricing_core_with_observer,
    run_vanilla_option_pricing as run_vanilla_option_pricing_core,
    run_vanilla_option_surface_pricing as run_vanilla_option_surface_pricing_core,
    run_vanilla_option_surface_pricing_with_observer as run_vanilla_option_surface_pricing_core_with_observer,
};
use janiform_workflows::pricing::{
    run_fitted_model_single_option_pricing_with_terminal_progress,
    run_vanilla_option_grid_batch_pricing_with_terminal_progress as run_grid_batch_pricing_with_terminal_progress,
    run_vanilla_option_surface_pricing_with_terminal_progress as run_surface_pricing_with_terminal_progress,
};
use janiform_workflows::terminal::TerminalProgressMode;
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::PyAny;
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use crate::PricingExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};

use super::events::run_pricing_workflow_with_events;
use super::output::{pricing_result_to_py_object, workflow_error_to_pricing_error};
use super::request::{
    FittedModelSingleOptionPricingWorkflowPayload, PricingActivityCompressedScheduleRequestPayload,
    grid_batch_execution_input_from_py, hourly_factor_contract_output_from_payload,
    surface_execution_input_from_py, vanilla_option_pricing_execution_input_from_py,
};

fn parse_pricing_progress_mode(value: &str) -> PyResult<TerminalProgressMode> {
    TerminalProgressMode::from_boundary_value(value)
        .map_err(|message| PricingExecutionError::new_err(format!("pricing {message}")))
}

#[pyfunction]
fn run_fitted_model_single_option_pricing_workflow(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let payload: FittedModelSingleOptionPricingWorkflowPayload =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize fitted-model single-option pricing request: {source}"
            ))
        })?;

    let output = py
        .detach(move || {
            run_fitted_model_single_option_pricing_with_terminal_progress(
                payload.request,
                payload.progress_mode,
            )
        })
        .map_err(workflow_error_to_pricing_error)?;

    pricing_result_to_py_object(
        py,
        &output.run_result,
        "fitted-model single-option pricing result",
    )
}

#[pyfunction]
fn resolve_high_level_pricing_maturity_inputs(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let request: HighLevelPricingMaturityResolutionRequest =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize high-level pricing maturity resolution request: {source}"
            ))
        })?;

    let resolution: HighLevelPricingMaturityResolution =
        resolve_high_level_pricing_maturity_inputs_core(request)
            .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    pricing_result_to_py_object(
        py,
        &resolution,
        "high-level pricing maturity resolution output",
    )
}

#[pyfunction]
fn derive_activity_compressed_pricing_interval_schedule(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let request: PricingActivityCompressedScheduleRequestPayload = serde_json::from_value(
        py_any_to_json_value(request)?,
    )
    .map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to deserialize activity-compressed pricing interval schedule request: {source}"
        ))
    })?;

    let hourly_factor_contract =
        hourly_factor_contract_output_from_payload(request.hourly_factor_contract);

    let derived_schedule = py
        .detach(|| {
            derive_activity_compressed_pricing_interval_schedule_core(
                request.valuation_time_utc_nanos,
                request.expiry_timestamps_utc_nanos.as_slice(),
                &hourly_factor_contract,
            )
        })
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    pricing_result_to_py_object(
        py,
        &derived_schedule,
        "activity-compressed pricing interval schedule output",
    )
}

#[pyfunction]
fn run_vanilla_option_pricing(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let execution_input = vanilla_option_pricing_execution_input_from_py(execution_input)?;

    let run_result = py
        .detach(|| run_vanilla_option_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    pricing_result_to_py_object(py, &run_result, "vanilla option pricing run result")
}

#[pyfunction]
fn run_vanilla_option_grid_batch_pricing(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let execution_input = grid_batch_execution_input_from_py(execution_input)?;

    let run_result = py
        .detach(|| run_vanilla_option_grid_batch_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    pricing_result_to_py_object(
        py,
        &run_result,
        "vanilla option grid batch pricing run result",
    )
}

#[pyfunction]
#[pyo3(signature = (execution_input, progress_mode))]
fn run_vanilla_option_grid_batch_pricing_with_terminal_progress(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    progress_mode: &str,
) -> PyResult<Py<PyAny>> {
    let execution_input = grid_batch_execution_input_from_py(execution_input)?;
    let progress_mode = parse_pricing_progress_mode(progress_mode)?;

    let run_result = py
        .detach(move || {
            run_grid_batch_pricing_with_terminal_progress(execution_input, progress_mode)
        })
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    pricing_result_to_py_object(
        py,
        &run_result,
        "vanilla option grid batch pricing run result",
    )
}

#[pyfunction]
#[pyo3(signature = (execution_input, event_callback))]
fn run_vanilla_option_grid_batch_pricing_with_events(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    event_callback: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let execution_input = grid_batch_execution_input_from_py(execution_input)?;
    let execution_input = serde_json::to_value(execution_input).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize vanilla option grid batch pricing execution input: {source}"
        ))
    })?;

    let execution_input = json_value_to_py_object(py, &execution_input)?;

    run_pricing_workflow_with_events(
        py,
        execution_input.bind(py),
        event_callback,
        "vanilla option grid batch pricing execution input",
        "vanilla option grid batch pricing run result",
        |execution_input, progress_sender| {
            let mut observer = |event: PricingProgressEvent| {
                let _ = progress_sender.send(event);
            };

            run_vanilla_option_grid_batch_pricing_core_with_observer(
                execution_input,
                Some(&mut observer),
            )
        },
        || janiform::pricing::VanillaOptionGridBatchPricingRunError::NodePricingFailure {
            message: "pricing progress worker terminated unexpectedly".to_string(),
        },
    )
}

#[pyfunction]
fn run_vanilla_option_surface_pricing(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let execution_input = surface_execution_input_from_py(execution_input)?;

    let run_result = py
        .detach(|| run_vanilla_option_surface_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    pricing_result_to_py_object(py, &run_result, "vanilla option surface pricing run result")
}

#[pyfunction]
#[pyo3(signature = (execution_input, progress_mode))]
fn run_vanilla_option_surface_pricing_with_terminal_progress(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    progress_mode: &str,
) -> PyResult<Py<PyAny>> {
    let execution_input = surface_execution_input_from_py(execution_input)?;
    let progress_mode = parse_pricing_progress_mode(progress_mode)?;

    let run_result = py
        .detach(move || run_surface_pricing_with_terminal_progress(execution_input, progress_mode))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    pricing_result_to_py_object(py, &run_result, "vanilla option surface pricing run result")
}

#[pyfunction]
#[pyo3(signature = (execution_input, event_callback))]
fn run_vanilla_option_surface_pricing_with_events(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    event_callback: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let execution_input = surface_execution_input_from_py(execution_input)?;
    let execution_input = serde_json::to_value(execution_input).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize vanilla option surface pricing execution input: {source}"
        ))
    })?;

    let execution_input = json_value_to_py_object(py, &execution_input)?;

    run_pricing_workflow_with_events(
        py,
        execution_input.bind(py),
        event_callback,
        "vanilla option surface pricing execution input",
        "vanilla option surface pricing run result",
        |execution_input, progress_sender| {
            let mut observer = |event: PricingProgressEvent| {
                let _ = progress_sender.send(event);
            };

            run_vanilla_option_surface_pricing_core_with_observer(
                execution_input,
                Some(&mut observer),
            )
        },
        || janiform::pricing::VanillaOptionSurfacePricingRunError::NodePricingFailure {
            message: "pricing progress worker terminated unexpectedly".to_string(),
        },
    )
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        run_fitted_model_single_option_pricing_workflow,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        resolve_high_level_pricing_maturity_inputs,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        derive_activity_compressed_pricing_interval_schedule,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(run_vanilla_option_pricing, module)?)?;
    module.add_function(wrap_pyfunction!(
        run_vanilla_option_grid_batch_pricing,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_vanilla_option_grid_batch_pricing_with_terminal_progress,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_vanilla_option_grid_batch_pricing_with_events,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_vanilla_option_surface_pricing,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_vanilla_option_surface_pricing_with_terminal_progress,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_vanilla_option_surface_pricing_with_events,
        module
    )?)?;

    Ok(())
}
