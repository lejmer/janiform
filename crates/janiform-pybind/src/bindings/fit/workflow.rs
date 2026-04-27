use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use janiform::calibration::{CalibrationResumeSettings, CalibrationStagedExecutionInput};
use janiform_event_log::RunEvent;
use janiform_terminal_ui::{ProgressAdapter, TerminalRenderer};
use janiform_workflows::fit::{
    FitExecutionObserver, FitExecutionOutput, FitExecutionRequest, FitPreparationRequest,
    FitTerminalOutput, FitWorkflowPaths, FitWorkflowRequest, build_staged_response_payload,
    run_fit_execution, run_fit_workflow as run_fit_workflow_core,
};
use janiform_workflows::{error::WorkflowError, write_machine_event_to_stderr};
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyDict, PyDictMethods, PyModule, PyModuleMethods};
use pyo3::{PyErr, pyfunction, wrap_pyfunction};
use serde::Deserialize;
use serde_json::Value;

use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::PyPreparedCalibrationExecutionHandle;
use crate::{CalibrationExecutionError, CalibrationValidationError};

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
struct FullRunRequestPayload {
    preparation_request: FitPreparationRequest,
    output_dir: Option<String>,
    resume_settings: Option<CalibrationResumeSettings>,
    progress_mode: ProgressMode,
    emit_machine_events: bool,
}

#[derive(Debug, Deserialize)]
struct PrepareRequestPayload {
    preparation_request: FitPreparationRequest,
    working_directory: String,
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
    module.add_function(wrap_pyfunction!(prepare_fit_workflow, module)?)?;
    module.add_function(wrap_pyfunction!(run_prepared_fit_workflow, module)?)?;
    module.add_function(wrap_pyfunction!(run_fit_workflow, module)?)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (request))]
fn prepare_fit_workflow(py: Python<'_>, request: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let request: PrepareRequestPayload = serde_json::from_value(py_any_to_json_value(request)?)
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

    prepared_workflow_to_py_object(py, prepared_workflow)
}

#[pyfunction]
#[pyo3(signature = (request, event_callback = None))]
fn run_fit_workflow(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let request: FullRunRequestPayload = serde_json::from_value(py_any_to_json_value(request)?)
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize fit workflow request: {source}"
            ))
        })?;

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_sink = Arc::clone(&callback_error_message);
    let event_callback = event_callback.map(|callback| callback.clone().unbind());

    let workflow_output = py
        .detach(move || {
            run_full_workflow_from_request(request, event_callback, callback_error_message_for_sink)
        })
        .map_err(workflow_error_to_py_error)?;

    raise_fit_progress_callback_error(&callback_error_message)?;

    workflow_output_to_py_object(py, workflow_output)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, request, event_callback = None))]
fn run_prepared_fit_workflow(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
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

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_sink = Arc::clone(&callback_error_message);
    let event_callback = event_callback.map(|callback| callback.clone().unbind());

    let workflow_output = py
        .detach(move || {
            run_workflow_from_request(
                execution_input,
                request,
                event_callback,
                callback_error_message_for_sink,
            )
        })
        .map_err(workflow_error_to_py_error)?;

    raise_fit_progress_callback_error(&callback_error_message)?;

    workflow_output_to_py_object(py, workflow_output)
}

fn run_workflow_from_request(
    execution_input: CalibrationStagedExecutionInput,
    request: RunRequestPayload,
    event_callback: Option<Py<PyAny>>,
    callback_error_message: Arc<Mutex<Option<String>>>,
) -> Result<FitExecutionOutput, WorkflowError> {
    let paths = FitWorkflowPaths {
        output_directory: PathBuf::from(request.paths.output_directory),
        model_path: PathBuf::from(request.paths.model_path),
        report_path: PathBuf::from(request.paths.report_path),
        checkpoint_directory: PathBuf::from(request.paths.checkpoint_directory),
    };

    let staged_execution_input_path = request.staged_execution_input_path.map(PathBuf::from);
    let terminal_output = fit_terminal_output(request.progress_mode);

    let mut progress_adapter = match request.progress_mode {
        ProgressMode::Human => Some(ProgressAdapter::stderr()),
        ProgressMode::None => None,
    };

    let mut machine_event_sink = fit_machine_event_sink(
        request.emit_machine_events,
        event_callback,
        callback_error_message,
    );

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
            terminal_output,
            machine_event_sink: machine_event_sink.as_mut().map(|sink| sink as _),
        },
    )
}

fn run_full_workflow_from_request(
    request: FullRunRequestPayload,
    event_callback: Option<Py<PyAny>>,
    callback_error_message: Arc<Mutex<Option<String>>>,
) -> Result<FitExecutionOutput, WorkflowError> {
    let terminal_output = fit_terminal_output(request.progress_mode);

    let mut progress_adapter = match request.progress_mode {
        ProgressMode::Human => Some(ProgressAdapter::stderr()),
        ProgressMode::None => None,
    };

    let mut machine_event_sink = fit_machine_event_sink(
        request.emit_machine_events,
        event_callback,
        callback_error_message,
    );

    run_fit_workflow_core(
        FitWorkflowRequest {
            preparation_request: request.preparation_request,
            output_base_directory: request.output_dir.map(PathBuf::from),
            resume_settings: request.resume_settings,
        },
        FitExecutionObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output,
            machine_event_sink: machine_event_sink.as_mut().map(|sink| sink as _),
        },
    )
}

fn fit_machine_event_sink(
    emit_machine_events: bool,
    event_callback: Option<Py<PyAny>>,
    callback_error_message: Arc<Mutex<Option<String>>>,
) -> Option<impl FnMut(&RunEvent) -> Result<(), WorkflowError>> {
    if !emit_machine_events && event_callback.is_none() {
        return None;
    }

    Some(move |event: &RunEvent| {
        if emit_machine_events {
            write_machine_event_to_stderr(event)?;
        }

        if let Some(event_callback) = &event_callback {
            emit_fit_progress_callback_event(event_callback, event, &callback_error_message);
        }

        Ok(())
    })
}

fn emit_fit_progress_callback_event(
    event_callback: &Py<PyAny>,
    event: &RunEvent,
    callback_error_message: &Arc<Mutex<Option<String>>>,
) {
    if fit_progress_callback_failed(callback_error_message) {
        return;
    }

    Python::attach(|py| {
        let callback_result = python_fit_progress_event_to_py_object(py, event)
            .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));

        if let Err(source) = callback_result {
            record_fit_progress_callback_error(callback_error_message, source.to_string());
        }
    });
}

fn fit_progress_callback_failed(callback_error_message: &Arc<Mutex<Option<String>>>) -> bool {
    callback_error_message
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

fn record_fit_progress_callback_error(
    callback_error_message: &Arc<Mutex<Option<String>>>,
    error_message: String,
) {
    let Ok(mut guard) = callback_error_message.lock() else {
        return;
    };

    if guard.is_some() {
        return;
    }

    *guard = Some(error_message);
}

fn raise_fit_progress_callback_error(
    callback_error_message: &Arc<Mutex<Option<String>>>,
) -> PyResult<()> {
    let Ok(guard) = callback_error_message.lock() else {
        return Ok(());
    };

    let Some(error_message) = guard.as_ref() else {
        return Ok(());
    };

    Err(CalibrationExecutionError::new_err(format!(
        "fit progress callback failed: {error_message}"
    )))
}

fn python_fit_progress_event_to_py_object(py: Python<'_>, event: &RunEvent) -> PyResult<Py<PyAny>> {
    let serialized_event = match serde_json::to_value(event).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize fit progress event: {source}"
        ))
    })? {
        Value::Object(object) => Value::Object(object),
        _ => {
            return Err(CalibrationExecutionError::new_err(
                "failed to serialize fit progress event: expected a JSON object",
            ));
        }
    };

    json_value_to_py_object(py, &serialized_event)
}

fn workflow_output_to_py_object(
    py: Python<'_>,
    workflow_output: FitExecutionOutput,
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

fn prepared_workflow_to_py_object(
    py: Python<'_>,
    prepared_workflow: janiform_workflows::fit::PreparedFitWorkflow,
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

fn fit_terminal_output(progress_mode: ProgressMode) -> Option<FitTerminalOutput> {
    match progress_mode {
        ProgressMode::Human => Some(FitTerminalOutput::start_and_completion(
            TerminalRenderer::new(TerminalRenderer::stdout_color_enabled(), None),
        )),
        ProgressMode::None => None,
    }
}

fn workflow_error_to_py_error(error: WorkflowError) -> PyErr {
    match error {
        WorkflowError::FitValidationFailed { field, message } => {
            CalibrationValidationError::new_err(format!("{field}: {message}"))
        }
        error => CalibrationExecutionError::new_err(error.to_string()),
    }
}
