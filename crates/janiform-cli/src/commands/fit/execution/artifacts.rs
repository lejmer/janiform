use std::path::Path;

use janiform::calibration::{
    CalibrationStageExecutionStatus, CalibrationStagedExecutionInput, CalibrationStagedRunReport,
    CalibrationStagedRunStageReport,
};
use serde_json::Value;
use serde_json::json;

use super::scope::{WorkflowScopeId, workflow_scope_id};
use crate::commands::shared::output::write_pretty_json_file;
use crate::error::CliError;

const FIT_REPORT_SCHEMA_VERSION: &str = "janiform.fit_report.v1";
const MODEL_PERSISTENCE_SCHEMA_VERSION: &str = "calibration.high_level.model.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CalibrationScopeId {
    SeasonalityOnly,
    UnderlyingOnly,
    JointOptionCalibrated,
}

impl CalibrationScopeId {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SeasonalityOnly => "seasonality_only",
            Self::UnderlyingOnly => "underlying_only",
            Self::JointOptionCalibrated => "joint_option_calibrated",
        }
    }
}

pub(super) fn build_fit_report_payload(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    preparation_payload: Option<Value>,
    staged_execution_input_path: Option<&Path>,
) -> Result<Value, CliError> {
    let stage_report_payloads = stage_report_payloads(report)?;
    let workflow_scope_id = workflow_scope_id(execution_input);

    Ok(json!({
        "schema_version": FIT_REPORT_SCHEMA_VERSION,
        "run_id": execution_input.request.run_id,
        "workflow_scope": workflow_scope_id.as_str(),
        "success": report.status == CalibrationStageExecutionStatus::Success,
        "summary": {
            "run_id": execution_input.request.run_id,
            "workflow_scope": workflow_scope_id.as_str(),
            "success": report.status == CalibrationStageExecutionStatus::Success,
            "selected_stages": execution_input.request.selected_stages,
            "stage_statuses": stage_statuses(report),
            "diagnostic_count": report.diagnostic_artifact_references.len(),
        },
        "preparation": preparation_payload,
        "prepared_data_reference": execution_input.request.data_inputs.trade_events_data_reference,
        "prepared_payload_path": null,
        "staged_execution_input_path": staged_execution_input_path.map(|path| path.display().to_string()),
        "staged_request": execution_input,
        "staged_response": report,
        "stage_report_payloads": stage_report_payloads,
    }))
}

pub(super) fn build_model_payload(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    report_payload: &Value,
) -> Result<Value, CliError> {
    let parameters = extract_model_parameters(report)?;
    let workflow_scope_id = workflow_scope_id(execution_input);

    Ok(json!({
        "schema_version": MODEL_PERSISTENCE_SCHEMA_VERSION,
        "run_id": execution_input.request.run_id,
        "workflow_scope": workflow_scope_id.as_str(),
        "parameters": parameters,
        "metadata": {
            "run_id": execution_input.request.run_id,
            "workflow_scope": workflow_scope_id.as_str(),
            "calibration_scope": calibration_scope_id(workflow_scope_id).as_str(),
            "execution_mode": execution_input.request.execution_mode,
            "selected_stages": execution_input.request.selected_stages,
            "fitted_parameter_summary": {
                "stage_keys": model_parameter_stage_keys(&parameters),
                "selected_candidate_ids": selected_candidate_ids(report),
            },
            "diagnostic_artifacts": {
                "artifact_bundle_available": report.diagnostic_artifact_bundle.is_some(),
                "artifact_families": report.diagnostic_artifact_references.iter().map(|reference| reference.artifact_family.clone()).collect::<Vec<_>>(),
            },
            "fit_report_summary": report_payload["summary"].clone(),
        },
        "resolved_configuration": {},
        "session_resolution": {},
    }))
}

pub(super) fn write_pretty_json(path: &Path, payload: &Value) -> Result<(), CliError> {
    write_pretty_json_file(path, payload, |path, source| CliError::FitArtifactWrite {
        path,
        source,
    })
}

fn calibration_scope_id(workflow_scope_id: WorkflowScopeId) -> CalibrationScopeId {
    match workflow_scope_id {
        WorkflowScopeId::SeasonalityOnly => CalibrationScopeId::SeasonalityOnly,
        WorkflowScopeId::FullWithoutOptionSurface => CalibrationScopeId::UnderlyingOnly,
        WorkflowScopeId::FullWithOptionSurface => CalibrationScopeId::JointOptionCalibrated,
    }
}

fn stage_report_payloads(report: &CalibrationStagedRunReport) -> Result<Vec<Value>, CliError> {
    report
        .stage_reports
        .iter()
        .map(|stage_report| {
            serde_json::to_value(stage_report)
                .map_err(|source| CliError::OutputSerialize { source })
        })
        .collect()
}

fn extract_model_parameters(report: &CalibrationStagedRunReport) -> Result<Value, CliError> {
    let mut parameters = serde_json::Map::new();

    for stage_report in &report.stage_reports {
        match stage_report {
            CalibrationStagedRunStageReport::Seasonality(report) => {
                if let Some(payload) = &report.normalized_stage_payload {
                    parameters.insert("seasonality".to_string(), seasonality_parameters(payload));
                }
            }
            CalibrationStagedRunStageReport::Hawkes(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    parameters.insert("hawkes".to_string(), serialize_payload(payload)?);
                }
            }
            CalibrationStagedRunStageReport::ReturnComponents(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    parameters.insert("return_components".to_string(), serialize_payload(payload)?);
                }
            }
            CalibrationStagedRunStageReport::MeanReversion(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    parameters.insert("mean_reversion".to_string(), serialize_payload(payload)?);
                }
            }
            CalibrationStagedRunStageReport::JointRefinement(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    parameters.insert("joint_refinement".to_string(), serialize_payload(payload)?);
                }
            }
        }
    }

    Ok(Value::Object(parameters))
}

fn seasonality_parameters(
    payload: &janiform::calibration::SeasonalityStageNormalizedPayload,
) -> Value {
    json!({
        "schema_version": payload.composition_schema_version,
        "exchange_code": payload.exchange_code,
        "intraday_minutes_per_bin": payload.intraday_minutes_per_bin,
        "intraday_factor_by_bin": payload.intraday_factor_by_bin,
        "day_of_week_factor": payload.day_of_week_factor,
        "day_of_year_factor": payload.day_of_year_factor,
    })
}

fn serialize_payload<T>(payload: &T) -> Result<Value, CliError>
where
    T: serde::Serialize,
{
    serde_json::to_value(payload).map_err(|source| CliError::OutputSerialize { source })
}

fn stage_statuses(report: &CalibrationStagedRunReport) -> Vec<Value> {
    report
        .stage_reports
        .iter()
        .map(|stage_report| match stage_report {
            CalibrationStagedRunStageReport::Seasonality(report) => {
                json!({"stage": "seasonality", "status": report.status})
            }
            CalibrationStagedRunStageReport::Hawkes(report) => {
                json!({"stage": "hawkes", "status": report.status})
            }
            CalibrationStagedRunStageReport::ReturnComponents(report) => {
                json!({"stage": "return_components", "status": report.status})
            }
            CalibrationStagedRunStageReport::MeanReversion(report) => {
                json!({"stage": "mean_reversion", "status": report.status})
            }
            CalibrationStagedRunStageReport::JointRefinement(report) => {
                json!({"stage": "joint_refinement", "status": report.status})
            }
        })
        .collect()
}

fn model_parameter_stage_keys(parameters: &Value) -> Vec<String> {
    let Some(parameters) = parameters.as_object() else {
        return Vec::new();
    };

    let mut keys: Vec<String> = parameters.keys().cloned().collect();
    keys.sort();

    keys
}

fn selected_candidate_ids(report: &CalibrationStagedRunReport) -> Value {
    let mut selected = serde_json::Map::new();

    for stage_report in &report.stage_reports {
        match stage_report {
            CalibrationStagedRunStageReport::Hawkes(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    selected.insert(
                        "hawkes".to_string(),
                        Value::String(payload.selected_candidate_id.clone()),
                    );
                }
            }
            CalibrationStagedRunStageReport::ReturnComponents(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    selected.insert(
                        "return_components".to_string(),
                        Value::String(payload.selected_candidate_id.clone()),
                    );
                }
            }
            CalibrationStagedRunStageReport::JointRefinement(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    selected.insert(
                        "joint_refinement".to_string(),
                        Value::String(payload.selected_candidate_id.clone()),
                    );
                }
            }
            CalibrationStagedRunStageReport::Seasonality(_)
            | CalibrationStagedRunStageReport::MeanReversion(_) => {}
        }
    }

    Value::Object(selected)
}
