use serde::{Deserialize, Serialize};

use super::super::super::super::LongVarianceStageDiagnostics;
use super::super::super::super::contracts::{
    CalibrationExecutionMode, CalibrationStage, CalibrationStageHandoffTransport,
};
use super::artifact::{CalibrationStageParameterArtifact, build_stage_parameter_artifact};
use super::errors::{
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactValidationError,
};
use super::input::CalibrationStageParameterArtifactInput;
use super::metadata::CalibrationStageParameterBuildMetadata;
use super::optimizer::CalibrationStageParameterOptimizerState;
use super::payload::CalibrationStageParameterPayload;
use super::support::{validate_optimizer_state_absence, validate_return_components_input_boundary};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongVarianceStageParameterPayload {
    pub fitted_parameters: janiform_model_kernels::long_variance::LongVarianceParameters,
    pub diagnostics: LongVarianceStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LongVarianceStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub fitted_parameters: janiform_model_kernels::long_variance::LongVarianceParameters,
    pub diagnostics: LongVarianceStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    pub return_components_selected_candidate_id: Option<String>,
    pub return_components_artifact_reference: Option<String>,
}

impl CalibrationStageParameterArtifactInput for LongVarianceStageParameterArtifactInput {
    fn stage(&self) -> CalibrationStage {
        CalibrationStage::LongVariance
    }

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
    {
        build_long_variance_stage_parameter_artifact(
            run_id,
            execution_mode,
            *self,
            artifact_reference,
        )
    }
}

pub(super) fn build_long_variance_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: LongVarianceStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let metadata = input.metadata;

    let payload = LongVarianceStageParameterPayload {
        fitted_parameters: input.fitted_parameters,
        diagnostics: input.diagnostics,
        return_components_input_transport: input.return_components_input_transport,
        return_components_selected_candidate_id: input.return_components_selected_candidate_id,
        return_components_artifact_reference: input.return_components_artifact_reference,
    };

    Ok(build_stage_parameter_artifact(
        run_id,
        execution_mode,
        CalibrationStage::LongVariance,
        artifact_reference,
        metadata,
        None,
        CalibrationStageParameterPayload::LongVariance(Box::new(payload)),
    ))
}

pub(super) fn validate_long_variance_stage_parameter_payload(
    stage: CalibrationStage,
    payload: &LongVarianceStageParameterPayload,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    payload.fitted_parameters.validate().map_err(|source| {
        CalibrationStageParameterArtifactValidationError::InvalidLongVarianceParameters {
            stage,
            error_message: source.to_string(),
        }
    })?;

    validate_long_variance_stage_parameter_boundary(payload)?;
    validate_optimizer_state_absence(stage, optimizer_state)
}

fn validate_long_variance_stage_parameter_boundary(
    payload: &LongVarianceStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    validate_return_components_input_boundary(
        payload.return_components_input_transport,
        payload.return_components_selected_candidate_id.as_deref(),
        payload.return_components_artifact_reference.as_deref(),
        |expected_transport| {
            CalibrationStageParameterArtifactValidationError::LongVarianceBoundaryMismatch {
                expected_transport,
            }
        },
    )
}
