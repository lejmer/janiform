use janiform_foundations::strings::is_non_empty_string;
use serde::{Deserialize, Serialize};

use super::super::super::super::HawkesStageFittedParameters;
use super::super::super::super::contracts::{
    CalibrationExecutionMode, CalibrationStage, CalibrationStageHandoffTransport,
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
pub struct HawkesStageParameterPayload {
    pub selected_candidate_id: String,
    pub fitted_parameters: HawkesStageFittedParameters,
    pub seasonality_input_transport: CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality_dataset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HawkesStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub optimizer_state: CalibrationStageParameterOptimizerStateInput,
    pub selected_candidate_id: String,
    pub fitted_parameters: HawkesStageFittedParameters,
    pub seasonality_input_transport: CalibrationStageHandoffTransport,
    pub seasonality_dataset_id: Option<String>,
    pub seasonality_artifact_reference: Option<String>,
}

impl CalibrationStageParameterArtifactInput for HawkesStageParameterArtifactInput {
    fn stage(&self) -> CalibrationStage {
        CalibrationStage::Hawkes
    }

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
    {
        build_hawkes_stage_parameter_artifact(run_id, execution_mode, *self, artifact_reference)
    }
}

pub(super) fn build_hawkes_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: HawkesStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let metadata = input.metadata;
    let optimizer_state = input.optimizer_state;

    let payload = HawkesStageParameterPayload {
        selected_candidate_id: input.selected_candidate_id,
        fitted_parameters: input.fitted_parameters,
        seasonality_input_transport: input.seasonality_input_transport,
        seasonality_dataset_id: input.seasonality_dataset_id,
        seasonality_artifact_reference: input.seasonality_artifact_reference,
    };

    Ok(build_stage_parameter_artifact(
        run_id,
        execution_mode,
        CalibrationStage::Hawkes,
        artifact_reference,
        metadata,
        Some(optimizer_state),
        CalibrationStageParameterPayload::Hawkes(Box::new(payload)),
    ))
}

pub(super) fn validate_hawkes_stage_parameter_payload(
    stage: CalibrationStage,
    payload: &HawkesStageParameterPayload,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    validate_non_empty_artifact_value(
        payload.selected_candidate_id.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.hawkes.selected_candidate_id",
        },
    )?;

    validate_hawkes_stage_parameter_boundary(payload)?;
    validate_optimizer_state_presence(stage, optimizer_state)
}

fn validate_hawkes_stage_parameter_boundary(
    payload: &HawkesStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match payload.seasonality_input_transport {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            if payload
                .seasonality_dataset_id
                .as_ref()
                .is_none_or(|value| !is_non_empty_string(value))
                || payload.seasonality_artifact_reference.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::HawkesBoundaryMismatch {
                        expected_transport: "in_memory_output",
                    },
                );
            }
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => {
            if payload
                .seasonality_artifact_reference
                .as_ref()
                .is_none_or(|value| !is_non_empty_string(value))
                || payload.seasonality_dataset_id.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::HawkesBoundaryMismatch {
                        expected_transport: "persisted_artifact_reference",
                    },
                );
            }
        }
    }

    Ok(())
}
