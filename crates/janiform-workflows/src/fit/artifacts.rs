use std::fs;
use std::io::Write;
use std::path::Path;

use janiform_calibration_runtime::{
    CalibrationStageExecutionStatus, CalibrationStagedExecutionInput, CalibrationStagedRunReport,
    CalibrationStagedRunStageReport,
};
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use crate::error::WorkflowError;

use super::scope::{WorkflowScopeId, workflow_scope_id};

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

/// Build the canonical fit report artifact payload.
pub fn build_fit_report_payload(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    preparation_payload: Option<&Value>,
    staged_execution_input_path: Option<&Path>,
) -> Result<Value, WorkflowError> {
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
        "preparation": preparation_payload.cloned(),
        "prepared_data_reference": execution_input.request.data_inputs.trade_events_data_reference,
        "prepared_payload_path": null,
        "staged_execution_input_path": staged_execution_input_path.map(|path| path.display().to_string()),
        "staged_request": execution_input,
        "staged_response": report,
        "stage_report_payloads": stage_report_payloads,
    }))
}

/// Build the canonical high-level fitted model payload.
pub fn build_model_payload(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    report_payload: &Value,
    preparation_payload: Option<&Value>,
) -> Result<Value, WorkflowError> {
    let parameters = extract_model_parameters(report)?;
    let workflow_scope_id = workflow_scope_id(execution_input);
    let resolved_configuration =
        optional_preparation_field(preparation_payload, "resolved_configuration");
    let session_resolution = optional_preparation_field(preparation_payload, "session_resolution");
    let metadata = build_model_metadata(
        execution_input,
        report,
        report_payload,
        preparation_payload,
        &parameters,
    );

    Ok(json!({
        "schema_version": MODEL_PERSISTENCE_SCHEMA_VERSION,
        "run_id": execution_input.request.run_id,
        "workflow_scope": workflow_scope_id.as_str(),
        "parameters": parameters,
        "metadata": metadata,
        "resolved_configuration": resolved_configuration,
        "session_resolution": session_resolution,
    }))
}

pub(crate) fn write_pretty_json<T>(path: &Path, payload: &T) -> Result<(), WorkflowError>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| WorkflowError::FitArtifactWrite {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let mut file = fs::File::create(path).map_err(|source| WorkflowError::FitArtifactWrite {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::to_writer_pretty(&mut file, payload)
        .map_err(|source| WorkflowError::OutputSerialize { source })?;

    writeln!(file).map_err(|source| WorkflowError::FitArtifactWrite {
        path: path.to_path_buf(),
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

fn optional_preparation_field(preparation_payload: Option<&Value>, field_name: &str) -> Value {
    preparation_payload
        .and_then(|payload| payload.get(field_name))
        .cloned()
        .unwrap_or(Value::Null)
}

fn build_model_metadata(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    report_payload: &Value,
    preparation_payload: Option<&Value>,
    parameters: &Value,
) -> Value {
    let workflow_scope_id = workflow_scope_id(execution_input);
    let resolved_configuration = preparation_field(preparation_payload, "resolved_configuration");
    let session_resolution = preparation_field(preparation_payload, "session_resolution");

    let market_profile = resolved_configuration.and_then(market_profile_name);
    let session = resolved_configuration.and_then(|configuration| configuration.get("session"));
    let known_news =
        resolved_configuration.and_then(|configuration| configuration.get("known_news"));
    let known_news_preset = known_news.and_then(selected_preset_name);
    let known_news_event_count = known_news_event_count(known_news);

    json!({
        "run_id": execution_input.request.run_id,
        "workflow_scope": workflow_scope_id.as_str(),
        "calibration_scope": calibration_scope_id(workflow_scope_id).as_str(),
        "execution_mode": execution_input.request.execution_mode,
        "selected_stages": execution_input.request.selected_stages,
        "market_profile": market_profile,
        "session_preset": session.and_then(selected_preset_name),
        "known_news_preset": known_news_preset,
        "session_source_kind": session.and_then(source_kind),
        "known_news_source_kind": known_news.and_then(source_kind),
        "fitted_parameter_summary": {
            "stage_keys": model_parameter_stage_keys(parameters),
            "selected_candidate_ids": selected_candidate_ids(report),
        },
        "preset_provenance": {
            "market_profile": resolved_configuration.and_then(|configuration| {
                configuration.get("market_profile").map(component_provenance)
            }),
            "session": session.map(session_provenance),
            "known_news": known_news.map(known_news_provenance),
        },
        "session_resolution": session_resolution.map(session_resolution_summary),
        "known_news_activation": {
            "selected_preset": known_news_preset.unwrap_or("none"),
            "event_count": known_news_event_count,
            "active": known_news_event_count > 0,
        },
        "simulation_output_preferences": {
            "reference_price": optional_preparation_field(preparation_payload, "reference_price"),
        },
        "diagnostic_artifacts": {
            "artifact_bundle_available": report.diagnostic_artifact_bundle.is_some(),
            "artifact_families": report.diagnostic_artifact_references.iter().map(|reference| reference.artifact_family.clone()).collect::<Vec<_>>(),
        },
        "input_inference": optional_preparation_field(preparation_payload, "input_inference"),
        "diagnostics": resolved_configuration
            .and_then(|configuration| configuration.get("diagnostics"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "fit_report_summary": report_payload["summary"].clone(),
    })
}

fn preparation_field<'a>(
    preparation_payload: Option<&'a Value>,
    field_name: &str,
) -> Option<&'a Value> {
    preparation_payload.and_then(|payload| payload.get(field_name))
}

fn market_profile_name(resolved_configuration: &Value) -> Option<&str> {
    let market_profile = resolved_configuration.get("market_profile")?;

    selected_preset_name(market_profile).or_else(|| {
        market_profile
            .get("configuration")
            .and_then(|configuration| configuration.get("profile_name"))
            .and_then(Value::as_str)
    })
}

fn selected_preset_name(component: &Value) -> Option<&str> {
    component
        .get("selected_preset")
        .and_then(|selection| selection.get("preset_name"))
        .and_then(Value::as_str)
}

fn source_kind(component: &Value) -> Option<&str> {
    component.get("source_kind").and_then(Value::as_str)
}

fn known_news_event_count(known_news: Option<&Value>) -> usize {
    known_news
        .and_then(|component| component.get("configuration"))
        .and_then(|configuration| configuration.get("events"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn component_provenance(component: &Value) -> Value {
    Value::Object(base_component_provenance(component))
}

fn session_provenance(component: &Value) -> Value {
    let mut provenance = base_component_provenance(component);
    provenance.insert(
        "inferred_from_data".to_string(),
        component
            .get("inferred_from_data")
            .cloned()
            .unwrap_or(Value::Bool(false)),
    );

    Value::Object(provenance)
}

fn known_news_provenance(component: &Value) -> Value {
    let mut provenance = base_component_provenance(component);
    provenance.insert(
        "recommended_preset".to_string(),
        component
            .get("recommended_reference")
            .and_then(|reference| reference.get("preset_name"))
            .cloned()
            .unwrap_or(Value::Null),
    );
    provenance.insert(
        "recommendation_policy".to_string(),
        component
            .get("recommendation_policy")
            .cloned()
            .unwrap_or(Value::Null),
    );
    provenance.insert(
        "recommendation_applied".to_string(),
        component
            .get("recommendation_applied")
            .cloned()
            .unwrap_or(Value::Bool(false)),
    );

    Value::Object(provenance)
}

fn base_component_provenance(component: &Value) -> Map<String, Value> {
    let mut provenance = Map::new();
    provenance.insert(
        "source_kind".to_string(),
        source_kind(component)
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    provenance.insert(
        "selected_preset".to_string(),
        component
            .get("selected_preset")
            .cloned()
            .unwrap_or(Value::Null),
    );
    provenance.insert(
        "applied_override_fields".to_string(),
        component
            .get("applied_override_fields")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    );

    provenance
}

fn session_resolution_summary(session_resolution: &Value) -> Value {
    json!({
        "resolution_kind": session_resolution.get("resolution_kind").cloned().unwrap_or(Value::Null),
        "provenance_kind": session_resolution.get("provenance_kind").cloned().unwrap_or(Value::Null),
        "selected_policy_id": session_resolution.get("selected_policy_id").cloned().unwrap_or(Value::Null),
        "exchange_code": session_resolution.get("exchange_code").cloned().unwrap_or(Value::Null),
        "exchange_name": session_resolution.get("exchange_name").cloned().unwrap_or(Value::Null),
        "resolved_timezone_name": session_resolution.get("resolved_timezone_name").cloned().unwrap_or(Value::Null),
        "confidence_score": session_resolution.get("confidence_score").cloned().unwrap_or(Value::Null),
        "confidence_label": session_resolution.get("confidence_label").cloned().unwrap_or(Value::Null),
        "manual_override_applied": session_resolution.get("manual_override_applied").cloned().unwrap_or(Value::Null),
    })
}

fn stage_report_payloads(report: &CalibrationStagedRunReport) -> Result<Vec<Value>, WorkflowError> {
    report
        .stage_reports
        .iter()
        .map(|stage_report| {
            serde_json::to_value(stage_report)
                .map_err(|source| WorkflowError::OutputSerialize { source })
        })
        .collect()
}

fn extract_model_parameters(report: &CalibrationStagedRunReport) -> Result<Value, WorkflowError> {
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
    payload: &janiform_calibration_runtime::SeasonalityStageNormalizedPayload,
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

fn serialize_payload<T>(payload: &T) -> Result<Value, WorkflowError>
where
    T: Serialize,
{
    serde_json::to_value(payload).map_err(|source| WorkflowError::OutputSerialize { source })
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
                        Value::String(payload.selected_candidate_id.to_owned()),
                    );
                }
            }
            CalibrationStagedRunStageReport::ReturnComponents(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    selected.insert(
                        "return_components".to_string(),
                        Value::String(payload.selected_candidate_id.to_owned()),
                    );
                }
            }
            CalibrationStagedRunStageReport::JointRefinement(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    selected.insert(
                        "joint_refinement".to_string(),
                        Value::String(payload.selected_candidate_id.to_owned()),
                    );
                }
            }
            CalibrationStagedRunStageReport::Seasonality(_)
            | CalibrationStagedRunStageReport::MeanReversion(_) => {}
        }
    }

    Value::Object(selected)
}
