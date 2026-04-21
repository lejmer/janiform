use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use janiform::calibration::{
    CalibrationMarketDataPreparationRequest as MarketDataPreparationRequestCore,
    CalibrationReturnComponentsActivityContextPayload,
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationStagedExecutionInput,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    CalibrationResumeSettings, CurrentStateEstimationInput, MarketDataInputFileReference, MarketDataLoadingOutput,
    MarketDataLoadingRequest, MarketDataNormalizationPolicy, MarketDataSessionConfiguration,
    MarketDataSessionResolutionPayload, MarketDataSourceProbePayload,
    MarketDataTickDataColumnMapping, MarketDataTranslationPreprocessingOutput,
    MarketDataTranslationPreprocessingRequest,
    PreparedMarketDataCalibrationPlan as PreparedMarketDataPlanCore,
    ReturnComponentsStageFittedParameters, ReturnComponentsStageObservedSignalSummaryStatistics,
    RuntimeStructuralSeasonalityMaterializationInput,
    estimate_current_state_from_observed_data as estimate_current_state_from_observed_data_core,
    evaluate_return_components_exact_candidate_from_bridge,
    load_and_preprocess_market_data_translation_request as load_and_preprocess_market_data_translation_request_native,
    materialize_runtime_structural_seasonality as materialize_runtime_structural_seasonality_core,
    normalize_market_data_source as normalize_market_data_source_native,
    prepare_market_data_calibration_plan as prepare_market_data_calibration_plan_core,
    prepare_market_data_calibration_plan_from_phases as prepare_market_data_calibration_plan_from_phases_core,
    preprocess_market_data_translation_request as preprocess_market_data_translation_request_native,
    probe_market_data_source as probe_market_data_source_native,
    resolve_market_data_session_configuration as resolve_market_data_session_configuration_native,
    run_staged_calibration_from_execution_input,
    run_staged_calibration_from_execution_input_with_observer,
};
use janiform::hawkes::{HawkesEventSide, HawkesKernelParameters};
use janiform::simulation::MeanReversionFittedParameters;
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyAnyMethods, PyDict, PyDictMethods, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};
use serde_json::Value;

use super::calendar::parse_timestamp_utc;
use crate::CalibrationExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::{
    PyCalendarRegistry, PyCalibrationMemoryCheckpointStore, PyNormalizedMarketDataHandle,
    PyPreparedCalibrationExecutionHandle,
};

#[pyfunction]
#[pyo3(signature = (preprocessing_request, calendar_registry))]
fn preprocess_market_data_translation_request(
    py: Python<'_>,
    preprocessing_request: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
) -> PyResult<Py<PyAny>> {
    let preprocessing_request: MarketDataTranslationPreprocessingRequest =
        serde_json::from_value(py_any_to_json_value(preprocessing_request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data preprocessing request: {source}"
            ))
        })?;

    let preprocessing_output = py
        .allow_threads(|| {
            preprocess_market_data_translation_request_native(
                preprocessing_request,
                &calendar_registry.registry,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_translation_preprocessing_output_to_py_object(py, &preprocessing_output).map_err(
        |source| {
            CalibrationExecutionError::new_err(format!(
                "failed to materialize market-data preprocessing output: {source}"
            ))
        },
    )
}

#[pyfunction]
#[pyo3(signature = (loading_request))]
fn load_and_preprocess_market_data_translation_request(
    py: Python<'_>,
    loading_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let loading_request: MarketDataLoadingRequest =
        serde_json::from_value(py_any_to_json_value(loading_request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data loading request: {source}"
            ))
        })?;

    let loading_output = py
        .allow_threads(|| {
            load_and_preprocess_market_data_translation_request_native(loading_request)
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_loading_output_to_py_object(py, &loading_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to materialize market-data loading output: {source}"
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (tick_data_source))]
fn probe_market_data_source(
    py: Python<'_>,
    tick_data_source: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let tick_data_source: MarketDataInputFileReference =
        serde_json::from_value(py_any_to_json_value(tick_data_source)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize tick-data source reference: {source}"
            ))
        })?;

    let source_probe = py
        .allow_threads(|| probe_market_data_source_native(&tick_data_source))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_source_probe_to_py_object(py, &source_probe).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to materialize market-data source probe: {source}"
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (tick_data_source, tick_data_column_mapping, normalization_policy, require_hawkes = false))]
fn normalize_market_data_source(
    py: Python<'_>,
    tick_data_source: &Bound<'_, PyAny>,
    tick_data_column_mapping: &Bound<'_, PyAny>,
    normalization_policy: &Bound<'_, PyAny>,
    require_hawkes: bool,
) -> PyResult<Py<PyAny>> {
    let tick_data_source: MarketDataInputFileReference =
        serde_json::from_value(py_any_to_json_value(tick_data_source)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize tick-data source reference: {source}"
            ))
        })?;

    let tick_data_column_mapping: MarketDataTickDataColumnMapping = serde_json::from_value(
        py_any_to_json_value(tick_data_column_mapping)?,
    )
    .map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to deserialize tick-data column mapping: {source}"
        ))
    })?;

    let normalization_policy: MarketDataNormalizationPolicy =
        serde_json::from_value(py_any_to_json_value(normalization_policy)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data normalization policy: {source}"
            ))
        })?;

    let normalized_output = py
        .allow_threads(|| {
            normalize_market_data_source_native(
                &tick_data_source,
                &tick_data_column_mapping,
                &normalization_policy,
                require_hawkes,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let result = PyDict::new(py);
    let normalized_handle = Py::new(
        py,
        PyNormalizedMarketDataHandle {
            normalized_output: Arc::new(normalized_output),
        },
    )?;

    result.set_item("normalized_handle", &normalized_handle)?;
    result.set_item(
        "tick_data_summary",
        json_value_to_py_object(
            py,
            &serde_json::to_value(
                &normalized_handle
                    .borrow(py)
                    .normalized_output
                    .tick_data_summary,
            )
            .map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize normalized tick-data summary: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item(
        "reference_price",
        normalized_handle
            .borrow(py)
            .normalized_output
            .reference_price,
    )?;
    result.set_item(
        "fit_warning",
        normalized_handle
            .borrow(py)
            .normalized_output
            .fit_warning
            .clone(),
    )?;

    Ok(result.into())
}

#[pyfunction]
#[pyo3(signature = (session_configuration, normalized_market_data_handle = None))]
fn resolve_market_data_session(
    py: Python<'_>,
    session_configuration: &Bound<'_, PyAny>,
    normalized_market_data_handle: Option<&PyNormalizedMarketDataHandle>,
) -> PyResult<Py<PyAny>> {
    let session_configuration: MarketDataSessionConfiguration =
        serde_json::from_value(py_any_to_json_value(session_configuration)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data session configuration: {source}"
            ))
        })?;

    let session_resolution = py
        .allow_threads(|| {
            resolve_market_data_session_configuration_native(
                session_configuration,
                normalized_market_data_handle.map(|handle| handle.normalized_output.as_ref()),
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_session_resolution_to_py_object(py, &session_resolution).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to materialize market-data session resolution: {source}"
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (request, normalized_market_data_handle, session_resolution, cpu_count = None))]
fn prepare_market_data_calibration_plan_from_phases(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    normalized_market_data_handle: &PyNormalizedMarketDataHandle,
    session_resolution: &Bound<'_, PyAny>,
    cpu_count: Option<usize>,
) -> PyResult<Py<PyAny>> {
    let request: MarketDataPreparationRequestCore =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data calibration request: {source}"
            ))
        })?;
    let session_resolution: MarketDataSessionResolutionPayload =
        serde_json::from_value(py_any_to_json_value(session_resolution)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data session resolution payload: {source}"
            ))
        })?;

    let loading_output = py
        .allow_threads(|| {
            prepare_market_data_calibration_plan_from_phases_core(
                request,
                normalized_market_data_handle.normalized_output.as_ref(),
                session_resolution,
                cpu_count,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    prepared_market_data_plan_to_py_object(py, loading_output)
}

fn market_data_loading_output_to_py_object(
    py: Python<'_>,
    loading_output: &MarketDataLoadingOutput,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(loading_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data loading output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn market_data_source_probe_to_py_object(
    py: Python<'_>,
    source_probe: &MarketDataSourceProbePayload,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(source_probe).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data source probe: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn market_data_session_resolution_to_py_object(
    py: Python<'_>,
    session_resolution: &MarketDataSessionResolutionPayload,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(session_resolution).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data session resolution: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn market_data_translation_preprocessing_output_to_py_object(
    py: Python<'_>,
    preprocessing_output: &MarketDataTranslationPreprocessingOutput,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(preprocessing_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data preprocessing output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn prepared_market_data_plan_to_py_object(
    py: Python<'_>,
    prepared_plan: PreparedMarketDataPlanCore,
) -> PyResult<Py<PyAny>> {
    let result = PyDict::new(py);

    let prepared_handle = Py::new(
        py,
        PyPreparedCalibrationExecutionHandle {
            execution_input: prepared_plan.execution_input.clone(),
        },
    )?;

    result.set_item("prepared_handle", prepared_handle)?;
    result.set_item(
        "staged_request",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&prepared_plan.execution_input.request).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize staged request: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item(
        "diagnostics",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&prepared_plan.diagnostics).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize preparation diagnostics: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item(
        "provenance",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&prepared_plan.provenance).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize preparation provenance: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item(
        "session_resolution",
        market_data_session_resolution_to_py_object(py, &prepared_plan.session_resolution)?,
    )?;
    result.set_item("fit_warning", prepared_plan.fit_warning)?;
    result.set_item("reference_price", prepared_plan.reference_price)?;

    Ok(result.into_any().unbind())
}

#[pyfunction]
#[pyo3(signature = (request, cpu_count = None))]
fn prepare_market_data_calibration_plan(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    cpu_count: Option<usize>,
) -> PyResult<Py<PyAny>> {
    let request: MarketDataPreparationRequestCore =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data calibration request: {source}"
            ))
        })?;

    let prepared_plan = py
        .allow_threads(|| prepare_market_data_calibration_plan_core(request, cpu_count))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    prepared_market_data_plan_to_py_object(py, prepared_plan)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, calendar_registry, memory_checkpoint_store = None))]
fn run_prepared_staged_calibration(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    calendar_registry: &PyCalendarRegistry,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let report = py
        .allow_threads(|| {
            run_staged_calibration_from_execution_input(
                prepared_handle.execution_input.clone(),
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, calendar_registry, event_callback, memory_checkpoint_store = None))]
fn run_prepared_staged_calibration_with_events(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    calendar_registry: &PyCalendarRegistry,
    event_callback: &Bound<'_, PyAny>,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let event_callback = event_callback.clone().unbind();

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_observer = Arc::clone(&callback_error_message);

    let report = py
        .allow_threads(move || {
            let mut observer = |event: CalibrationStagedRunObserverEvent| {
                if callback_error_message_for_observer
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .is_some()
                {
                    return;
                }

                Python::with_gil(|py| {
                    let callback_result = python_staged_calibration_event_to_py_object(py, &event)
                        .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));
                    if let Err(source) = callback_result {
                        if let Ok(mut guard) = callback_error_message_for_observer.lock() {
                            if guard.is_none() {
                                *guard = Some(source.to_string());
                            }
                        }
                    }
                });
            };

            run_staged_calibration_from_execution_input_with_observer(
                prepared_handle.execution_input.clone(),
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
                &mut observer,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    if let Ok(guard) = callback_error_message.lock() {
        if let Some(error_message) = guard.as_ref() {
            return Err(CalibrationExecutionError::new_err(format!(
                "staged calibration event callback failed: {error_message}"
            )));
        }
    }

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, calendar_registry, resume_settings, memory_checkpoint_store = None))]
fn run_prepared_staged_calibration_with_resume(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    calendar_registry: &PyCalendarRegistry,
    resume_settings: &Bound<'_, PyAny>,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let resume_settings: CalibrationResumeSettings =
        serde_json::from_value(py_any_to_json_value(resume_settings)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize calibration resume settings: {source}"
            ))
        })?;

    let mut execution_input = prepared_handle.execution_input.clone();
    execution_input.request.resume_settings = Some(resume_settings);

    let report = py
        .allow_threads(move || {
            run_staged_calibration_from_execution_input(
                execution_input,
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, calendar_registry, resume_settings, event_callback, memory_checkpoint_store = None))]
fn run_prepared_staged_calibration_with_resume_and_events(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    calendar_registry: &PyCalendarRegistry,
    resume_settings: &Bound<'_, PyAny>,
    event_callback: &Bound<'_, PyAny>,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let resume_settings: CalibrationResumeSettings =
        serde_json::from_value(py_any_to_json_value(resume_settings)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize calibration resume settings: {source}"
            ))
        })?;
    let event_callback = event_callback.clone().unbind();

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_observer = Arc::clone(&callback_error_message);

    let mut execution_input = prepared_handle.execution_input.clone();
    execution_input.request.resume_settings = Some(resume_settings);

    let report = py
        .allow_threads(move || {
            let mut observer = |event: CalibrationStagedRunObserverEvent| {
                if callback_error_message_for_observer
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .is_some()
                {
                    return;
                }

                Python::with_gil(|py| {
                    let callback_result = python_staged_calibration_event_to_py_object(py, &event)
                        .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));
                    if let Err(source) = callback_result {
                        if let Ok(mut guard) = callback_error_message_for_observer.lock() {
                            if guard.is_none() {
                                *guard = Some(source.to_string());
                            }
                        }
                    }
                });
            };

            run_staged_calibration_from_execution_input_with_observer(
                execution_input,
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
                &mut observer,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    if let Ok(guard) = callback_error_message.lock() {
        if let Some(error_message) = guard.as_ref() {
            return Err(CalibrationExecutionError::new_err(format!(
                "staged calibration event callback failed: {error_message}"
            )));
        }
    }

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (execution_input, calendar_registry, memory_checkpoint_store = None))]
fn run_staged_calibration(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let execution_input: CalibrationStagedExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize staged calibration execution input: {source}"
            ))
        })?;

    let report = py
        .allow_threads(move || {
            run_staged_calibration_from_execution_input(
                execution_input,
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (execution_input, calendar_registry, event_callback, memory_checkpoint_store = None))]
fn run_staged_calibration_with_events(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
    event_callback: &Bound<'_, PyAny>,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let execution_input: CalibrationStagedExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize staged calibration execution input: {source}"
            ))
        })?;

    let event_callback = event_callback.clone().unbind();

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_observer = Arc::clone(&callback_error_message);

    let report = py
        .allow_threads(move || {
            let mut observer = |event: CalibrationStagedRunObserverEvent| {
                if callback_error_message_for_observer
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .is_some()
                {
                    return;
                }

                Python::with_gil(|py| {
                    let callback_result = python_staged_calibration_event_to_py_object(py, &event)
                        .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));
                    if let Err(source) = callback_result {
                        if let Ok(mut guard) = callback_error_message_for_observer.lock() {
                            if guard.is_none() {
                                *guard = Some(source.to_string());
                            }
                        }
                    }
                });
            };

            run_staged_calibration_from_execution_input_with_observer(
                execution_input,
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
                &mut observer,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    if let Ok(guard) = callback_error_message.lock() {
        if let Some(error_message) = guard.as_ref() {
            return Err(CalibrationExecutionError::new_err(format!(
                "staged calibration event callback failed: {error_message}"
            )));
        }
    }

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (candidate_generation_input, observed_summary_statistics, fitted_parameters, stage_seed, hawkes_payload = None))]
fn evaluate_return_components_candidate(
    py: Python<'_>,
    candidate_generation_input: &Bound<'_, PyAny>,
    observed_summary_statistics: &Bound<'_, PyAny>,
    fitted_parameters: &Bound<'_, PyAny>,
    stage_seed: u64,
    hawkes_payload: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let candidate_generation_input: CalibrationReturnComponentsStageCandidateGenerationInput =
        serde_json::from_value(py_any_to_json_value(candidate_generation_input)?).map_err(
            |source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize return-components candidate generation input: {source}"
                ))
            },
        )?;

    let observed_summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics =
        serde_json::from_value(py_any_to_json_value(observed_summary_statistics)?).map_err(
            |source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize return-components observed summary statistics: {source}"
                ))
            },
        )?;

    let fitted_parameters: ReturnComponentsStageFittedParameters =
        serde_json::from_value(py_any_to_json_value(fitted_parameters)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize return-components fitted parameters: {source}"
            ))
        })?;

    let activity_context_payload: Option<CalibrationReturnComponentsActivityContextPayload> =
        hawkes_payload
            .map(py_any_to_json_value)
            .transpose()?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize Hawkes activity context payload: {source}"
                ))
            })?;

    let evaluation = py
        .allow_threads(|| {
            evaluate_return_components_exact_candidate_from_bridge(
                candidate_generation_input,
                activity_context_payload,
                observed_summary_statistics,
                fitted_parameters,
                stage_seed,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let raw_value = serde_json::to_value(evaluation).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize return-components exact candidate evaluation: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

#[pyfunction]
fn estimate_current_state_from_observed_data(
    py: Python<'_>,
    kernel_parameters: &Bound<'_, PyAny>,
    mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
    state_time_utc_nanos: i64,
    observed_log_price: f64,
    observation_timestamps_utc: &Bound<'_, PyAny>,
    observation_trade_prices: &Bound<'_, PyAny>,
    observation_trade_sides: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_kernel_parameters: HawkesKernelParameters =
        serde_json::from_value(py_any_to_json_value(kernel_parameters)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize current-state kernel parameters: {source}"
            ))
        })?;
    let parsed_mean_reversion_fitted_parameters: MeanReversionFittedParameters =
        serde_json::from_value(py_any_to_json_value(mean_reversion_fitted_parameters)?).map_err(
            |source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize current-state mean-reversion fitted parameters: {source}"
                ))
            },
        )?;
    let parsed_observation_timestamps_utc: Vec<String> =
        observation_timestamps_utc.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "current-state observation_timestamps_utc must be an iterable of RFC3339 UTC timestamp strings",
            )
        })?;
    let parsed_observation_trade_prices: Vec<f64> =
        observation_trade_prices.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "current-state observation_trade_prices must be an iterable of float values",
            )
        })?;
    let parsed_observation_trade_sides: Vec<String> =
        observation_trade_sides.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "current-state observation_trade_sides must be an iterable of trade-side strings",
            )
        })?;

    let parsed_observation_timestamp_utc_nanos = parsed_observation_timestamps_utc
        .iter()
        .enumerate()
        .map(|(observation_index, raw_timestamp_utc)| {
            DateTime::parse_from_rfc3339(raw_timestamp_utc)
                .map(|timestamp| timestamp.with_timezone(&Utc))
                .map_err(|_| {
                    CalibrationExecutionError::new_err(format!(
                        "current-state observation_timestamps_utc[{observation_index}] must be a RFC3339 timestamp with UTC offset, received `{raw_timestamp_utc}`"
                    ))
                })?
                .timestamp_nanos_opt()
                .ok_or_else(|| {
                    CalibrationExecutionError::new_err(format!(
                        "current-state observation_timestamps_utc[{observation_index}] is outside the supported nanosecond timestamp range"
                    ))
                })
        })
        .collect::<PyResult<Vec<_>>>()?;

    let parsed_observation_trade_sides = parsed_observation_trade_sides
        .iter()
        .map(|raw_trade_side: &String| {
            HawkesEventSide::try_from(raw_trade_side.as_str()).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to parse current-state trade_side: {source}"
                ))
            })
        })
        .collect::<PyResult<Vec<_>>>()?;

    let estimation_output =
        estimate_current_state_from_observed_data_core(CurrentStateEstimationInput {
            kernel_parameters: parsed_kernel_parameters,
            mean_reversion_fitted_parameters: parsed_mean_reversion_fitted_parameters,
            state_time_utc_nanos,
            observed_log_price,
            observation_timestamp_utc_nanos: parsed_observation_timestamp_utc_nanos,
            observation_trade_prices: parsed_observation_trade_prices,
            observation_trade_sides: parsed_observation_trade_sides,
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let serialized_output = serde_json::to_value(&estimation_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize current-state estimation output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_output)
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, interval_duration_nanos, intraday_minutes_per_bin, intraday_factor_by_bin, day_of_week_factor, day_of_year_factor))]
fn materialize_runtime_structural_seasonality(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    intraday_minutes_per_bin: u16,
    intraday_factor_by_bin: &Bound<'_, PyAny>,
    day_of_week_factor: &Bound<'_, PyAny>,
    day_of_year_factor: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;
    let parsed_intraday_factor_by_bin: Vec<f64> =
        intraday_factor_by_bin.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "runtime seasonality intraday_factor_by_bin must be an iterable of float values",
            )
        })?;
    let parsed_day_of_week_factor: Vec<f64> = day_of_week_factor.extract().map_err(|_| {
        CalibrationExecutionError::new_err(
            "runtime seasonality day_of_week_factor must be an iterable of float values",
        )
    })?;
    let parsed_day_of_year_factor: Vec<f64> = day_of_year_factor.extract().map_err(|_| {
        CalibrationExecutionError::new_err(
            "runtime seasonality day_of_year_factor must be an iterable of float values",
        )
    })?;

    let parsed_range_start_utc_nanos =
        parsed_range_start_utc.timestamp_nanos_opt().ok_or_else(|| {
            CalibrationExecutionError::new_err(
                "runtime seasonality range_start_utc is outside the supported nanosecond timestamp range",
            )
        })?;
    let parsed_range_end_utc_nanos = parsed_range_end_utc.timestamp_nanos_opt().ok_or_else(|| {
        CalibrationExecutionError::new_err(
            "runtime seasonality range_end_utc is outside the supported nanosecond timestamp range",
        )
    })?;

    let materialization = py
        .allow_threads(|| {
            materialize_runtime_structural_seasonality_core(
                &calendar_registry.registry,
                RuntimeStructuralSeasonalityMaterializationInput {
                    exchange_code: exchange_code.to_string(),
                    intraday_minutes_per_bin,
                    intraday_factor_by_bin: parsed_intraday_factor_by_bin,
                    day_of_week_factor: parsed_day_of_week_factor,
                    day_of_year_factor: parsed_day_of_year_factor,
                    range_start_utc_nanos: parsed_range_start_utc_nanos,
                    range_end_utc_nanos: parsed_range_end_utc_nanos,
                    interval_duration_nanos,
                },
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let serialized_output = serde_json::to_value(&materialization).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize runtime structural seasonality materialization: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_output)
}

fn python_staged_calibration_report_to_py_object(
    py: Python<'_>,
    report: &CalibrationStagedRunReport,
) -> PyResult<Py<PyAny>> {
    let mut serialized_report = match serde_json::to_value(report).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize staged calibration report: {source}"
        ))
    })? {
        Value::Object(object) => object,
        _ => {
            return Err(CalibrationExecutionError::new_err(
                "failed to serialize staged calibration report: expected a JSON object",
            ));
        }
    };

    let flattened_stage_reports = report
        .stage_reports
        .iter()
        .map(flatten_stage_report_for_python)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to flatten staged calibration report for Python: {source}"
            ))
        })?;

    serialized_report.insert(
        "stage_reports".to_string(),
        Value::Array(flattened_stage_reports),
    );

    json_value_to_py_object(py, &Value::Object(serialized_report))
}

fn python_staged_calibration_event_to_py_object(
    py: Python<'_>,
    event: &CalibrationStagedRunObserverEvent,
) -> PyResult<Py<PyAny>> {
    let mut serialized_event = match serde_json::to_value(event).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize staged calibration observer event: {source}"
        ))
    })? {
        Value::Object(object) => object,
        _ => {
            return Err(CalibrationExecutionError::new_err(
                "failed to serialize staged calibration observer event: expected a JSON object",
            ));
        }
    };

    if serialized_event.contains_key("stage_report") {
        let flattened_stage_report = match event {
            CalibrationStagedRunObserverEvent::StageCompleted { stage_report, .. } => {
                flatten_stage_report_for_python(stage_report)
            }
            _ => unreachable!("stage_report field is only emitted for stage_completed events"),
        }
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to flatten staged calibration observer event stage report: {source}"
            ))
        })?;

        serialized_event.insert("stage_report".to_string(), flattened_stage_report);
    }

    json_value_to_py_object(py, &Value::Object(serialized_event))
}

fn flatten_stage_report_for_python(
    report: &CalibrationStagedRunStageReport,
) -> Result<Value, serde_json::Error> {
    let (stage_report_kind, raw_report) = match report {
        CalibrationStagedRunStageReport::Seasonality(stage_report) => {
            ("seasonality", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::Hawkes(stage_report) => {
            ("hawkes", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::ReturnComponents(stage_report) => {
            ("return_components", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::MeanReversion(stage_report) => {
            ("mean_reversion", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::JointRefinement(stage_report) => {
            ("joint_refinement", serde_json::to_value(stage_report)?)
        }
    };

    let mut flattened_report = match raw_report {
        Value::Object(object) => object,
        _ => {
            return Err(serde_json::Error::io(std::io::Error::other(
                "stage report should serialize to a JSON object",
            )));
        }
    };

    flattened_report.insert(
        "stage_report_kind".to_string(),
        Value::String(stage_report_kind.to_string()),
    );

    Ok(Value::Object(flattened_report))
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        preprocess_market_data_translation_request,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        load_and_preprocess_market_data_translation_request,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(probe_market_data_source, module)?)?;
    module.add_function(wrap_pyfunction!(normalize_market_data_source, module)?)?;
    module.add_function(wrap_pyfunction!(resolve_market_data_session, module)?)?;
    module.add_function(wrap_pyfunction!(
        prepare_market_data_calibration_plan,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        prepare_market_data_calibration_plan_from_phases,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(run_prepared_staged_calibration, module)?)?;
    module.add_function(wrap_pyfunction!(
        run_prepared_staged_calibration_with_events,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_prepared_staged_calibration_with_resume,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        run_prepared_staged_calibration_with_resume_and_events,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(run_staged_calibration, module)?)?;
    module.add_function(wrap_pyfunction!(
        run_staged_calibration_with_events,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        evaluate_return_components_candidate,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        estimate_current_state_from_observed_data,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        materialize_runtime_structural_seasonality,
        module
    )?)?;

    Ok(())
}
