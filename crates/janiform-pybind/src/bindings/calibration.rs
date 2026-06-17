use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;

use chrono::{DateTime, Utc};
use janiform::calibration::artifacts::{
    CalibrationResumeSettings, ReturnComponentsStageFittedParameters,
};
use janiform::calibration::current_state::{
    CurrentStateEstimationInput,
    estimate_current_state_from_observed_data as estimate_current_state_from_observed_data_core,
};
use janiform::calibration::market_data::{
    MarketDataFitInputInferenceRequest, MarketDataInputFileReference, MarketDataLoadingOutput,
    MarketDataLoadingRequest, MarketDataNormalizationPolicy, MarketDataSessionConfiguration,
    MarketDataSessionResolutionPayload, MarketDataSourceProbePayload,
    MarketDataTickDataColumnMapping, MarketDataTranslationCalibrationOutput,
    MarketDataTranslationCalibrationRequest,
    PreparedMarketDataCalibrationPlan as PreparedMarketDataPlanCore,
    calibrate_market_data_translation_request as calibrate_market_data_translation_request_native,
    exchange_code_from_market_profile_reference as exchange_code_from_market_profile_reference_native,
    exchange_code_from_session_reference as exchange_code_from_session_reference_native,
    infer_bundled_market_profile_from_tick_data_path,
    infer_market_data_fit_inputs as infer_market_data_fit_inputs_native,
    load_and_calibrate_market_data_translation_request as load_and_calibrate_market_data_translation_request_native,
    normalize_market_data_source as normalize_market_data_source_native,
    prepare_market_data_calibration_plan as prepare_market_data_calibration_plan_core,
    prepare_market_data_calibration_plan_from_phases as prepare_market_data_calibration_plan_from_phases_core,
    probe_market_data_source as probe_market_data_source_native,
    resolve_market_data_session_configuration as resolve_market_data_session_configuration_native,
    resolve_market_profile_configuration as resolve_market_profile_configuration_native,
};
use janiform::calibration::presets::{
    built_in_preset_registry_index as built_in_preset_registry_index_native,
    compute_preset_artifact_content_hash as compute_preset_artifact_content_hash_native,
    deserialize_preset_artifact_json as deserialize_preset_artifact_json_native,
    normalize_preset_artifact as normalize_preset_artifact_native,
    preset_registry_index as preset_registry_index_native,
    resolve_preset_artifact as resolve_preset_artifact_native,
    resolve_preset_snapshot as resolve_preset_snapshot_native,
    resolve_preset_workflow_configuration as resolve_preset_workflow_configuration_native,
    serialize_preset_artifact as serialize_preset_artifact_native,
};
use janiform::calibration::staged::{
    CalibrationMarketDataPreparationRequest as MarketDataPreparationRequestCore,
    CalibrationReturnComponentsActivityContextPayload,
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationStagedExecutionInput,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    RuntimeStructuralSeasonalityMaterializationInput,
    evaluate_return_components_exact_candidate_from_bridge,
    materialize_runtime_structural_seasonality as materialize_runtime_structural_seasonality_core,
    materialize_surprise_news_weekly_profile_multipliers_for_interval_range as materialize_surprise_news_weekly_profile_multipliers_for_interval_range_core,
    run_staged_calibration_from_execution_input,
    run_staged_calibration_from_execution_input_with_observer,
};
use janiform::model::hawkes::{HawkesEventSide, HawkesKernelParameters};
use janiform::model::long_variance::LongVarianceParameters;
use janiform::simulation::MeanReversionFittedParameters;
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyAnyMethods, PyDict, PyDictMethods, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};
use serde_json::Value;

use super::calendar::parse_timestamp_utc;
use super::events::{callback_error_state, emit_callback_event_payload, raise_callback_error};
use crate::CalibrationExecutionError;
use crate::native_interrupt::{
    NativeInterruptMode, run_interruptible_native, run_interruptible_native_with_poll,
};
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::{
    PyCalendarRegistry, PyCalibrationMemoryCheckpointStore, PyNormalizedMarketDataHandle,
    PyPreparedCalibrationExecutionHandle,
};

#[pyfunction]
#[pyo3(signature = (tick_data_path))]
fn infer_bundled_market_profile_for_tick_data_path(
    py: Python<'_>,
    tick_data_path: &str,
) -> PyResult<Py<PyAny>> {
    let inferred_profile =
        infer_bundled_market_profile_from_tick_data_path(Path::new(tick_data_path))
            .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let raw_value = serde_json::to_value(inferred_profile).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize inferred market profile: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

#[pyfunction]
#[pyo3(signature = (inference_request))]
fn infer_market_data_fit_inputs(
    py: Python<'_>,
    inference_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let inference_request: MarketDataFitInputInferenceRequest =
        serde_json::from_value(py_any_to_json_value(inference_request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data fit inference request: {source}"
            ))
        })?;

    let inference_output = py
        .detach(|| infer_market_data_fit_inputs_native(inference_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let raw_value = serde_json::to_value(inference_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data fit inference output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

#[pyfunction]
#[pyo3(signature = (_request))]
fn built_in_preset_registry_index(
    py: Python<'_>,
    _request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let index_payload = py
        .detach(built_in_preset_registry_index_native)
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    json_value_to_py_object(py, &index_payload)
}

#[pyfunction]
#[pyo3(signature = (index_request))]
fn preset_registry_index(py: Python<'_>, index_request: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let index_request = py_any_to_json_value(index_request)?;

    let index_payload = py
        .detach(|| preset_registry_index_native(index_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    json_value_to_py_object(py, &index_payload)
}

#[pyfunction]
#[pyo3(signature = (resolution_request))]
fn resolve_preset_artifact(
    py: Python<'_>,
    resolution_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let resolution_request = py_any_to_json_value(resolution_request)?;

    let resolved_artifact = py
        .detach(|| resolve_preset_artifact_native(resolution_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    json_value_to_py_object(py, &resolved_artifact)
}

#[pyfunction]
#[pyo3(signature = (snapshot_request))]
fn resolve_preset_snapshot(
    py: Python<'_>,
    snapshot_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let snapshot_request = py_any_to_json_value(snapshot_request)?;

    let snapshot_payload = py
        .detach(|| resolve_preset_snapshot_native(snapshot_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    json_value_to_py_object(py, &snapshot_payload)
}

#[pyfunction]
#[pyo3(signature = (artifact_request))]
fn serialize_preset_artifact(
    py: Python<'_>,
    artifact_request: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let artifact = preset_artifact_from_request(artifact_request)?;

    py.detach(|| serialize_preset_artifact_native(artifact))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))
}

#[pyfunction]
#[pyo3(signature = (artifact_request))]
fn deserialize_preset_artifact(
    py: Python<'_>,
    artifact_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let request = py_any_to_json_value(artifact_request)?;
    let payload_json = preset_artifact_json_from_request(request)?;

    let artifact = py
        .detach(|| deserialize_preset_artifact_json_native(payload_json.as_str()))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    json_value_to_py_object(py, &artifact)
}

#[pyfunction]
#[pyo3(signature = (artifact_request))]
fn normalize_preset_artifact(
    py: Python<'_>,
    artifact_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let artifact = preset_artifact_from_request(artifact_request)?;

    let normalized_artifact = py
        .detach(|| normalize_preset_artifact_native(artifact))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    json_value_to_py_object(py, &normalized_artifact)
}

#[pyfunction]
#[pyo3(signature = (artifact_request))]
fn compute_preset_artifact_content_hash(
    py: Python<'_>,
    artifact_request: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let artifact = preset_artifact_from_request(artifact_request)?;

    py.detach(|| compute_preset_artifact_content_hash_native(artifact))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))
}

fn preset_artifact_from_request(artifact_request: &Bound<'_, PyAny>) -> PyResult<Value> {
    let request = py_any_to_json_value(artifact_request)?;
    let mut request = match request {
        Value::Object(request) => request,
        _ => {
            return Err(CalibrationExecutionError::new_err(
                "preset artifact request must be an object",
            ));
        }
    };

    request
        .remove("artifact")
        .ok_or_else(|| CalibrationExecutionError::new_err("artifact is required"))
}

fn preset_artifact_json_from_request(request: Value) -> PyResult<String> {
    let mut request = match request {
        Value::Object(request) => request,
        _ => {
            return Err(CalibrationExecutionError::new_err(
                "preset artifact request must be an object",
            ));
        }
    };

    match request.remove("payload_json") {
        Some(Value::String(payload_json)) => Ok(payload_json),
        Some(_) => Err(CalibrationExecutionError::new_err(
            "payload_json must be a string",
        )),
        None => Err(CalibrationExecutionError::new_err(
            "payload_json is required",
        )),
    }
}

#[pyfunction]
#[pyo3(signature = (resolution_request))]
fn resolve_preset_workflow_configuration(
    py: Python<'_>,
    resolution_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let resolution_request = py_any_to_json_value(resolution_request)?;

    let resolved_configuration = py
        .detach(|| resolve_preset_workflow_configuration_native(resolution_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    json_value_to_py_object(py, &resolved_configuration)
}

#[pyfunction]
#[pyo3(signature = (resolution_request))]
fn resolve_market_profile_configuration(
    py: Python<'_>,
    resolution_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let resolution_request = py_any_to_json_value(resolution_request)?;

    let resolved_configuration = py
        .detach(|| resolve_market_profile_configuration_native(resolution_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let raw_value = serde_json::to_value(resolved_configuration).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-profile configuration: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

#[pyfunction]
#[pyo3(signature = (exchange_code_request))]
fn exchange_code_from_session_reference(
    py: Python<'_>,
    exchange_code_request: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let exchange_code_request = py_any_to_json_value(exchange_code_request)?;

    py.detach(|| exchange_code_from_session_reference_native(exchange_code_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))
}

#[pyfunction]
#[pyo3(signature = (exchange_code_request))]
fn exchange_code_from_market_profile_reference(
    py: Python<'_>,
    exchange_code_request: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let exchange_code_request = py_any_to_json_value(exchange_code_request)?;

    py.detach(|| exchange_code_from_market_profile_reference_native(exchange_code_request))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))
}

#[pyfunction]
#[pyo3(signature = (calibration_request, calendar_registry))]
fn calibrate_market_data_translation_request(
    py: Python<'_>,
    calibration_request: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
) -> PyResult<Py<PyAny>> {
    let calibration_request: MarketDataTranslationCalibrationRequest =
        serde_json::from_value(py_any_to_json_value(calibration_request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data calibration request: {source}"
            ))
        })?;

    let calibration_output = py
        .detach(|| {
            calibrate_market_data_translation_request_native(
                calibration_request,
                &calendar_registry.registry,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_translation_calibration_output_to_py_object(py, &calibration_output).map_err(
        |source| {
            CalibrationExecutionError::new_err(format!(
                "failed to materialize market-data calibration output: {source}"
            ))
        },
    )
}

#[pyfunction]
#[pyo3(signature = (loading_request))]
fn load_and_calibrate_market_data_translation_request(
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
        .detach(|| load_and_calibrate_market_data_translation_request_native(loading_request))
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
        .detach(|| probe_market_data_source_native(&tick_data_source))
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
        .detach(|| {
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
        .detach(|| {
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
        .detach(|| {
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

fn market_data_translation_calibration_output_to_py_object(
    py: Python<'_>,
    calibration_output: &MarketDataTranslationCalibrationOutput,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(calibration_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data calibration output: {source}"
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
        .detach(|| prepare_market_data_calibration_plan_core(request, cpu_count))
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
    run_staged_calibration_without_python_events(
        py,
        prepared_handle.execution_input.clone(),
        calendar_registry,
        memory_checkpoint_store,
    )
}

fn run_staged_calibration_without_python_events(
    py: Python<'_>,
    execution_input: CalibrationStagedExecutionInput,
    calendar_registry: &PyCalendarRegistry,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let report = run_interruptible_native(
        py,
        NativeInterruptMode::ImportedApi,
        move |cancellation_token| {
            run_staged_calibration_from_execution_input(
                execution_input,
                &calendar_registry.registry,
                cancellation_token,
                memory_checkpoint_store.map(|store| &mut store.store),
            )
            .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))
        },
    )?;

    python_staged_calibration_report_to_py_object(py, &report)
}

fn run_staged_calibration_with_python_events(
    py: Python<'_>,
    execution_input: CalibrationStagedExecutionInput,
    calendar_registry: &PyCalendarRegistry,
    event_callback: &Bound<'_, PyAny>,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let event_callback = event_callback.clone().unbind();
    let callback_error_state = callback_error_state();
    let callback_error_state_for_observer = callback_error_state.clone();

    let (event_sender, event_receiver) = mpsc::channel::<CalibrationStagedRunObserverEvent>();

    let report = run_interruptible_native_with_poll(
        py,
        NativeInterruptMode::ImportedApi,
        move |cancellation_token| {
            let mut observer = |event: CalibrationStagedRunObserverEvent| {
                if callback_error_state_for_observer
                    .lock()
                    .map(|guard| guard.is_some())
                    .unwrap_or(true)
                {
                    return;
                }

                let _ = event_sender.send(event);
            };

            run_staged_calibration_from_execution_input_with_observer(
                execution_input,
                &calendar_registry.registry,
                cancellation_token,
                memory_checkpoint_store.map(|store| &mut store.store),
                &mut observer,
            )
            .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))
        },
        |_: Python<'_>| {
            for event in event_receiver.try_iter() {
                emit_callback_event_payload(&event_callback, &callback_error_state, |py| {
                    python_staged_calibration_event_to_py_object(py, &event)
                });
            }

            Ok(())
        },
    )?;

    raise_callback_error(
        &callback_error_state,
        "staged calibration event callback failed",
        CalibrationExecutionError::new_err,
    )?;

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
    let execution_input = prepared_handle.execution_input.clone();

    run_staged_calibration_with_python_events(
        py,
        execution_input,
        calendar_registry,
        event_callback,
        memory_checkpoint_store,
    )
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

    run_staged_calibration_without_python_events(
        py,
        execution_input,
        calendar_registry,
        memory_checkpoint_store,
    )
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

    let mut execution_input = prepared_handle.execution_input.clone();
    execution_input.request.resume_settings = Some(resume_settings);

    run_staged_calibration_with_python_events(
        py,
        execution_input,
        calendar_registry,
        event_callback,
        memory_checkpoint_store,
    )
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

    run_staged_calibration_without_python_events(
        py,
        execution_input,
        calendar_registry,
        memory_checkpoint_store,
    )
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

    run_staged_calibration_with_python_events(
        py,
        execution_input,
        calendar_registry,
        event_callback,
        memory_checkpoint_store,
    )
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
        .detach(|| {
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
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn estimate_current_state_from_observed_data(
    py: Python<'_>,
    kernel_parameters: &Bound<'_, PyAny>,
    long_variance_parameters: &Bound<'_, PyAny>,
    mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
    state_time_utc_nanos: i64,
    observed_log_return: f64,
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

    let parsed_long_variance_parameters: LongVarianceParameters = serde_json::from_value(
        py_any_to_json_value(long_variance_parameters)?,
    )
    .map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to deserialize current-state long-variance parameters: {source}"
        ))
    })?;

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
            long_variance_parameters: parsed_long_variance_parameters,
            mean_reversion_fitted_parameters: parsed_mean_reversion_fitted_parameters,
            state_time_utc_nanos,
            observed_log_return,
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
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, interval_duration_nanos, clock_model))]
fn materialize_runtime_structural_seasonality(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    clock_model: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;

    let parsed_clock_model = serde_json::from_value(py_any_to_json_value(clock_model)?).map_err(
        |source| {
            CalibrationExecutionError::new_err(format!(
                "runtime seasonality clock_model must match the Fourier business-time clock contract: {source}"
            ))
        },
    )?;

    let parsed_range_start_utc_nanos =
        parsed_range_start_utc.timestamp_nanos_opt().ok_or_else(|| {
            CalibrationExecutionError::new_err(
                "runtime seasonality range_start_utc is outside the supported nanosecond timestamp range",
            )
        })?;

    let parsed_range_end_utc_nanos =
        parsed_range_end_utc.timestamp_nanos_opt().ok_or_else(|| {
            CalibrationExecutionError::new_err(
            "runtime seasonality range_end_utc is outside the supported nanosecond timestamp range",
        )
        })?;

    let materialization = py
        .detach(|| {
            materialize_runtime_structural_seasonality_core(
                &calendar_registry.registry,
                RuntimeStructuralSeasonalityMaterializationInput {
                    exchange_code: exchange_code.to_string(),
                    clock_model: parsed_clock_model,
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

#[pyfunction]
#[pyo3(signature = (range_start_utc, range_end_utc, interval_duration_nanos, surprise_profile))]
fn materialize_surprise_news_weekly_profile_multipliers_for_interval_range(
    py: Python<'_>,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    surprise_profile: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;

    let parsed_surprise_profile = serde_json::from_value(py_any_to_json_value(surprise_profile)?)
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "surprise deterministic intensity profile must match the fitted profile contract: {source}"
            ))
        })?;

    let multipliers = py
        .detach(|| {
            materialize_surprise_news_weekly_profile_multipliers_for_interval_range_core(
                parsed_range_start_utc,
                parsed_range_end_utc,
                interval_duration_nanos,
                &parsed_surprise_profile,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let result = PyDict::new(py);

    for (interval_index, multiplier) in multipliers {
        result.set_item(interval_index, multiplier)?;
    }

    Ok(result.into_any().unbind())
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
        CalibrationStagedRunStageReport::ObservationModels(stage_report) => {
            ("observation_models", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::ReturnComponents(stage_report) => {
            ("return_components", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::LongVariance(stage_report) => {
            ("long_variance", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::NewsShocks(stage_report) => {
            ("news_shocks", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::ReturnBackfit(stage_report) => {
            ("return_backfit", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::MeanReversion(stage_report) => {
            ("mean_reversion", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::JointRefinement(stage_report) => {
            ("joint_refinement", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::OptionRefinement(stage_report) => {
            ("option_refinement", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::Finalization(stage_report) => {
            ("finalization", serde_json::to_value(stage_report)?)
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
        infer_bundled_market_profile_for_tick_data_path,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(infer_market_data_fit_inputs, module)?)?;
    module.add_function(wrap_pyfunction!(built_in_preset_registry_index, module)?)?;
    module.add_function(wrap_pyfunction!(preset_registry_index, module)?)?;
    module.add_function(wrap_pyfunction!(resolve_preset_artifact, module)?)?;
    module.add_function(wrap_pyfunction!(resolve_preset_snapshot, module)?)?;
    module.add_function(wrap_pyfunction!(serialize_preset_artifact, module)?)?;
    module.add_function(wrap_pyfunction!(deserialize_preset_artifact, module)?)?;
    module.add_function(wrap_pyfunction!(normalize_preset_artifact, module)?)?;

    module.add_function(wrap_pyfunction!(
        compute_preset_artifact_content_hash,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        resolve_preset_workflow_configuration,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        resolve_market_profile_configuration,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        exchange_code_from_session_reference,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        exchange_code_from_market_profile_reference,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        calibrate_market_data_translation_request,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        load_and_calibrate_market_data_translation_request,
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

    module.add_function(wrap_pyfunction!(
        materialize_surprise_news_weekly_profile_multipliers_for_interval_range,
        module
    )?)?;

    Ok(())
}
