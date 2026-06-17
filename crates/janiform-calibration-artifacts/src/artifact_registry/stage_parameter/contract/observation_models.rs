use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel;
use serde::{Deserialize, Serialize};

use super::super::super::super::ObservationModelsStageDiagnostics;
use super::super::super::super::contracts::{CalibrationExecutionMode, CalibrationStage};
use super::artifact::{CalibrationStageParameterArtifact, build_stage_parameter_artifact};
use super::errors::{
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactValidationError,
};
use super::input::CalibrationStageParameterArtifactInput;
use super::metadata::CalibrationStageParameterBuildMetadata;
use super::optimizer::CalibrationStageParameterOptimizerState;
use super::payload::CalibrationStageParameterPayload;
use super::support::validate_optimizer_state_absence;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationModelsStageParameterPayload {
    pub timestamp_batching_observation_model: TimestampBatchingObservationModel,
    pub diagnostics: ObservationModelsStageDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservationModelsStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub timestamp_batching_observation_model: TimestampBatchingObservationModel,
    pub diagnostics: ObservationModelsStageDiagnostics,
}

impl CalibrationStageParameterArtifactInput for ObservationModelsStageParameterArtifactInput {
    fn stage(&self) -> CalibrationStage {
        CalibrationStage::ObservationModels
    }

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
    {
        build_observation_models_stage_parameter_artifact(
            run_id,
            execution_mode,
            *self,
            artifact_reference,
        )
    }
}

pub(super) fn build_observation_models_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: ObservationModelsStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let metadata = input.metadata;

    let payload = ObservationModelsStageParameterPayload {
        timestamp_batching_observation_model: input.timestamp_batching_observation_model,
        diagnostics: input.diagnostics,
    };

    Ok(build_stage_parameter_artifact(
        run_id,
        execution_mode,
        CalibrationStage::ObservationModels,
        artifact_reference,
        metadata,
        None,
        CalibrationStageParameterPayload::ObservationModels(Box::new(payload)),
    ))
}

pub(super) fn validate_observation_models_stage_parameter_payload(
    stage: CalibrationStage,
    payload: &ObservationModelsStageParameterPayload,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    payload
        .timestamp_batching_observation_model
        .validate()
        .map_err(|source| {
            CalibrationStageParameterArtifactValidationError::InvalidTimestampBatchingObservationModel {
                stage,
                error_message: source.to_string(),
            }
        })?;

    validate_optimizer_state_absence(stage, optimizer_state)
}
