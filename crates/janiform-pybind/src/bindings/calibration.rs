use janiform::calibration::{
    CalibrationReturnComponentsActivityContextPayload,
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationStagedExecutionInput,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    MarketDataTranslationPreprocessingOutput, MarketDataTranslationPreprocessingRequest,
    MarketDataLoadingOutput, MarketDataLoadingRequest,
    ReturnComponentsStageFittedParameters, ReturnComponentsStageObservedSignalSummaryStatistics,
    evaluate_return_components_exact_candidate_from_bridge,
    load_and_preprocess_market_data_translation_request as load_and_preprocess_market_data_translation_request_native,
    preprocess_market_data_translation_request as preprocess_market_data_translation_request_native,
    run_staged_calibration_from_execution_input,
};
use pyo3::prelude::*;
use pyo3::types::PyAny;
use pyo3::types::PyModule;
use serde_json::Value;

use crate::CalibrationExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::{PyCalendarRegistry, PyCalibrationMemoryCheckpointStore};

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
#[pyo3(signature = (loading_request, calendar_registry))]
fn load_and_preprocess_market_data_translation_request(
    py: Python<'_>,
    loading_request: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
) -> PyResult<Py<PyAny>> {
    let loading_request: MarketDataLoadingRequest =
        serde_json::from_value(py_any_to_json_value(loading_request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data loading request: {source}"
            ))
        })?;

    let loading_output = py
        .allow_threads(|| {
            load_and_preprocess_market_data_translation_request_native(
                loading_request,
                &calendar_registry.registry,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_loading_output_to_py_object(py, &loading_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to materialize market-data loading output: {source}"
        ))
    })
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
    module.add_function(wrap_pyfunction!(run_staged_calibration, module)?)?;
    module.add_function(wrap_pyfunction!(
        evaluate_return_components_candidate,
        module
    )?)?;
    Ok(())
}
