use janiform::pricing::{
    HighLevelPricingMaturityResolution, HighLevelPricingMaturityResolutionRequest,
    PricingProgressEvent, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionPricingExecutionInput, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionSurfacePricingExecutionInput,
    resolve_high_level_pricing_maturity_inputs as resolve_high_level_pricing_maturity_inputs_core,
    run_vanilla_option_grid_batch_pricing as run_vanilla_option_grid_batch_pricing_core,
    run_vanilla_option_grid_batch_pricing_with_observer as run_vanilla_option_grid_batch_pricing_core_with_observer,
    run_vanilla_option_pricing as run_vanilla_option_pricing_core,
    run_vanilla_option_surface_pricing as run_vanilla_option_surface_pricing_core,
    run_vanilla_option_surface_pricing_with_observer as run_vanilla_option_surface_pricing_core_with_observer,
};
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::PyAny;
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt::Display;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::PricingExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::PyHourlyFactorContractResult;

const PRICING_PROGRESS_CALLBACK_MIN_INTERVAL: Duration = Duration::from_millis(100);

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

    let serialized_resolution = serde_json::to_value(&resolution).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize high-level pricing maturity resolution output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_resolution)
}

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
        .map(
            |((seasonality_factor, known_component_nu_h), surprise_component_nu_h)| {
                VanillaOptionPricingIntervalFactors::try_new(
                    *seasonality_factor,
                    *known_component_nu_h,
                    *surprise_component_nu_h,
                )
            },
        )
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;
    let execution_input = VanillaOptionPricingExecutionInput::try_new(
        request,
        interval_factors,
        optional_output_selection,
    )
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

#[pyfunction]
fn run_vanilla_option_grid_batch_pricing(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let execution_input: VanillaOptionGridBatchPricingExecutionInput = serde_json::from_value(
        py_any_to_json_value(execution_input)?,
    )
    .map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to deserialize vanilla option grid batch pricing execution input: {source}"
        ))
    })?;

    let run_result = py
        .allow_threads(|| run_vanilla_option_grid_batch_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    let serialized_run_result = serde_json::to_value(&run_result).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize vanilla option grid batch pricing run result: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_run_result)
}

#[pyfunction]
#[pyo3(signature = (execution_input, event_callback))]
fn run_vanilla_option_grid_batch_pricing_with_events(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    event_callback: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    run_pricing_workflow_with_events(
        py,
        execution_input,
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
    let execution_input: VanillaOptionSurfacePricingExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize vanilla option surface pricing execution input: {source}"
            ))
        })?;

    let run_result = py
        .allow_threads(|| run_vanilla_option_surface_pricing_core(execution_input))
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    let serialized_run_result = serde_json::to_value(&run_result).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize vanilla option surface pricing run result: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_run_result)
}

#[pyfunction]
#[pyo3(signature = (execution_input, event_callback))]
fn run_vanilla_option_surface_pricing_with_events(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    event_callback: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    run_pricing_workflow_with_events(
        py,
        execution_input,
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

fn run_pricing_workflow_with_events<TExecutionInput, TRunResult, TRunError, TRunFn, TPanicError>(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    event_callback: &Bound<'_, PyAny>,
    execution_input_description: &str,
    run_result_description: &str,
    run_with_progress_sender: TRunFn,
    worker_panic_error: TPanicError,
) -> PyResult<Py<PyAny>>
where
    TExecutionInput: DeserializeOwned + Send + 'static,
    TRunResult: Serialize + Send + 'static,
    TRunError: Display + Send + 'static,
    TRunFn: FnOnce(TExecutionInput, mpsc::Sender<PricingProgressEvent>) -> Result<TRunResult, TRunError>
        + Send
        + 'static,
    TPanicError: FnOnce() -> TRunError + Send + 'static,
{
    let execution_input: TExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            PricingExecutionError::new_err(format!(
                "failed to deserialize {execution_input_description}: {source}"
            ))
        })?;
    let event_callback = event_callback.clone().unbind();

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_observer = Arc::clone(&callback_error_message);

    let run_result = py
        .allow_threads(move || {
            let (progress_sender, progress_receiver) = mpsc::channel::<PricingProgressEvent>();

            std::thread::scope(|scope| {
                let worker =
                    scope.spawn(move || run_with_progress_sender(execution_input, progress_sender));

                let mut last_emitted_event = None::<PricingProgressEvent>;
                let mut last_emitted_at = None::<Instant>;

                for event in progress_receiver {
                    if callback_error_message_for_observer
                        .lock()
                        .ok()
                        .and_then(|guard| guard.clone())
                        .is_some()
                    {
                        continue;
                    }

                    if !should_emit_pricing_progress_event(
                        &event,
                        last_emitted_event.as_ref(),
                        last_emitted_at,
                    ) {
                        continue;
                    }

                    last_emitted_event = Some(event.clone());
                    last_emitted_at = Some(Instant::now());

                    Python::with_gil(|py| {
                        let callback_result = python_pricing_progress_event_to_py_object(
                            py, &event,
                        )
                        .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));

                        if let Err(source) = callback_result {
                            if let Ok(mut guard) = callback_error_message_for_observer.lock() {
                                if guard.is_none() {
                                    *guard = Some(source.to_string());
                                }
                            }
                        }
                    });
                }

                worker.join().unwrap_or_else(|_| Err(worker_panic_error()))
            })
        })
        .map_err(|source| PricingExecutionError::new_err(source.to_string()))?;

    if let Ok(guard) = callback_error_message.lock() {
        if let Some(error_message) = guard.as_ref() {
            return Err(PricingExecutionError::new_err(format!(
                "pricing progress callback failed: {error_message}"
            )));
        }
    }

    let serialized_run_result = serde_json::to_value(&run_result).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize {run_result_description}: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_run_result)
}

fn python_pricing_progress_event_to_py_object(
    py: Python<'_>,
    event: &PricingProgressEvent,
) -> PyResult<Py<PyAny>> {
    let serialized_event = match serde_json::to_value(event).map_err(|source| {
        PricingExecutionError::new_err(format!(
            "failed to serialize pricing progress event: {source}"
        ))
    })? {
        Value::Object(object) => Value::Object(object),
        _ => {
            return Err(PricingExecutionError::new_err(
                "failed to serialize pricing progress event: expected a JSON object",
            ));
        }
    };

    json_value_to_py_object(py, &serialized_event)
}

fn should_emit_pricing_progress_event(
    event: &PricingProgressEvent,
    last_emitted_event: Option<&PricingProgressEvent>,
    last_emitted_at: Option<Instant>,
) -> bool {
    let Some(previous_event) = last_emitted_event else {
        return true;
    };

    if previous_event.phase != event.phase {
        return true;
    }

    if event.completed_units >= event.total_units {
        return true;
    }

    if previous_event.completed_units != event.completed_units {
        return last_emitted_at
            .is_none_or(|instant| instant.elapsed() >= PRICING_PROGRESS_CALLBACK_MIN_INTERVAL);
    }

    false
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        resolve_high_level_pricing_maturity_inputs,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(run_vanilla_option_pricing, module)?)?;
    module.add_function(wrap_pyfunction!(
        run_vanilla_option_grid_batch_pricing,
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
        run_vanilla_option_surface_pricing_with_events,
        module
    )?)?;

    Ok(())
}
