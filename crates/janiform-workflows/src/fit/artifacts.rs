use std::path::Path;
use std::path::PathBuf;

use janiform_calibration_artifacts::{
    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationStage, CalibrationStageParameterArtifact, CalibrationStageParameterPayload,
    calibration_stage_parameter_artifact_path, load_calibration_stage_parameter_artifact_from_disk,
};
use janiform_calibration_runtime::{
    CalibrationStageExecutionStatus, CalibrationStagedExecutionInput, CalibrationStagedRunReport,
    CalibrationStagedRunStageReport,
};
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use crate::artifact_io::{
    ArtifactWriteKind, write_pretty_json_file as write_workflow_pretty_json_file,
};
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
    let staged_response = build_staged_response_payload(report)?;
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
        "staged_request": execution_input.request,
        "staged_response": staged_response,
    }))
}

/// Build the public staged calibration response payload.
pub fn build_staged_response_payload(
    report: &CalibrationStagedRunReport,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": report.schema_version,
        "failure_triage_policy": report.failure_triage_policy,
        "run_id": report.run_id,
        "execution_mode": report.execution_mode,
        "status": report.status,
        "stage_reports": compact_stage_report_payloads(report)?,
        "emitted_checkpoints": report.emitted_checkpoints,
        "diagnostic_artifact_references": report.diagnostic_artifact_references,
        "resumed_from_checkpoint_reference": report.resumed_from_checkpoint_reference,
        "resumed_from_last_successful_stage": report.resumed_from_last_successful_stage,
        "started_from_stage": report.started_from_stage,
        "failure": report.failure,
    }))
}

/// Build the canonical high-level fitted model payload.
pub fn build_model_payload(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    report_payload: &Value,
    preparation_payload: Option<&Value>,
) -> Result<Value, WorkflowError> {
    let parameters = extract_model_parameters(execution_input, report)?;

    let workflow_scope_id = workflow_scope_id(execution_input);
    let model_preparation_payload =
        preparation_payload.or_else(|| report_payload.get("preparation"));
    let resolved_configuration =
        optional_preparation_field(model_preparation_payload, "resolved_configuration");
    let session_resolution =
        optional_preparation_field(model_preparation_payload, "session_resolution");

    let metadata = build_model_metadata(
        execution_input,
        report,
        report_payload,
        model_preparation_payload,
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

/// Write a canonical fitted model payload artifact.
pub fn write_model_payload_artifact(
    model_payload: &Value,
    path: Option<&Path>,
) -> Result<PathBuf, WorkflowError> {
    let run_id = crate::model::persisted_model_run_id(model_payload)?;
    let model_path = path.map(Path::to_path_buf).unwrap_or_else(|| {
        PathBuf::from(".janiform")
            .join("runs")
            .join(run_id)
            .join("artifacts")
            .join("model.jf")
    });

    write_pretty_json(model_path.as_path(), model_payload)?;

    Ok(model_path)
}

pub(crate) fn write_pretty_json<T>(path: &Path, payload: &T) -> Result<(), WorkflowError>
where
    T: Serialize + ?Sized,
{
    write_workflow_pretty_json_file(path, payload, ArtifactWriteKind::Fit)
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

fn compact_stage_report_payloads(
    report: &CalibrationStagedRunReport,
) -> Result<Vec<Value>, WorkflowError> {
    report
        .stage_reports
        .iter()
        .map(compact_stage_report_payload)
        .collect()
}

fn compact_stage_report_payload(
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<Value, WorkflowError> {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => compact_stage_report_from_parts(
            "seasonality",
            report.stage,
            report.status,
            report.run_id.as_str(),
            report.execution_mode,
            report.handoff_policy,
            &report.failure,
        ),
        CalibrationStagedRunStageReport::Hawkes(report) => compact_stage_report_from_parts(
            "hawkes",
            report.stage,
            report.status,
            report.run_id.as_str(),
            report.execution_mode,
            report.handoff_policy,
            &report.failure,
        ),
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            compact_stage_report_from_parts(
                "return_components",
                report.stage,
                report.status,
                report.run_id.as_str(),
                report.execution_mode,
                report.handoff_policy,
                &report.failure,
            )
        }
        CalibrationStagedRunStageReport::MeanReversion(report) => compact_stage_report_from_parts(
            "mean_reversion",
            report.stage,
            report.status,
            report.run_id.as_str(),
            report.execution_mode,
            report.handoff_policy,
            &report.failure,
        ),
        CalibrationStagedRunStageReport::JointRefinement(report) => {
            compact_stage_report_from_parts(
                "joint_refinement",
                report.stage,
                report.status,
                report.run_id.as_str(),
                report.execution_mode,
                report.handoff_policy,
                &report.failure,
            )
        }
    }
}

fn compact_stage_report_from_parts<T>(
    stage_report_kind: &str,
    stage: CalibrationStage,
    status: CalibrationStageExecutionStatus,
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    handoff_policy: &str,
    failure: &Option<T>,
) -> Result<Value, WorkflowError>
where
    T: Serialize,
{
    let failure = match failure {
        Some(failure) => serialize_payload(failure)?,
        None => Value::Null,
    };

    Ok(json!({
        "stage_report_kind": stage_report_kind,
        "stage": stage,
        "status": status,
        "run_id": run_id,
        "execution_mode": execution_mode,
        "handoff_policy": handoff_policy,
        "failure": failure,
    }))
}

fn extract_model_parameters(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
) -> Result<Value, WorkflowError> {
    let mut stage_parameters = ModelStageParameters::default();

    apply_resumed_disk_stage_parameter_artifacts(execution_input, report, &mut stage_parameters)?;
    apply_stage_reports_to_model_parameters(report, &mut stage_parameters)?;

    Ok(Value::Object(stage_parameters.into_model_parameters()))
}

fn apply_resumed_disk_stage_parameter_artifacts(
    execution_input: &CalibrationStagedExecutionInput,
    report: &CalibrationStagedRunReport,
    stage_parameters: &mut ModelStageParameters,
) -> Result<(), WorkflowError> {
    if execution_input.request.execution_mode != CalibrationExecutionMode::Disk
        || execution_input.request.resume_settings.is_none()
    {
        return Ok(());
    }

    let Some(runtime_resources) = &execution_input.runtime_resources else {
        return Ok(());
    };
    let Some(disk_checkpoint_directory) = &runtime_resources.disk_checkpoint_directory else {
        return Ok(());
    };

    let disk_checkpoint_directory = Path::new(disk_checkpoint_directory);

    for stage in execution_input.request.selected_stages.iter().copied() {
        if report
            .stage_reports
            .iter()
            .any(|stage_report| stage_report_stage(stage_report) == stage)
        {
            continue;
        }

        let artifact_path = calibration_stage_parameter_artifact_path(
            disk_checkpoint_directory,
            execution_input.request.run_id.as_str(),
            stage,
        );

        let artifact = load_calibration_stage_parameter_artifact_from_disk(artifact_path.as_path())
            .map_err(|source| WorkflowError::ModelValidationFailed {
                field: "model_payload.parameters",
                message: format!(
                    "failed to load resumed stage parameter artifact `{}`: {source}",
                    artifact_path.display()
                ),
            })?;

        apply_stage_parameter_artifact_to_model_parameters(&artifact, stage_parameters)?;
    }

    Ok(())
}

fn apply_stage_reports_to_model_parameters(
    report: &CalibrationStagedRunReport,
    stage_parameters: &mut ModelStageParameters,
) -> Result<(), WorkflowError> {
    for stage_report in &report.stage_reports {
        match stage_report {
            CalibrationStagedRunStageReport::Seasonality(report) => {
                if let Some(payload) = &report.normalized_stage_payload {
                    stage_parameters.seasonality = Some(seasonality_parameters(payload));
                }
            }
            CalibrationStagedRunStageReport::Hawkes(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    stage_parameters.hawkes = Some(hawkes_parameters(payload)?);
                }
            }
            CalibrationStagedRunStageReport::ReturnComponents(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    stage_parameters.return_components =
                        Some(return_components_parameters(payload)?);
                }
            }
            CalibrationStagedRunStageReport::MeanReversion(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    stage_parameters.mean_reversion = Some(serialize_payload(payload)?);
                }
            }
            CalibrationStagedRunStageReport::JointRefinement(report) => {
                if let Some(payload) = &report.fitted_stage_payload {
                    stage_parameters.joint_refinement = Some(joint_refinement_parameters(payload)?);
                }
            }
        }
    }

    Ok(())
}

fn stage_report_stage(stage_report: &CalibrationStagedRunStageReport) -> CalibrationStage {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(_) => CalibrationStage::Seasonality,
        CalibrationStagedRunStageReport::Hawkes(_) => CalibrationStage::Hawkes,
        CalibrationStagedRunStageReport::ReturnComponents(_) => CalibrationStage::ReturnComponents,
        CalibrationStagedRunStageReport::MeanReversion(_) => CalibrationStage::MeanReversion,
        CalibrationStagedRunStageReport::JointRefinement(_) => CalibrationStage::JointRefinement,
    }
}

fn apply_stage_parameter_artifact_to_model_parameters(
    artifact: &CalibrationStageParameterArtifact,
    stage_parameters: &mut ModelStageParameters,
) -> Result<(), WorkflowError> {
    match &artifact.stage_parameters {
        CalibrationStageParameterPayload::Seasonality(payload) => {
            stage_parameters.seasonality = Some(seasonality_parameters_from_artifact(payload));
        }
        CalibrationStageParameterPayload::Hawkes(payload) => {
            stage_parameters.hawkes = Some(hawkes_parameters_from_artifact(payload)?);
        }
        CalibrationStageParameterPayload::ReturnComponents(payload) => {
            stage_parameters.return_components =
                Some(return_components_parameters_from_artifact(payload)?);
        }
        CalibrationStageParameterPayload::MeanReversion(payload) => {
            stage_parameters.mean_reversion =
                Some(mean_reversion_parameters_from_artifact(payload)?);
        }
        CalibrationStageParameterPayload::JointRefinement(payload) => {
            stage_parameters.joint_refinement =
                Some(joint_refinement_parameters_from_artifact(payload)?);
        }
    }

    Ok(())
}

#[derive(Default)]
struct ModelStageParameters {
    seasonality: Option<Value>,
    hawkes: Option<Value>,
    return_components: Option<Value>,
    mean_reversion: Option<Value>,
    joint_refinement: Option<Value>,
}

impl ModelStageParameters {
    fn into_model_parameters(self) -> Map<String, Value> {
        let mut parameters = Map::new();

        if let Some(seasonality) = self.seasonality {
            parameters.insert("seasonality".to_string(), seasonality);
        }

        if let Some(joint_refinement) = self.joint_refinement {
            parameters.insert("joint_refinement".to_string(), joint_refinement);

            return parameters;
        }

        if let Some(hawkes) = self.hawkes {
            parameters.insert("hawkes".to_string(), hawkes);
        }

        if let Some(return_components) = self.return_components {
            parameters.insert("return_components".to_string(), return_components);
        }

        if let Some(mean_reversion) = self.mean_reversion {
            parameters.insert("mean_reversion".to_string(), mean_reversion);
        }

        parameters
    }
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

fn seasonality_parameters_from_artifact(
    payload: &janiform_calibration_artifacts::SeasonalityStageParameterPayload,
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

fn hawkes_parameters(
    payload: &janiform_calibration_runtime::HawkesStageFittedPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": payload.schema_version,
        "selected_candidate_id": payload.selected_candidate_id,
        "fitted_parameters": serialize_payload(&payload.fitted_parameters)?,
        "seasonality_input_transport": payload.seasonality_input_transport,
        "seasonality_dataset_id": payload.seasonality_dataset_id,
        "seasonality_artifact_reference": payload.seasonality_artifact_reference,
    }))
}

fn hawkes_parameters_from_artifact(
    payload: &janiform_calibration_artifacts::HawkesStageParameterPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
        "selected_candidate_id": payload.selected_candidate_id,
        "fitted_parameters": serialize_payload(&payload.fitted_parameters)?,
        "seasonality_input_transport": payload.seasonality_input_transport,
        "seasonality_dataset_id": payload.seasonality_dataset_id,
        "seasonality_artifact_reference": payload.seasonality_artifact_reference,
    }))
}

fn return_components_parameters(
    payload: &janiform_calibration_runtime::ReturnComponentsStageFittedPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": payload.schema_version,
        "selected_candidate_id": payload.selected_candidate_id,
        "fitted_parameters": serialize_payload(&payload.fitted_parameters)?,
        "candidate_generation_policy": payload.candidate_generation_policy,
        "objective_weighting_policy": payload.objective_weighting_policy,
        "objective_metric_bucket_policy": payload.objective_metric_bucket_policy,
        "candidate_family_counts": serialize_payload(&payload.candidate_family_counts)?,
        "activity_input_transport": payload.activity_input_transport,
        "activity_selected_hawkes_candidate_id": payload.activity_selected_hawkes_candidate_id,
        "activity_artifact_reference": payload.activity_artifact_reference,
    }))
}

fn return_components_parameters_from_artifact(
    payload: &janiform_calibration_artifacts::ReturnComponentsStageParameterPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
        "selected_candidate_id": payload.selected_candidate_id,
        "fitted_parameters": serialize_payload(&payload.fitted_parameters)?,
        "candidate_generation_policy": payload.candidate_generation_policy,
        "objective_weighting_policy": payload.objective_weighting_policy,
        "objective_metric_bucket_policy": payload.objective_metric_bucket_policy,
        "activity_input_transport": payload.activity_input_transport,
        "activity_selected_hawkes_candidate_id": payload.activity_selected_hawkes_candidate_id,
        "activity_artifact_reference": payload.activity_artifact_reference,
    }))
}

fn mean_reversion_parameters_from_artifact(
    payload: &janiform_calibration_artifacts::MeanReversionStageParameterPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
        "fitted_parameters": serialize_payload(&payload.fitted_parameters)?,
        "diagnostics": serialize_payload(&payload.diagnostics)?,
        "return_components_input_transport": payload.return_components_input_transport,
        "return_components_selected_candidate_id": payload.return_components_selected_candidate_id,
        "return_components_artifact_reference": payload.return_components_artifact_reference,
    }))
}

fn joint_refinement_parameters(
    payload: &janiform_calibration_runtime::JointRefinementStageFittedPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": payload.schema_version,
        "selected_candidate_id": payload.selected_candidate_id,
        "fitted_parameters": serialize_payload(&payload.fitted_parameters)?,
        "objective_breakdown": serialize_payload(&payload.objective_breakdown)?,
        "optimizer_diagnostics": serialize_payload(&payload.optimizer_diagnostics)?,
        "option_calibration_diagnostics": serialize_payload(&payload.option_calibration_diagnostics)?,
    }))
}

fn joint_refinement_parameters_from_artifact(
    payload: &janiform_calibration_artifacts::JointRefinementStageParameterPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        "selected_candidate_id": payload.selected_candidate_id,
        "fitted_parameters": serialize_payload(&payload.fitted_parameters)?,
        "objective_breakdown": serialize_payload(&payload.objective_breakdown)?,
        "optimizer_diagnostics": serialize_payload(&payload.optimizer_diagnostics)?,
    }))
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
    let mut selected = Map::new();

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
