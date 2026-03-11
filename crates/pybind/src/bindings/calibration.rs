use janiform_core::calibration::{
    CalibrationStagedExecutionInput, CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    MarketDataTranslationPreprocessingOutput, MarketDataTranslationPreprocessingRequest,
    preprocess_market_data_translation_request as preprocess_market_data_translation_request_native,
    run_staged_calibration_from_execution_input,
};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::Value;

use crate::CalibrationExecutionError;
use crate::types::{PyCalendarRegistry, PyCalibrationMemoryCheckpointStore};

#[pyfunction]
#[pyo3(signature = (preprocessing_request_json, calendar_registry))]
fn preprocess_market_data_translation_request(
    py: Python<'_>,
    preprocessing_request_json: &str,
    calendar_registry: &PyCalendarRegistry,
) -> PyResult<String> {
    let preprocessing_request: MarketDataTranslationPreprocessingRequest =
        serde_json::from_str(preprocessing_request_json).map_err(|source| {
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

    serialize_market_data_translation_preprocessing_output(&preprocessing_output).map_err(
        |source| {
            CalibrationExecutionError::new_err(format!(
                "failed to serialize market-data preprocessing output: {source}"
            ))
        },
    )
}

fn serialize_market_data_translation_preprocessing_output(
    preprocessing_output: &MarketDataTranslationPreprocessingOutput,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(preprocessing_output)
}

#[pyfunction]
#[pyo3(signature = (execution_input_json, calendar_registry, memory_checkpoint_store = None))]
fn run_staged_calibration(
    execution_input_json: &str,
    calendar_registry: &PyCalendarRegistry,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<String> {
    let execution_input: CalibrationStagedExecutionInput =
        serde_json::from_str(execution_input_json).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize staged calibration execution input: {source}"
            ))
        })?;

    let report = run_staged_calibration_from_execution_input(
        execution_input,
        &calendar_registry.registry,
        memory_checkpoint_store.map(|store| &mut store.store),
    )
    .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    serialize_python_staged_calibration_report(&report).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize staged calibration report: {source}"
        ))
    })
}

fn serialize_python_staged_calibration_report(
    report: &CalibrationStagedRunReport,
) -> Result<String, serde_json::Error> {
    let mut serialized_report = match serde_json::to_value(report)? {
        Value::Object(object) => object,
        _ => unreachable!("staged calibration report should serialize to an object"),
    };

    let flattened_stage_reports = report
        .stage_reports
        .iter()
        .map(flatten_stage_report_for_python)
        .collect::<Result<Vec<_>, _>>()?;
    serialized_report.insert(
        "stage_reports".to_string(),
        Value::Array(flattened_stage_reports),
    );

    serde_json::to_string(&Value::Object(serialized_report))
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
    };

    let mut flattened_report = match raw_report {
        Value::Object(object) => object,
        _ => unreachable!("stage report should serialize to an object"),
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
    module.add_function(wrap_pyfunction!(run_staged_calibration, module)?)?;
    Ok(())
}
