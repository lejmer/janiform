use janiform_calibration_artifacts::{
    CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
};
use serde_json::Value;
use serde_json::json;

use crate::error::WorkflowError;

use super::serialize_payload;

pub(super) fn return_components_parameters_from_backfit_artifact(
    payload: &janiform_calibration_artifacts::ReturnBackfitStageParameterPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
        "selected_candidate_id": payload.effective_return_components_selected_candidate_id,
        "fitted_parameters": serialize_payload(&payload.effective_fitted_parameters)?,
        "candidate_generation_policy": payload.candidate_generation_policy,
        "objective_weighting_policy": payload.objective_weighting_policy,
        "objective_metric_bucket_policy": payload.objective_metric_bucket_policy,
    }))
}

pub(super) fn return_backfit_parameters(
    payload: &janiform_calibration_runtime::ReturnBackfitStageFittedPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": payload.schema_version,
        "selected_candidate_id": payload.selected_candidate_id,
        "effective_return_components_selected_candidate_id": payload
            .effective_return_components_payload
            .selected_candidate_id,
        "diagnostics": serialize_payload(&payload.diagnostics)?,
        "return_components_input_transport": payload.return_components_input_transport,
        "return_components_selected_candidate_id": payload.return_components_selected_candidate_id,
        "return_components_artifact_reference": payload.return_components_artifact_reference,
    }))
}

pub(super) fn return_backfit_parameters_from_artifact(
    payload: &janiform_calibration_artifacts::ReturnBackfitStageParameterPayload,
) -> Result<Value, WorkflowError> {
    Ok(json!({
        "schema_version": CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
        "selected_candidate_id": payload.selected_candidate_id,
        "effective_return_components_selected_candidate_id": payload
            .effective_return_components_selected_candidate_id,
        "diagnostics": serialize_payload(&payload.diagnostics)?,
        "return_components_input_transport": payload.return_components_input_transport,
        "return_components_selected_candidate_id": payload.return_components_selected_candidate_id,
        "return_components_artifact_reference": payload.return_components_artifact_reference,
    }))
}
