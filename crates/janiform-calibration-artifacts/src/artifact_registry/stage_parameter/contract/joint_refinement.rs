use serde::{Deserialize, Serialize};

use super::super::super::super::contracts::{CalibrationExecutionMode, CalibrationStage};
use super::super::super::super::{
    HawkesStageFittedParameters, JointRefinementStageFittedParameters,
    JointRefinementStageObjectiveBreakdown, JointRefinementStageOptimizerDiagnostics,
    ReturnComponentsStageFittedParameters,
};
use super::artifact::{CalibrationStageParameterArtifact, build_stage_parameter_artifact};
use super::errors::{
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactValidationError,
};
use super::input::CalibrationStageParameterArtifactInput;
use super::metadata::CalibrationStageParameterBuildMetadata;
use super::optimizer::{
    CalibrationStageParameterOptimizerState, CalibrationStageParameterOptimizerStateInput,
};
use super::payload::CalibrationStageParameterPayload;
use super::support::{validate_non_empty_artifact_value, validate_optimizer_state_presence};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JointRefinementStageParameterPayload {
    pub selected_candidate_id: String,
    pub fitted_parameters: JointRefinementStageFittedParameters,
    pub baseline_hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageParameterArtifactInput {
    pub stage: CalibrationStage,
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub optimizer_state: CalibrationStageParameterOptimizerStateInput,
    pub selected_candidate_id: String,
    pub fitted_parameters: JointRefinementStageFittedParameters,
    pub baseline_hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics,
}

impl CalibrationStageParameterArtifactInput for JointRefinementStageParameterArtifactInput {
    fn stage(&self) -> CalibrationStage {
        self.stage
    }

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
    {
        build_joint_refinement_stage_parameter_artifact(
            run_id,
            execution_mode,
            *self,
            artifact_reference,
        )
    }
}

pub(super) fn build_joint_refinement_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: JointRefinementStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let JointRefinementStageParameterArtifactInput {
        stage,
        metadata,
        optimizer_state,
        selected_candidate_id,
        fitted_parameters,
        baseline_hawkes_fitted_parameters,
        baseline_return_components_fitted_parameters,
        objective_breakdown,
        optimizer_diagnostics,
    } = input;

    let payload = JointRefinementStageParameterPayload {
        selected_candidate_id,
        fitted_parameters,
        baseline_hawkes_fitted_parameters,
        baseline_return_components_fitted_parameters,
        objective_breakdown,
        optimizer_diagnostics,
    };

    let stage_parameters = match stage {
        CalibrationStage::JointRefinement => {
            CalibrationStageParameterPayload::JointRefinement(Box::new(payload))
        }
        CalibrationStage::OptionRefinement => {
            CalibrationStageParameterPayload::OptionRefinement(Box::new(payload))
        }
        CalibrationStage::Finalization => {
            CalibrationStageParameterPayload::Finalization(Box::new(payload))
        }
        _ => {
            return Err(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage,
                    error_message:
                        "joint refinement parameter payload cannot be used for this stage"
                            .to_string(),
                },
            );
        }
    };

    Ok(build_stage_parameter_artifact(
        run_id,
        execution_mode,
        stage,
        artifact_reference,
        metadata,
        Some(optimizer_state),
        stage_parameters,
    ))
}

pub(super) fn validate_joint_refinement_stage_parameter_payload(
    stage: CalibrationStage,
    payload: &JointRefinementStageParameterPayload,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    validate_non_empty_artifact_value(
        payload.selected_candidate_id.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.selected_candidate_id",
        },
    )?;

    payload
        .fitted_parameters
        .return_components_fitted_parameters
        .mean_reversion_fitted_parameters
        .validate()
        .map_err(|source| {
            CalibrationStageParameterArtifactValidationError::InvalidMeanReversionParameters {
                stage,
                error_message: source.to_string(),
            }
        })?;

    payload
        .baseline_return_components_fitted_parameters
        .mean_reversion_fitted_parameters
        .validate()
        .map_err(|source| {
            CalibrationStageParameterArtifactValidationError::InvalidMeanReversionParameters {
                stage,
                error_message: source.to_string(),
            }
        })?;

    validate_optimizer_state_presence(stage, optimizer_state)
}
