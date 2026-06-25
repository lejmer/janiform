use janiform_foundations::paths::format_path;
use janiform_workflows::error::WorkflowError;
use janiform_workflows::simulation::{DirectSimulationCompleted, SimulationWorkflowOutput};
use pyo3::prelude::{Py, PyErr, PyResult, Python};
use pyo3::types::{PyAny, PyDictMethods};

use crate::SeasonalityInputValidationError;
use crate::SimulationExecutionError;
use crate::python_value::json_value_to_py_object;
use crate::types::PySeededTickSimulationNativeResult;

pub(super) fn simulation_from_model_artifact_output_to_py_object(
    py: Python<'_>,
    output: SimulationWorkflowOutput,
) -> PyResult<Py<PyAny>> {
    let result = pyo3::types::PyDict::new(py);

    result.set_item(
        "workflow",
        json_value_to_py_object(
            py,
            &output
                .response_payload()
                .map_err(workflow_error_to_simulation_error)?,
        )?,
    )?;

    result.set_item(
        "execution_duration_seconds",
        output.execution_duration.as_secs_f64(),
    )?;

    match output.completed {
        DirectSimulationCompleted::SeededTick {
            result: native_result,
        } => {
            result.set_item("simulation_kind", "seeded_tick")?;
            result.set_item(
                "native_result",
                Py::new(
                    py,
                    PySeededTickSimulationNativeResult {
                        output: *native_result,
                    },
                )?,
            )?;
        }
        DirectSimulationCompleted::CoarseInterval {
            result: coarse_path,
        } => {
            let raw_result_payload = serde_json::to_value(&coarse_path).map_err(|source| {
                SimulationExecutionError::new_err(format!(
                    "failed to serialize coarse simulation path: {source}"
                ))
            })?;

            result.set_item("simulation_kind", "coarse_interval")?;
            result.set_item(
                "raw_result_payload",
                json_value_to_py_object(py, &raw_result_payload)?,
            )?;
        }
    }

    Ok(result.into_any().unbind())
}

pub(super) fn workflow_error_to_simulation_error(error: WorkflowError) -> PyErr {
    match error {
        WorkflowError::SimulationValidationFailed { field, message } => {
            SimulationExecutionError::new_err(format!("{field}: {message}"))
        }
        WorkflowError::SimulationArtifactWrite { path, source } => {
            SimulationExecutionError::new_err(format!(
                "failed to write simulation artifact `{}`: {source}",
                format_path(path.as_path())
            ))
        }
        _ => SimulationExecutionError::new_err(error.to_string()),
    }
}

pub(super) fn map_seeded_tick_simulation_run_error(
    error: janiform::simulation::SeededTickSimulationRunError,
) -> PyErr {
    SeasonalityInputValidationError::new_err(error.to_string())
}
