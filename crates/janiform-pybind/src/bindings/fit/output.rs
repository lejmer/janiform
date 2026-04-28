use janiform_workflows::error::WorkflowError;
use janiform_workflows::fit::{
    FitWorkflowOutput, PreparedFitWorkflow, build_staged_response_payload,
};
use pyo3::prelude::{Py, PyErr, PyResult, Python};
use pyo3::types::{PyAny, PyDict, PyDictMethods};

use crate::python_value::json_value_to_py_object;
use crate::types::PyPreparedCalibrationExecutionHandle;
use crate::{CalibrationExecutionError, CalibrationValidationError};

pub(super) fn fit_workflow_output_to_py_object(
    py: Python<'_>,
    workflow_output: FitWorkflowOutput,
) -> PyResult<Py<PyAny>> {
    let raw_report_payload =
        build_staged_response_payload(&workflow_output.report).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to serialize staged calibration response: {source}"
            ))
        })?;

    let result = PyDict::new(py);

    result.set_item(
        "raw_staged_response_payload",
        json_value_to_py_object(py, &raw_report_payload)?,
    )?;
    result.set_item(
        "fit_report_payload",
        json_value_to_py_object(py, &workflow_output.report_payload)?,
    )?;
    result.set_item(
        "model_payload",
        match &workflow_output.model_payload {
            Some(payload) => json_value_to_py_object(py, payload)?,
            None => py.None(),
        },
    )?;
    result.set_item(
        "event_log_path",
        workflow_output.event_log_path.display().to_string(),
    )?;
    result.set_item(
        "latest_state_snapshot_path",
        workflow_output
            .latest_state_snapshot_path
            .display()
            .to_string(),
    )?;
    result.set_item(
        "execution_duration_seconds",
        workflow_output.execution_duration.as_secs_f64(),
    )?;

    Ok(result.into_any().unbind())
}

pub(super) fn prepared_fit_workflow_to_py_object(
    py: Python<'_>,
    prepared_workflow: PreparedFitWorkflow,
) -> PyResult<Py<PyAny>> {
    let result = PyDict::new(py);

    // Intentional clone because the Python handle owns an execution input while
    // the response also serializes fields from the original prepared workflow.
    let prepared_handle = Py::new(
        py,
        PyPreparedCalibrationExecutionHandle {
            execution_input: prepared_workflow.execution_input.clone(),
        },
    )?;

    result.set_item("prepared_handle", prepared_handle)?;
    result.set_item(
        "staged_request",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&prepared_workflow.execution_input.request).map_err(
                |source| {
                    CalibrationExecutionError::new_err(format!(
                        "failed to serialize staged request: {source}"
                    ))
                },
            )?,
        )?,
    )?;
    result.set_item(
        "preparation_payload",
        json_value_to_py_object(py, &prepared_workflow.preparation_payload)?,
    )?;
    result.set_item(
        "session_definition_json",
        prepared_workflow.session_definition_json,
    )?;

    Ok(result.into_any().unbind())
}

pub(super) fn workflow_error_to_py_error(error: WorkflowError) -> PyErr {
    match error {
        WorkflowError::FitValidationFailed { field, message } => {
            CalibrationValidationError::new_err(format!("{field}: {message}"))
        }
        error => CalibrationExecutionError::new_err(error.to_string()),
    }
}
