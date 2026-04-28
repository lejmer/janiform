use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use janiform::affine_oracle::{
    EndogenousAnchorIntervalFactors, ExponentialAffineTransformEquationSpecification,
    materialize_endogenous_anchor_path,
};
use janiform::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform::news::NewsProcessConfiguration;
use janiform::nig::NigSharedPlusShiftConfiguration;
use janiform::simulation::{
    CoarseSimulationModelInputs, CoarseSimulationRunRequest, MeanReversionBoundaryState,
    MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationModelInputs, SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
    SimulationProgressEvent,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
    run_coarse_simulation as run_coarse_simulation_core,
    run_coarse_simulation_with_observer as run_coarse_simulation_core_with_observer,
    run_seeded_tick_event_simulation as run_seeded_tick_event_simulation_core,
    run_seeded_tick_event_simulation_with_observer as run_seeded_tick_event_simulation_core_with_observer,
};
use janiform::sufficient_statistics::SufficientStatisticsIntervalFactors;
use janiform_workflows::error::WorkflowError;
use janiform_workflows::simulation::{
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, DirectSimulationCompleted,
    ModelArtifactSimulationWorkflowRequest, SimulationOutputColumns, export_coarse_simulation,
    export_seeded_tick_simulation,
    run_simulation_from_model_artifact as run_simulation_from_model_artifact_core,
};
use pyo3::prelude::{Bound, Py, PyErr, PyResult, Python};
use pyo3::types::{
    PyAny, PyAnyMethods, PyDictMethods, PyMapping, PyMappingMethods, PyModule, PyModuleMethods,
};
use pyo3::{pyfunction, wrap_pyfunction};
use serde_json::Value;

use super::calendar::parse_timestamp_utc;
use crate::SeasonalityInputValidationError;
use crate::SimulationExecutionError;
use crate::panic_boundary::panic_to_pyresult;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::{
    PyCalendarRegistry, PyHourlyFactorContractResult, PySeededTickSimulationNativeResult,
};

#[pyfunction]
#[pyo3(signature = (request, event_callback = None))]
fn run_simulation_from_model_artifact(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let request: ModelArtifactSimulationWorkflowRequest =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            SimulationExecutionError::new_err(format!(
                "failed to deserialize simulation-from-model-artifact workflow request: {source}"
            ))
        })?;

    let output = run_simulation_with_optional_progress_events(
        py,
        "running simulation from model artifact",
        request,
        event_callback,
        |request| {
            run_simulation_from_model_artifact_core(request, &mut |_| {})
                .map_err(workflow_error_to_simulation_error)
        },
        |request, observer| {
            run_simulation_from_model_artifact_core(request, observer)
                .map_err(workflow_error_to_simulation_error)
        },
    )?;

    simulation_from_model_artifact_output_to_py_object(py, output)
}

#[pyfunction]
#[pyo3(signature = (kernel_parameters, nig_shared_plus_shift_configuration, news_process_configuration, initial_excitation_state, mean_reversion_fitted_parameters, interval_alignment_policy, start_anchor_value, s_h, known_nu_h, surprise_nu_h, interval_deltas))]
fn materialize_endogenous_mean_reversion_runtime_interval_anchors(
    py: Python<'_>,
    kernel_parameters: &Bound<'_, PyAny>,
    nig_shared_plus_shift_configuration: &Bound<'_, PyAny>,
    news_process_configuration: &Bound<'_, PyAny>,
    initial_excitation_state: &Bound<'_, PyAny>,
    mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
    interval_alignment_policy: &str,
    start_anchor_value: f64,
    s_h: &Bound<'_, PyAny>,
    known_nu_h: &Bound<'_, PyAny>,
    surprise_nu_h: &Bound<'_, PyAny>,
    interval_deltas: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_kernel_parameters = parse_seeded_simulation_kernel_parameters(kernel_parameters)?;

    let parsed_nig_shared_plus_shift_configuration =
        parse_seeded_simulation_nig_shared_plus_shift_configuration(
            nig_shared_plus_shift_configuration,
        )?;

    let parsed_news_process_configuration = serde_json::from_value::<NewsProcessConfiguration>(
        py_any_to_json_value(news_process_configuration)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "news_process_configuration failed to deserialize: {source}"
        ))
    })?;

    let parsed_initial_excitation_state =
        parse_seeded_simulation_initial_excitation_state(initial_excitation_state)?;

    let parsed_mean_reversion_fitted_parameters =
        parse_seeded_simulation_mean_reversion_fitted_parameters(mean_reversion_fitted_parameters)?;

    let parsed_s_h = parse_required_numeric_sequence(s_h, "s_h")?;
    let parsed_known_nu_h = parse_required_numeric_sequence(known_nu_h, "known_nu_h")?;
    let parsed_surprise_nu_h = parse_required_numeric_sequence(surprise_nu_h, "surprise_nu_h")?;

    let parsed_interval_deltas =
        parse_required_numeric_sequence(interval_deltas, "interval_deltas")?;

    let interval_count = parsed_s_h.len();
    if parsed_known_nu_h.len() != interval_count
        || parsed_surprise_nu_h.len() != interval_count
        || parsed_interval_deltas.len() != interval_count
    {
        return Err(SeasonalityInputValidationError::new_err(
            "endogenous mean-reversion runtime anchor materialization requires `s_h`, `known_nu_h`, `surprise_nu_h`, and `interval_deltas` to have identical lengths",
        ));
    }

    let interval_factors: Result<Vec<EndogenousAnchorIntervalFactors>, PyErr> = parsed_s_h
        .iter()
        .zip(parsed_known_nu_h.iter())
        .zip(parsed_surprise_nu_h.iter())
        .zip(parsed_interval_deltas.iter())
        .map(
            |(((seasonality_factor, known_intensity), surprise_intensity), interval_delta)| {
                EndogenousAnchorIntervalFactors::try_new(
                    *seasonality_factor,
                    *known_intensity,
                    *surprise_intensity,
                    *interval_delta,
                )
                .map_err(|source| {
                    SeasonalityInputValidationError::new_err(format!(
                        "endogenous mean-reversion interval factors failed validation: {source}"
                    ))
                })
            },
        )
        .collect();

    let interval_factors = interval_factors?;

    let materialization = materialize_endogenous_anchor_path(
        interval_alignment_policy,
        start_anchor_value,
        ExponentialAffineTransformEquationSpecification::new(
            parsed_kernel_parameters,
            parsed_news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
        ),
        parsed_nig_shared_plus_shift_configuration,
        &parsed_news_process_configuration,
        parsed_initial_excitation_state,
        interval_factors.as_slice(),
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "endogenous mean-reversion runtime anchor materialization failed: {source}"
        ))
    })?;

    let core_expected_increments: Vec<f64> = materialization
        .interval_expectations
        .iter()
        .map(|expectation| expectation.core_expected_increment)
        .collect();

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            interval_alignment_policy,
            start_anchor_value,
            0.0,
            &parsed_mean_reversion_fitted_parameters.anchor_model,
            core_expected_increments.as_slice(),
            &parsed_interval_deltas,
        )
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "endogenous mean-reversion runtime anchor materialization failed: {source}"
            ))
        })?;

    let serialized_runtime_interval_anchors = serde_json::to_value(&runtime_interval_anchors)
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "failed to serialize endogenous mean-reversion runtime anchors: {source}"
            ))
        })?;

    json_value_to_py_object(py, &serialized_runtime_interval_anchors)
}

#[pyfunction]
#[pyo3(signature = (seed, start_time_utc_nanos, horizon_interval_count, interval_delta, model_inputs, hourly_factor_contract, event_callback = None))]
fn run_coarse_simulation(
    py: Python<'_>,
    seed: u64,
    start_time_utc_nanos: i64,
    horizon_interval_count: usize,
    interval_delta: f64,
    model_inputs: &Bound<'_, PyAny>,
    hourly_factor_contract: &PyHourlyFactorContractResult,
    event_callback: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let model_inputs: CoarseSimulationModelInputs =
        serde_json::from_value(py_any_to_json_value(model_inputs)?).map_err(|source| {
            SimulationExecutionError::new_err(format!(
                "failed to deserialize coarse simulation model inputs: {source}"
            ))
        })?;

    let interval_factors = SufficientStatisticsIntervalFactors::try_new(
        hourly_factor_contract.output.s_h.clone(),
        hourly_factor_contract.output.known_component_nu_h.clone(),
        hourly_factor_contract
            .output
            .surprise_component_nu_h
            .clone(),
        Some(interval_delta),
    )
    .map_err(|source| SimulationExecutionError::new_err(source.to_string()))?;

    let run_request = CoarseSimulationRunRequest::try_new(
        seed,
        start_time_utc_nanos,
        horizon_interval_count,
        interval_delta,
        model_inputs,
        interval_factors,
    )
    .map_err(|source| SimulationExecutionError::new_err(source.to_string()))?;

    let coarse_path = run_simulation_with_optional_progress_events(
        py,
        "running coarse simulation",
        run_request,
        event_callback,
        |run_request| {
            run_coarse_simulation_core(run_request)
                .map_err(|source| SimulationExecutionError::new_err(source.to_string()))
        },
        |run_request, observer| {
            run_coarse_simulation_core_with_observer(run_request, observer)
                .map_err(|source| SimulationExecutionError::new_err(source.to_string()))
        },
    )?;

    let serialized_coarse_path = serde_json::to_value(&coarse_path).map_err(|source| {
        SimulationExecutionError::new_err(format!(
            "failed to serialize coarse simulation path payload: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_coarse_path)
}

#[pyfunction]
#[pyo3(signature = (seed, start_time_utc, horizon_interval_count, kernel_parameters, nig_shared_plus_shift_configuration, initial_excitation_state, hourly_factor_contract, news_process_configuration, mean_reversion_fitted_parameters, mean_reversion_runtime_interval_anchors, initial_mean_reversion_boundary_state, *, debug_trace_enabled = false, sample_every_nth_transition = 1, max_records_per_chunk = 1024, state_snapshot_capture = "disabled", event_callback = None))]
fn run_seeded_tick_event_simulation(
    py: Python<'_>,
    seed: u64,
    start_time_utc: &str,
    horizon_interval_count: usize,
    kernel_parameters: &Bound<'_, PyAny>,
    nig_shared_plus_shift_configuration: &Bound<'_, PyAny>,
    initial_excitation_state: &Bound<'_, PyAny>,
    hourly_factor_contract: &PyHourlyFactorContractResult,
    news_process_configuration: &Bound<'_, PyAny>,
    mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
    mean_reversion_runtime_interval_anchors: &Bound<'_, PyAny>,
    initial_mean_reversion_boundary_state: &Bound<'_, PyAny>,
    debug_trace_enabled: bool,
    sample_every_nth_transition: u64,
    max_records_per_chunk: usize,
    state_snapshot_capture: &str,
    event_callback: Option<&Bound<'_, PyAny>>,
) -> PyResult<PySeededTickSimulationNativeResult> {
    let parsed_start_time_utc = parse_timestamp_utc(start_time_utc)?;

    let parsed_kernel_parameters = parse_seeded_simulation_kernel_parameters(kernel_parameters)?;

    let parsed_nig_shared_plus_shift_configuration =
        parse_seeded_simulation_nig_shared_plus_shift_configuration(
            nig_shared_plus_shift_configuration,
        )?;

    let parsed_initial_excitation_state =
        parse_seeded_simulation_initial_excitation_state(initial_excitation_state)?;

    let parsed_news_process_configuration = serde_json::from_value::<NewsProcessConfiguration>(
        py_any_to_json_value(news_process_configuration)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "news_process_configuration failed to deserialize: {source}"
        ))
    })?;

    let parsed_mean_reversion_fitted_parameters =
        parse_seeded_simulation_mean_reversion_fitted_parameters(mean_reversion_fitted_parameters)?;

    let parsed_mean_reversion_runtime_interval_anchors =
        parse_seeded_simulation_mean_reversion_runtime_interval_anchors(
            mean_reversion_runtime_interval_anchors,
        )?;

    let parsed_initial_mean_reversion_boundary_state =
        parse_seeded_simulation_mean_reversion_boundary_state(
            initial_mean_reversion_boundary_state,
        )?;

    let parsed_state_snapshot_capture =
        parse_seeded_simulation_state_snapshot_capture(state_snapshot_capture)?;

    let run_options = SeededTickSimulationRunOptions {
        debug_trace_enabled,
        debug_trace_options: SeededTickSimulationDebugTraceOptions {
            sample_every_nth_transition,
            max_records_per_chunk,
        },
        state_snapshot_capture: parsed_state_snapshot_capture,
    };

    let calendar_registry = hourly_factor_contract
        .calendar_registry
        .clone()
        .ok_or_else(|| {
            SimulationExecutionError::new_err(
                "seeded tick-event simulation requires a runtime hourly factor contract with calendar registry state",
            )
        })?;

    let run_config = SeededTickSimulationRunConfig::try_new(
        seed,
        parsed_start_time_utc,
        horizon_interval_count,
        SeededTickSimulationModelInputs {
            kernel_parameters: parsed_kernel_parameters,
            nig_shared_plus_shift_configuration: parsed_nig_shared_plus_shift_configuration,
            news_process_configuration: parsed_news_process_configuration,
            initial_excitation_state: parsed_initial_excitation_state,
            mean_reversion_fitted_parameters: parsed_mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors: parsed_mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state: parsed_initial_mean_reversion_boundary_state,
            hourly_factor_contract: hourly_factor_contract.output.clone(),
            calendar_registry,
        },
        Some(run_options),
    )
    .map_err(map_seeded_tick_simulation_run_error)?;

    let run_result = run_simulation_with_optional_progress_events(
        py,
        "running seeded tick-event simulation",
        run_config,
        event_callback,
        |run_config| {
            run_seeded_tick_event_simulation_core(run_config)
                .map_err(map_seeded_tick_simulation_run_error)
        },
        |run_config, observer| {
            run_seeded_tick_event_simulation_core_with_observer(run_config, observer)
                .map_err(map_seeded_tick_simulation_run_error)
        },
    )?;

    Ok(PySeededTickSimulationNativeResult { output: run_result })
}

fn simulation_from_model_artifact_output_to_py_object(
    py: Python<'_>,
    output: janiform_workflows::simulation::ModelArtifactSimulationWorkflowOutput,
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
            ..
        } => {
            result.set_item("simulation_kind", "seeded_tick")?;
            result.set_item(
                "native_result",
                Py::new(
                    py,
                    PySeededTickSimulationNativeResult {
                        output: native_result,
                    },
                )?,
            )?;
        }
        DirectSimulationCompleted::CoarseInterval {
            result: coarse_path,
            ..
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

#[pyfunction]
#[pyo3(signature = (native_result, output_path, reference_price, timestamp_column, price_column, side_column = None, batch_size = DEFAULT_SIMULATION_EXPORT_BATCH_SIZE))]
fn export_seeded_tick_simulation_artifact(
    py: Python<'_>,
    native_result: &PySeededTickSimulationNativeResult,
    output_path: &str,
    reference_price: f64,
    timestamp_column: &str,
    price_column: &str,
    side_column: Option<&str>,
    batch_size: usize,
) -> PyResult<usize> {
    let columns = simulation_output_columns(timestamp_column, price_column, side_column);

    let output_path = PathBuf::from(output_path);

    let summary = py
        .detach(|| {
            export_seeded_tick_simulation(
                &native_result.output,
                output_path.as_path(),
                reference_price,
                &columns,
                batch_size,
            )
        })
        .map_err(workflow_error_to_simulation_error)?;

    Ok(summary.record_count)
}

#[pyfunction]
#[pyo3(signature = (path_payload, output_path, reference_price, timestamp_column, price_column, calendar_registry, exchange_code, batch_size = DEFAULT_SIMULATION_EXPORT_BATCH_SIZE))]
fn export_coarse_simulation_artifact(
    py: Python<'_>,
    path_payload: &Bound<'_, PyAny>,
    output_path: &str,
    reference_price: f64,
    timestamp_column: &str,
    price_column: &str,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    batch_size: usize,
) -> PyResult<usize> {
    let coarse_path: janiform::simulation::CoarseSimulationPath =
        serde_json::from_value(py_any_to_json_value(path_payload)?).map_err(|source| {
            SimulationExecutionError::new_err(format!(
                "failed to deserialize coarse simulation path: {source}"
            ))
        })?;

    let columns = simulation_output_columns(timestamp_column, price_column, None);

    let output_path = PathBuf::from(output_path);

    let registry = calendar_registry.registry.clone();

    let exchange_code = exchange_code.to_string();

    let summary = py
        .detach(move || {
            export_coarse_simulation(
                &coarse_path,
                output_path.as_path(),
                reference_price,
                &columns,
                &registry,
                exchange_code.as_str(),
                batch_size,
            )
        })
        .map_err(workflow_error_to_simulation_error)?;

    Ok(summary.record_count)
}

fn run_simulation_with_optional_progress_events<TInput, TOutput, TRun, TRunWithObserver>(
    py: Python<'_>,
    panic_context: &'static str,
    run_input: TInput,
    event_callback: Option<&Bound<'_, PyAny>>,
    run_without_observer: TRun,
    run_with_observer: TRunWithObserver,
) -> PyResult<TOutput>
where
    TInput: Send + 'static,
    TOutput: Send + 'static,
    TRun: FnOnce(TInput) -> PyResult<TOutput> + Send + 'static,
    TRunWithObserver: FnOnce(TInput, &mut dyn FnMut(SimulationProgressEvent)) -> PyResult<TOutput>
        + Send
        + 'static,
{
    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_observer = Arc::clone(&callback_error_message);

    let event_callback = event_callback.map(|callback| callback.clone().unbind());

    let run_output = panic_to_pyresult(panic_context, SimulationExecutionError::new_err, || {
        py.detach(move || match event_callback {
            None => run_without_observer(run_input),
            Some(event_callback) => {
                let mut observer = simulation_progress_observer(
                    event_callback,
                    callback_error_message_for_observer,
                );

                run_with_observer(run_input, &mut observer)
            }
        })
    })?;

    raise_simulation_progress_callback_error(&callback_error_message)?;

    Ok(run_output)
}

fn simulation_progress_observer(
    event_callback: Py<PyAny>,
    callback_error_message: Arc<Mutex<Option<String>>>,
) -> impl FnMut(SimulationProgressEvent) {
    move |event: SimulationProgressEvent| {
        if simulation_progress_callback_failed(&callback_error_message) {
            return;
        }

        Python::attach(|py| {
            let callback_result = python_simulation_progress_event_to_py_object(py, &event)
                .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));

            if let Err(source) = callback_result {
                record_simulation_progress_callback_error(
                    &callback_error_message,
                    source.to_string(),
                );
            }
        });
    }
}

fn simulation_progress_callback_failed(
    callback_error_message: &Arc<Mutex<Option<String>>>,
) -> bool {
    callback_error_message
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

fn record_simulation_progress_callback_error(
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

fn raise_simulation_progress_callback_error(
    callback_error_message: &Arc<Mutex<Option<String>>>,
) -> PyResult<()> {
    let Ok(guard) = callback_error_message.lock() else {
        return Ok(());
    };

    let Some(error_message) = guard.as_ref() else {
        return Ok(());
    };

    Err(SimulationExecutionError::new_err(format!(
        "simulation progress callback failed: {error_message}"
    )))
}

fn python_simulation_progress_event_to_py_object(
    py: Python<'_>,
    event: &SimulationProgressEvent,
) -> PyResult<Py<PyAny>> {
    let serialized_event = match serde_json::to_value(event).map_err(|source| {
        SimulationExecutionError::new_err(format!(
            "failed to serialize simulation progress event: {source}"
        ))
    })? {
        Value::Object(object) => Value::Object(object),
        _ => {
            return Err(SimulationExecutionError::new_err(
                "failed to serialize simulation progress event: expected a JSON object",
            ));
        }
    };

    json_value_to_py_object(py, &serialized_event)
}

fn parse_seeded_simulation_mean_reversion_fitted_parameters(
    raw_mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
) -> PyResult<MeanReversionFittedParameters> {
    let parsed_parameters = serde_json::from_value::<MeanReversionFittedParameters>(
        py_any_to_json_value(raw_mean_reversion_fitted_parameters)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "mean_reversion_fitted_parameters failed to deserialize: {source}"
        ))
    })?;

    parsed_parameters.validate().map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "mean_reversion_fitted_parameters failed validation: {source}"
        ))
    })?;

    Ok(parsed_parameters)
}

fn parse_seeded_simulation_mean_reversion_runtime_interval_anchors(
    raw_mean_reversion_runtime_interval_anchors: &Bound<'_, PyAny>,
) -> PyResult<MeanReversionRuntimeIntervalAnchors> {
    let parsed_runtime_interval_anchors =
        serde_json::from_value::<MeanReversionRuntimeIntervalAnchors>(py_any_to_json_value(
            raw_mean_reversion_runtime_interval_anchors,
        )?)
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "mean_reversion_runtime_interval_anchors failed to deserialize: {source}"
            ))
        })?;

    parsed_runtime_interval_anchors
        .validate()
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "mean_reversion_runtime_interval_anchors failed validation: {source}"
            ))
        })?;

    Ok(parsed_runtime_interval_anchors)
}

fn parse_seeded_simulation_mean_reversion_boundary_state(
    raw_initial_mean_reversion_boundary_state: &Bound<'_, PyAny>,
) -> PyResult<MeanReversionBoundaryState> {
    let parsed_boundary_state = serde_json::from_value::<MeanReversionBoundaryState>(
        py_any_to_json_value(raw_initial_mean_reversion_boundary_state)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "initial_mean_reversion_boundary_state failed to deserialize: {source}"
        ))
    })?;

    parsed_boundary_state.validate().map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "initial_mean_reversion_boundary_state failed validation: {source}"
        ))
    })?;

    Ok(parsed_boundary_state)
}

fn parse_required_numeric_sequence(
    raw_sequence: &Bound<'_, PyAny>,
    context: &str,
) -> PyResult<Vec<f64>> {
    let parsed_sequence = serde_json::from_value::<Vec<f64>>(py_any_to_json_value(raw_sequence)?)
        .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "{context} failed to deserialize as a numeric sequence: {source}"
        ))
    })?;

    if parsed_sequence.iter().any(|value| !value.is_finite()) {
        return Err(SeasonalityInputValidationError::new_err(format!(
            "{context} must contain only finite numeric values"
        )));
    }

    Ok(parsed_sequence)
}

fn parse_seeded_simulation_kernel_parameters(
    raw_kernel_parameters: &Bound<'_, PyAny>,
) -> PyResult<HawkesKernelParameters> {
    let kernel_parameters_mapping = raw_kernel_parameters.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err("kernel_parameters must be a mapping")
    })?;

    let lambda_bar = parse_required_float_field_from_mapping(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "lambda_bar",
    )?;

    let beta = parse_required_float_field_from_mapping(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "beta",
    )?;

    let alpha = parse_required_hawkes_kernel_loadings_field(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "alpha",
    )?;

    let rho = parse_required_hawkes_kernel_loadings_field(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "rho",
    )?;

    HawkesKernelParameters::try_new(lambda_bar, beta, alpha, rho).map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "kernel_parameters payload failed Hawkes validation: {source}"
        ))
    })
}

fn parse_seeded_simulation_initial_excitation_state(
    raw_initial_excitation_state: &Bound<'_, PyAny>,
) -> PyResult<HawkesExcitationState> {
    let state_mapping = raw_initial_excitation_state
        .cast::<PyMapping>()
        .map_err(|_| {
            SeasonalityInputValidationError::new_err("initial_excitation_state must be a mapping")
        })?;

    Ok(HawkesExcitationState {
        y_plus_plus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_plus_plus",
        )?,
        y_plus_minus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_plus_minus",
        )?,
        y_minus_plus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_minus_plus",
        )?,
        y_minus_minus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_minus_minus",
        )?,
    })
}

fn parse_seeded_simulation_nig_shared_plus_shift_configuration(
    raw_configuration: &Bound<'_, PyAny>,
) -> PyResult<NigSharedPlusShiftConfiguration> {
    let configuration_mapping = raw_configuration.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "nig_shared_plus_shift_configuration must be a mapping",
        )
    })?;

    NigSharedPlusShiftConfiguration::try_new(
        parse_required_float_field_from_mapping(
            &configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "alpha",
        )?,
        parse_required_float_field_from_mapping(
            &configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "beta_0",
        )?,
        parse_required_float_field_from_mapping(
            &configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "delta",
        )?,
        parse_required_float_field_from_mapping(
            &configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "mu",
        )?,
        parse_required_float_field_from_mapping(
            &configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "delta_beta",
        )?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "nig_shared_plus_shift_configuration payload failed NIG validation: {source}"
        ))
    })
}

fn parse_seeded_simulation_state_snapshot_capture(
    raw_state_snapshot_capture: &str,
) -> PyResult<SeededTickEventStateSnapshotCapture> {
    match raw_state_snapshot_capture {
        "disabled" => Ok(SeededTickEventStateSnapshotCapture::Disabled),
        "post_event_transition" => Ok(SeededTickEventStateSnapshotCapture::PostEventTransition),
        _ => Err(SeasonalityInputValidationError::new_err(format!(
            "state_snapshot_capture must be either 'disabled' or 'post_event_transition', received '{raw_state_snapshot_capture}'"
        ))),
    }
}

fn parse_required_hawkes_kernel_loadings_field(
    parent_mapping: &Bound<'_, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<HawkesKernelLoadings> {
    let loadings_mapping =
        parse_required_mapping_field(parent_mapping, parent_context, field_name)?;

    let loadings_context = format!("{parent_context}.{field_name}");

    Ok(HawkesKernelLoadings {
        plus_plus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "plus_plus",
        )?,
        plus_minus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "plus_minus",
        )?,
        minus_plus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "minus_plus",
        )?,
        minus_minus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "minus_minus",
        )?,
    })
}

fn parse_required_mapping_field<'py>(
    parent_mapping: &Bound<'py, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<Bound<'py, PyMapping>> {
    let field_value = parse_required_field(parent_mapping, parent_context, field_name)?;

    let field_mapping = field_value.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err(format!(
            "{parent_context}.{field_name} must be a mapping"
        ))
    })?;

    Ok(field_mapping.clone())
}

fn parse_required_float_field_from_mapping(
    parent_mapping: &Bound<'_, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<f64> {
    let field_value = parse_required_field(parent_mapping, parent_context, field_name)?;

    let parsed_value = field_value.extract::<f64>().map_err(|_| {
        SeasonalityInputValidationError::new_err(format!(
            "{parent_context}.{field_name} must be a finite number"
        ))
    })?;

    if !parsed_value.is_finite() {
        return Err(SeasonalityInputValidationError::new_err(format!(
            "{parent_context}.{field_name} must be a finite number"
        )));
    }

    Ok(parsed_value)
}

fn parse_required_field<'py>(
    parent_mapping: &Bound<'py, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    parent_mapping
        .contains(field_name)?
        .then(|| parent_mapping.get_item(field_name))
        .ok_or_else(|| {
            SeasonalityInputValidationError::new_err(format!(
                "{parent_context} is missing required field '{field_name}'"
            ))
        })?
}

fn map_seeded_tick_simulation_run_error(
    error: janiform::simulation::SeededTickSimulationRunError,
) -> PyErr {
    SeasonalityInputValidationError::new_err(error.to_string())
}

fn simulation_output_columns(
    timestamp_column: &str,
    price_column: &str,
    side_column: Option<&str>,
) -> SimulationOutputColumns {
    SimulationOutputColumns {
        timestamp_column: timestamp_column.to_string(),
        price_column: price_column.to_string(),
        side_column: side_column.map(str::to_string),
    }
}

fn workflow_error_to_simulation_error(error: WorkflowError) -> PyErr {
    match error {
        WorkflowError::SimulationValidationFailed { field, message } => {
            SimulationExecutionError::new_err(format!("{field}: {message}"))
        }
        WorkflowError::SimulationArtifactWrite { path, source } => {
            SimulationExecutionError::new_err(format!(
                "failed to write simulation artifact `{}`: {source}",
                path.display()
            ))
        }
        _ => SimulationExecutionError::new_err(error.to_string()),
    }
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_coarse_simulation, module)?)?;
    module.add_function(wrap_pyfunction!(export_coarse_simulation_artifact, module)?)?;
    module.add_function(wrap_pyfunction!(
        export_seeded_tick_simulation_artifact,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        materialize_endogenous_mean_reversion_runtime_interval_anchors,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_simulation_from_model_artifact,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(run_seeded_tick_event_simulation, module)?)?;

    Ok(())
}
