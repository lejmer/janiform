use serde::{Deserialize, Serialize};

use janiform_foundations::numeric::validate_positive_number;

use super::super::super::super::contracts::{
    CalibrationExecutionMode, CalibrationStage, CalibrationStageHandoffTransport,
};
use super::super::super::super::{
    ReturnBackfitStageDiagnostics, ReturnComponentsStageFittedParameters,
};
use super::artifact::{CalibrationStageParameterArtifact, build_stage_parameter_artifact};
use super::errors::{
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactValidationError,
};
use super::input::CalibrationStageParameterArtifactInput;
use super::metadata::CalibrationStageParameterBuildMetadata;
use super::optimizer::CalibrationStageParameterOptimizerState;
use super::payload::CalibrationStageParameterPayload;
use super::support::{
    validate_non_empty_artifact_value, validate_optimizer_state_absence,
    validate_return_components_input_boundary,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnBackfitStageParameterPayload {
    pub selected_candidate_id: String,
    pub effective_return_components_selected_candidate_id: String,
    pub source_window_open_exposure_hours: f64,
    pub effective_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_generation_policy: String,
    pub objective_weighting_policy: String,
    pub objective_metric_bucket_policy: String,
    pub diagnostics: ReturnBackfitStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnBackfitStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub selected_candidate_id: String,
    pub effective_return_components_selected_candidate_id: String,
    pub source_window_open_exposure_hours: f64,
    pub effective_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_generation_policy: String,
    pub objective_weighting_policy: String,
    pub objective_metric_bucket_policy: String,
    pub diagnostics: ReturnBackfitStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    pub return_components_selected_candidate_id: Option<String>,
    pub return_components_artifact_reference: Option<String>,
}

impl CalibrationStageParameterArtifactInput for ReturnBackfitStageParameterArtifactInput {
    fn stage(&self) -> CalibrationStage {
        CalibrationStage::ReturnBackfit
    }

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
    {
        build_return_backfit_stage_parameter_artifact(
            run_id,
            execution_mode,
            *self,
            artifact_reference,
        )
    }
}

pub(super) fn build_return_backfit_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: ReturnBackfitStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let metadata = input.metadata;

    let payload = ReturnBackfitStageParameterPayload {
        selected_candidate_id: input.selected_candidate_id,
        effective_return_components_selected_candidate_id: input
            .effective_return_components_selected_candidate_id,
        source_window_open_exposure_hours: input.source_window_open_exposure_hours,
        effective_fitted_parameters: input.effective_fitted_parameters,
        candidate_generation_policy: input.candidate_generation_policy,
        objective_weighting_policy: input.objective_weighting_policy,
        objective_metric_bucket_policy: input.objective_metric_bucket_policy,
        diagnostics: input.diagnostics,
        return_components_input_transport: input.return_components_input_transport,
        return_components_selected_candidate_id: input.return_components_selected_candidate_id,
        return_components_artifact_reference: input.return_components_artifact_reference,
    };

    Ok(build_stage_parameter_artifact(
        run_id,
        execution_mode,
        CalibrationStage::ReturnBackfit,
        artifact_reference,
        metadata,
        None,
        CalibrationStageParameterPayload::ReturnBackfit(Box::new(payload)),
    ))
}

pub(super) fn validate_return_backfit_stage_parameter_payload(
    stage: CalibrationStage,
    payload: &ReturnBackfitStageParameterPayload,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    validate_non_empty_artifact_value(
        payload.selected_candidate_id.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_backfit.selected_candidate_id",
        },
    )?;

    validate_non_empty_artifact_value(
        payload
            .effective_return_components_selected_candidate_id
            .as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_backfit.effective_return_components_selected_candidate_id",
        },
    )?;

    validate_positive_number(
        payload.source_window_open_exposure_hours,
        "source_window_open_exposure_hours",
    )
    .map_err(|source| {
        CalibrationStageParameterArtifactValidationError::InvalidStageParameterField {
            stage,
            field: "stage_parameters.return_backfit.source_window_open_exposure_hours",
            error_message: source.to_string(),
        }
    })?;

    validate_non_empty_artifact_value(
        payload.candidate_generation_policy.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_backfit.candidate_generation_policy",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.objective_weighting_policy.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_backfit.objective_weighting_policy",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.objective_metric_bucket_policy.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_backfit.objective_metric_bucket_policy",
        },
    )?;

    payload
        .effective_fitted_parameters
        .mean_reversion_fitted_parameters
        .validate()
        .map_err(|source| {
            CalibrationStageParameterArtifactValidationError::InvalidMeanReversionParameters {
                stage,
                error_message: source.to_string(),
            }
        })?;

    validate_return_backfit_stage_parameter_boundary(payload)?;
    validate_optimizer_state_absence(stage, optimizer_state)
}

fn validate_return_backfit_stage_parameter_boundary(
    payload: &ReturnBackfitStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    validate_return_components_input_boundary(
        payload.return_components_input_transport,
        payload.return_components_selected_candidate_id.as_deref(),
        payload.return_components_artifact_reference.as_deref(),
        |expected_transport| {
            CalibrationStageParameterArtifactValidationError::ReturnBackfitBoundaryMismatch {
                expected_transport,
            }
        },
    )
}
