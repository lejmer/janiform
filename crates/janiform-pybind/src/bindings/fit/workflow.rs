use std::path::PathBuf;

use janiform::calibration::staged::CalibrationStagedExecutionInput;
use janiform_workflows::error::WorkflowError;
use janiform_workflows::fit::{
    FitWorkflowObserver, FitWorkflowOutput, FitWorkflowRequest,
    run_fit_workflow as run_fit_workflow_core, run_staged_fit_execution_workflow,
    write_model_payload_artifact,
};
use janiform_workflows::terminal::{stderr_progress_adapter, terminal_summary_policy};
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyDict, PyDictMethods, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use crate::CalibrationExecutionError;
use crate::python_value::py_any_to_json_value;
use crate::types::PyPreparedCalibrationExecutionHandle;

use super::super::events::{CallbackErrorState, callback_error_state};
use super::events::{fit_machine_event_sink, raise_fit_progress_callback_error};
use super::output::{
    fit_workflow_output_to_py_object, prepared_fit_workflow_to_py_object,
    workflow_error_to_py_error,
};
use super::request::{
    FitFullRunRequestPayload, FitPrepareRequestPayload, FitPreparedRunRequestPayload,
    ModelArtifactSaveRequestPayload,
};

pub(crate) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(prepare_fit_workflow, module)?)?;
    module.add_function(wrap_pyfunction!(run_prepared_fit_workflow, module)?)?;
    module.add_function(wrap_pyfunction!(run_fit_workflow, module)?)?;
    module.add_function(wrap_pyfunction!(save_model_artifact, module)?)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (request))]
fn prepare_fit_workflow(py: Python<'_>, request: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let request: FitPrepareRequestPayload = serde_json::from_value(py_any_to_json_value(request)?)
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize fit preparation request: {source}"
            ))
        })?;

    let prepared_workflow = py
        .detach(move || {
            janiform_workflows::fit::prepare_fit_workflow(
                request.preparation_request,
                PathBuf::from(request.working_directory).as_path(),
            )
        })
        .map_err(workflow_error_to_py_error)?;

    prepared_fit_workflow_to_py_object(py, prepared_workflow)
}

#[pyfunction]
#[pyo3(signature = (request, event_callback = None))]
fn run_fit_workflow(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let request: FitFullRunRequestPayload = serde_json::from_value(py_any_to_json_value(request)?)
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize fit workflow request: {source}"
            ))
        })?;

    let callback_error_state = callback_error_state();
    let callback_error_state_for_sink = callback_error_state.clone();
    let event_callback = event_callback.map(|callback| callback.clone().unbind());

    let workflow_output = py
        .detach(move || {
            run_full_workflow_from_request(request, event_callback, callback_error_state_for_sink)
        })
        .map_err(workflow_error_to_py_error)?;

    raise_fit_progress_callback_error(&callback_error_state)?;

    fit_workflow_output_to_py_object(py, workflow_output)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, request, event_callback = None))]
fn run_prepared_fit_workflow(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let mut request: FitPreparedRunRequestPayload =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize fit workflow run request: {source}"
            ))
        })?;

    // Intentional clone because the Python handle remains reusable after
    // transferring execution input into the detached Rust workflow.
    let mut execution_input = prepared_handle.execution_input.clone();
    if let Some(resume_settings) = request.take_resume_settings() {
        execution_input.request.resume_settings = Some(resume_settings);
    }

    let callback_error_state = callback_error_state();
    let callback_error_state_for_sink = callback_error_state.clone();
    let event_callback = event_callback.map(|callback| callback.clone().unbind());

    let workflow_output = py
        .detach(move || {
            run_workflow_from_request(
                execution_input,
                request,
                event_callback,
                callback_error_state_for_sink,
            )
        })
        .map_err(workflow_error_to_py_error)?;

    raise_fit_progress_callback_error(&callback_error_state)?;

    fit_workflow_output_to_py_object(py, workflow_output)
}

#[pyfunction]
#[pyo3(signature = (request))]
fn save_model_artifact(py: Python<'_>, request: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let request: ModelArtifactSaveRequestPayload =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize model artifact save request: {source}"
            ))
        })?;

    let model_path = py
        .detach(move || {
            write_model_payload_artifact(
                &request.model_payload,
                request.path.as_deref().map(std::path::Path::new),
            )
        })
        .map_err(workflow_error_to_py_error)?;

    let result = PyDict::new(py);
    result.set_item("path", model_path.display().to_string())?;

    Ok(result.into_any().unbind())
}

fn run_workflow_from_request(
    execution_input: CalibrationStagedExecutionInput,
    request: FitPreparedRunRequestPayload,
    event_callback: Option<Py<PyAny>>,
    callback_error_state: CallbackErrorState,
) -> Result<FitWorkflowOutput, WorkflowError> {
    let progress_mode = request.progress_mode();
    let emit_machine_events = request.emit_machine_events();
    let terminal_output = terminal_summary_policy(progress_mode);

    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    let mut machine_event_sink =
        fit_machine_event_sink(emit_machine_events, event_callback, callback_error_state);

    run_staged_fit_execution_workflow(
        request.into_workflow_request(execution_input),
        FitWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output,
            machine_event_sink: machine_event_sink.as_mut().map(|sink| sink as _),
        },
    )
}

fn run_full_workflow_from_request(
    request: FitFullRunRequestPayload,
    event_callback: Option<Py<PyAny>>,
    callback_error_state: CallbackErrorState,
) -> Result<FitWorkflowOutput, WorkflowError> {
    let terminal_output = terminal_summary_policy(request.progress_mode);

    let mut progress_adapter = stderr_progress_adapter(request.progress_mode);

    let mut machine_event_sink = fit_machine_event_sink(
        request.emit_machine_events,
        event_callback,
        callback_error_state,
    );

    run_fit_workflow_core(
        FitWorkflowRequest {
            preparation_request: request.preparation_request,
            output_base_directory: request.output_dir.map(PathBuf::from),
            resume_settings: request.resume_settings,
        },
        FitWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output,
            machine_event_sink: machine_event_sink.as_mut().map(|sink| sink as _),
        },
    )
}
