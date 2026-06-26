use std::sync::mpsc;
use std::time::Duration;

use janiform_foundations::paths::format_path;
use janiform_workflows::forecast::{
    ForecastExpectedPathRow, ForecastOutput, ForecastProgressEvent, ForecastTableFormat,
    ForecastTerminalDistributionRow, ForecastWorkflowObserver, ForecastWorkflowRequest,
    load_forecast_bundle_manifest, run_forecast, run_forecast_with_terminal_progress,
    save_forecast_bundle,
};
use janiform_workflows::terminal::TerminalProgressMode;
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};
use serde::Deserialize;

use crate::ForecastExecutionError;
use crate::bindings::events::{
    callback_error_state, callback_failed, emit_serializable_callback_event, raise_callback_error,
};
use crate::native_interrupt::{NativeInterruptMode, run_interruptible_native};
use crate::panic_boundary::panic_to_pyresult;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};

#[pyfunction]
#[pyo3(signature = (request, event_callback = None, progress_mode = "none"))]
fn run_forecast_from_model_artifact(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
    progress_mode: &str,
) -> PyResult<Py<PyAny>> {
    let request = forecast_request_from_py(request)?;
    let progress_mode = parse_forecast_progress_mode(progress_mode)?;

    let output = if event_callback.is_some() {
        run_forecast_with_progress_events(py, request, event_callback)?
    } else {
        panic_to_pyresult(
            "running forecast from model artifact",
            ForecastExecutionError::new_err,
            || {
                run_interruptible_native(
                    py,
                    NativeInterruptMode::ImportedApi,
                    move |cancellation_token| {
                        run_forecast_with_terminal_progress(
                            request,
                            cancellation_token,
                            progress_mode,
                        )
                        .map_err(workflow_error_to_forecast_error)
                    },
                )
            },
        )?
    };

    forecast_output_to_py_object(py, output)
}

#[pyfunction]
#[pyo3(signature = (forecast_payload, path, table_format = "parquet"))]
fn save_forecast_bundle_from_payload(
    py: Python<'_>,
    forecast_payload: &Bound<'_, PyAny>,
    path: &str,
    table_format: &str,
) -> PyResult<Py<PyAny>> {
    let payload: ForecastSavePayload =
        serde_json::from_value(py_any_to_json_value(forecast_payload)?).map_err(|source| {
            ForecastExecutionError::new_err(format!(
                "forecast save payload failed to deserialize: {source}"
            ))
        })?;

    let table_format = parse_table_format(table_format)?;

    let output = ForecastOutput {
        manifest: payload.manifest,
        expected_path: payload.expected_path,
        terminal_distribution: payload.terminal_distribution,
        bundle_path: None,
        execution_duration: Duration::ZERO,
    };

    let resolved_path = save_forecast_bundle(&output, std::path::Path::new(path), table_format)
        .map_err(workflow_error_to_forecast_error)?;

    json_value_to_py_object(
        py,
        &serde_json::json!({
            "path": format_path(&resolved_path),
        }),
    )
}

#[pyfunction]
#[pyo3(signature = (path))]
fn load_forecast_bundle_manifest_py(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    let manifest = load_forecast_bundle_manifest(std::path::Path::new(path))
        .map_err(workflow_error_to_forecast_error)?;

    let payload = serde_json::to_value(manifest).map_err(|source| {
        ForecastExecutionError::new_err(format!("failed to serialize forecast manifest: {source}"))
    })?;

    json_value_to_py_object(py, &payload)
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_forecast_from_model_artifact, module)?)?;
    module.add_function(wrap_pyfunction!(save_forecast_bundle_from_payload, module)?)?;
    module.add_function(wrap_pyfunction!(load_forecast_bundle_manifest_py, module)?)?;

    Ok(())
}

#[derive(Deserialize)]
struct ForecastSavePayload {
    manifest: janiform_workflows::forecast::ForecastBundleManifest,
    expected_path: Vec<ForecastExpectedPathRow>,
    terminal_distribution: Vec<ForecastTerminalDistributionRow>,
}

fn forecast_request_from_py(raw_request: &Bound<'_, PyAny>) -> PyResult<ForecastWorkflowRequest> {
    serde_json::from_value(py_any_to_json_value(raw_request)?).map_err(|source| {
        ForecastExecutionError::new_err(format!(
            "failed to deserialize forecast workflow request: {source}"
        ))
    })
}

fn parse_forecast_progress_mode(value: &str) -> PyResult<TerminalProgressMode> {
    TerminalProgressMode::from_boundary_value(value).map_err(ForecastExecutionError::new_err)
}

fn parse_table_format(value: &str) -> PyResult<ForecastTableFormat> {
    match value {
        "parquet" => Ok(ForecastTableFormat::Parquet),
        "csv" => Ok(ForecastTableFormat::Csv),
        "json" => Ok(ForecastTableFormat::Json),
        _ => Err(ForecastExecutionError::new_err(
            "table_format must be one of 'parquet', 'csv', or 'json'",
        )),
    }
}

fn run_forecast_with_progress_events(
    py: Python<'_>,
    request: ForecastWorkflowRequest,
    event_callback: Option<&Bound<'_, PyAny>>,
) -> PyResult<ForecastOutput> {
    let callback_error_state = callback_error_state();
    let callback_error_state_for_observer = callback_error_state.clone();
    let (event_sender, event_receiver) = mpsc::channel::<ForecastProgressEvent>();
    let event_callback = event_callback.map(|callback| callback.clone().unbind());

    let run_output = panic_to_pyresult(
        "running forecast from model artifact",
        ForecastExecutionError::new_err,
        || {
            crate::native_interrupt::run_interruptible_native_with_poll(
                py,
                NativeInterruptMode::ImportedApi,
                move |cancellation_token| {
                    let mut observer = move |event: ForecastProgressEvent| {
                        if callback_failed(&callback_error_state_for_observer) {
                            return;
                        }

                        let _ = event_sender.send(event);
                    };

                    run_forecast(
                        request,
                        ForecastWorkflowObserver {
                            cancellation_token,
                            progress_adapter: None,
                            terminal_output: None,
                            progress_event_observer: Some(&mut observer),
                        },
                    )
                    .map_err(workflow_error_to_forecast_error)
                },
                |_: Python<'_>| {
                    if let Some(event_callback) = event_callback.as_ref() {
                        for event in event_receiver.try_iter() {
                            emit_serializable_callback_event(
                                event_callback,
                                &callback_error_state,
                                &event,
                                "forecast progress event",
                            );
                        }
                    }

                    Ok(())
                },
            )
        },
    )?;

    raise_callback_error(
        &callback_error_state,
        "forecast progress callback failed",
        ForecastExecutionError::new_err,
    )?;

    Ok(run_output)
}

fn forecast_output_to_py_object(py: Python<'_>, output: ForecastOutput) -> PyResult<Py<PyAny>> {
    let payload = serde_json::json!({
        "workflow": output.response_payload(),
        "manifest": output.manifest,
        "expected_path": output.expected_path,
        "terminal_distribution": output.terminal_distribution,
        "bundle_path": output.bundle_path,
        "execution_duration_seconds": output.execution_duration.as_secs_f64(),
    });

    json_value_to_py_object(py, &payload)
}

fn workflow_error_to_forecast_error(
    error: janiform_workflows::error::WorkflowError,
) -> pyo3::PyErr {
    ForecastExecutionError::new_err(error.to_string())
}
