use std::io::Write;
use std::path::PathBuf;

use janiform::calibration::{CalibrationResumeSettings, CalibrationStagedExecutionInput};
use janiform_event_log::RunEvent;
use janiform_terminal_ui::ProgressAdapter;
use janiform_workflows::error::WorkflowError;
use janiform_workflows::fit::{
    FitExecutionObserver, FitExecutionOutput, FitExecutionRequest, FitWorkflowPaths,
    run_fit_execution,
};
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyDict, PyDictMethods, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};
use serde::Deserialize;
use serde_json::Value;

use crate::CalibrationExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::PyPreparedCalibrationExecutionHandle;

#[derive(Debug, Deserialize)]
struct RunRequestPayload {
    paths: PathsPayload,
    input_paths: Vec<String>,
    preparation_payload: Option<Value>,
    staged_execution_input_path: Option<String>,
    session_definition_json: Option<String>,
    resume_settings: Option<CalibrationResumeSettings>,
    progress_mode: ProgressMode,
    emit_machine_events: bool,
}

#[derive(Debug, Deserialize)]
struct PathsPayload {
    output_directory: String,
    model_path: String,
    report_path: String,
    checkpoint_directory: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ProgressMode {
    Human,
    None,
}

pub(crate) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_prepared_fit_workflow, module)?)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, request))]
fn run_prepared_fit_workflow(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let mut request: RunRequestPayload = serde_json::from_value(py_any_to_json_value(request)?)
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize fit workflow run request: {source}"
            ))
        })?;

    // Intentional clone because the Python handle remains reusable after
    // transferring execution input into the detached Rust workflow.
    let mut execution_input = prepared_handle.execution_input.clone();
    if let Some(resume_settings) = request.resume_settings.take() {
        execution_input.request.resume_settings = Some(resume_settings);
    }

    let workflow_output = py
        .detach(move || run_workflow_from_request(execution_input, request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    workflow_output_to_py_object(py, workflow_output)
}

fn run_workflow_from_request(
    execution_input: CalibrationStagedExecutionInput,
    request: RunRequestPayload,
) -> Result<FitExecutionOutput, WorkflowError> {
    let paths = FitWorkflowPaths {
        output_directory: PathBuf::from(request.paths.output_directory),
        model_path: PathBuf::from(request.paths.model_path),
        report_path: PathBuf::from(request.paths.report_path),
        checkpoint_directory: PathBuf::from(request.paths.checkpoint_directory),
    };

    let staged_execution_input_path = request.staged_execution_input_path.map(PathBuf::from);

    let mut progress_adapter = match request.progress_mode {
        ProgressMode::Human => Some(ProgressAdapter::stderr()),
        ProgressMode::None => None,
    };

    let mut machine_event_sink = write_machine_event;

    run_fit_execution(
        FitExecutionRequest {
            execution_input,
            paths,
            input_paths: request.input_paths,
            preparation_payload: request.preparation_payload,
            staged_execution_input_path,
            session_definition_json: request.session_definition_json,
        },
        FitExecutionObserver {
            progress_adapter: progress_adapter.as_mut(),
            machine_event_sink: request
                .emit_machine_events
                .then_some(&mut machine_event_sink),
        },
    )
}

fn write_machine_event(event: &RunEvent) -> Result<(), WorkflowError> {
    let mut stderr = std::io::stderr().lock();

    serde_json::to_writer(&mut stderr, event)
        .map_err(|source| WorkflowError::OutputSerialize { source })?;

    writeln!(stderr).map_err(|source| WorkflowError::OutputWrite { source })
}

fn workflow_output_to_py_object(
    py: Python<'_>,
    workflow_output: FitExecutionOutput,
) -> PyResult<Py<PyAny>> {
    let raw_report_payload = serde_json::to_value(&workflow_output.report).map_err(|source| {
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
