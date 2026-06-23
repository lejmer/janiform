use janiform_foundations::numeric::validate_positive_number;
use janiform_foundations::strings::is_non_empty_string;
use serde::{Deserialize, Serialize};

use super::super::super::super::ReturnComponentsStageFittedParameters;
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
pub struct ReturnComponentsStageParameterPayload {
    pub selected_candidate_id: String,
    pub source_window_open_exposure_hours: f64,
    pub fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_generation_policy: String,
    pub objective_weighting_policy: String,
    pub objective_metric_bucket_policy: String,
    pub activity_input_transport: CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_selected_hawkes_candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub optimizer_state: CalibrationStageParameterOptimizerStateInput,
    pub selected_candidate_id: String,
    pub source_window_open_exposure_hours: f64,
    pub fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_generation_policy: String,
    pub objective_weighting_policy: String,
    pub objective_metric_bucket_policy: String,
    pub activity_input_transport: CalibrationStageHandoffTransport,
    pub activity_selected_hawkes_candidate_id: Option<String>,
    pub activity_artifact_reference: Option<String>,
}

impl CalibrationStageParameterArtifactInput for ReturnComponentsStageParameterArtifactInput {
    fn stage(&self) -> CalibrationStage {
        CalibrationStage::ReturnComponents
    }

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
    {
        build_return_components_stage_parameter_artifact(
            run_id,
            execution_mode,
            *self,
            artifact_reference,
        )
    }
}

pub(super) fn build_return_components_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: ReturnComponentsStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let metadata = input.metadata;
    let optimizer_state = input.optimizer_state;

    let payload = ReturnComponentsStageParameterPayload {
        selected_candidate_id: input.selected_candidate_id,
        source_window_open_exposure_hours: input.source_window_open_exposure_hours,
        fitted_parameters: input.fitted_parameters,
        candidate_generation_policy: input.candidate_generation_policy,
        objective_weighting_policy: input.objective_weighting_policy,
        objective_metric_bucket_policy: input.objective_metric_bucket_policy,
        activity_input_transport: input.activity_input_transport,
        activity_selected_hawkes_candidate_id: input.activity_selected_hawkes_candidate_id,
        activity_artifact_reference: input.activity_artifact_reference,
    };

    Ok(build_stage_parameter_artifact(
        run_id,
        execution_mode,
        CalibrationStage::ReturnComponents,
        artifact_reference,
        metadata,
        Some(optimizer_state),
        CalibrationStageParameterPayload::ReturnComponents(Box::new(payload)),
    ))
}

pub(super) fn validate_return_components_stage_parameter_payload(
    stage: CalibrationStage,
    payload: &ReturnComponentsStageParameterPayload,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    validate_non_empty_artifact_value(
        payload.selected_candidate_id.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_components.selected_candidate_id",
        },
    )?;

    validate_positive_number(
        payload.source_window_open_exposure_hours,
        "source_window_open_exposure_hours",
    )
    .map_err(|source| {
        CalibrationStageParameterArtifactValidationError::InvalidStageParameterField {
            stage,
            field: "stage_parameters.return_components.source_window_open_exposure_hours",
            error_message: source.to_string(),
        }
    })?;

    validate_non_empty_artifact_value(
        payload.candidate_generation_policy.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_components.candidate_generation_policy",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.objective_weighting_policy.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_components.objective_weighting_policy",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.objective_metric_bucket_policy.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.return_components.objective_metric_bucket_policy",
        },
    )?;

    validate_return_components_stage_parameter_boundary(payload)?;

    payload
        .fitted_parameters
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

fn validate_return_components_stage_parameter_boundary(
    payload: &ReturnComponentsStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match payload.activity_input_transport {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            if payload
                .activity_selected_hawkes_candidate_id
                .as_ref()
                .is_none_or(|value| !is_non_empty_string(value))
                || payload.activity_artifact_reference.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::ReturnComponentsBoundaryMismatch {
                        expected_transport: "in_memory_output",
                    },
                );
            }
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => {
            if payload
                .activity_artifact_reference
                .as_ref()
                .is_none_or(|value| !is_non_empty_string(value))
                || payload.activity_selected_hawkes_candidate_id.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::ReturnComponentsBoundaryMismatch {
                        expected_transport: "persisted_artifact_reference",
                    },
                );
            }
        }
    }

    Ok(())
}
