use janiform_market_time::business_time_clock::FourierBusinessTimeClockModel;
use serde::{Deserialize, Serialize};

use super::super::super::super::contracts::{CalibrationExecutionMode, CalibrationStage};
use super::artifact::{CalibrationStageParameterArtifact, build_stage_parameter_artifact};
use super::errors::{
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactValidationError,
};
use super::input::CalibrationStageParameterArtifactInput;
use super::metadata::CalibrationStageParameterBuildMetadata;
use super::optimizer::CalibrationStageParameterOptimizerState;
use super::payload::CalibrationStageParameterPayload;
use super::support::{validate_non_empty_artifact_value, validate_optimizer_state_absence};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonalityStageParameterPayload {
    pub dataset_id: String,
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub estimator_version: String,
    pub timestamp_event_input_schema_version: String,
    pub timestamp_event_preprocessed_schema_version: String,
    pub clock_model_schema_version: String,
    pub clock_model: FourierBusinessTimeClockModel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonalityStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub dataset_id: String,
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub estimator_version: String,
    pub timestamp_event_input_schema_version: String,
    pub timestamp_event_preprocessed_schema_version: String,
    pub clock_model_schema_version: String,
    pub clock_model: FourierBusinessTimeClockModel,
}

impl CalibrationStageParameterArtifactInput for SeasonalityStageParameterArtifactInput {
    fn stage(&self) -> CalibrationStage {
        CalibrationStage::Seasonality
    }

    fn build_stage_parameter_artifact(
        self: Box<Self>,
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        artifact_reference: String,
    ) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError>
    {
        build_seasonality_stage_parameter_artifact(
            run_id,
            execution_mode,
            *self,
            artifact_reference,
        )
    }
}

pub(super) fn build_seasonality_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: SeasonalityStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let metadata = input.metadata;

    let payload = SeasonalityStageParameterPayload {
        dataset_id: input.dataset_id,
        exchange_code: input.exchange_code,
        trade_events_data_reference: input.trade_events_data_reference,
        estimator_version: input.estimator_version,
        timestamp_event_input_schema_version: input.timestamp_event_input_schema_version,
        timestamp_event_preprocessed_schema_version: input
            .timestamp_event_preprocessed_schema_version,
        clock_model_schema_version: input.clock_model_schema_version,
        clock_model: input.clock_model,
    };

    Ok(build_stage_parameter_artifact(
        run_id,
        execution_mode,
        CalibrationStage::Seasonality,
        artifact_reference,
        metadata,
        None,
        CalibrationStageParameterPayload::Seasonality(Box::new(payload)),
    ))
}

pub(super) fn validate_seasonality_stage_parameter_payload(
    stage: CalibrationStage,
    payload: &SeasonalityStageParameterPayload,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    validate_non_empty_artifact_value(
        payload.dataset_id.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.seasonality.dataset_id",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.exchange_code.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.seasonality.exchange_code",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.trade_events_data_reference.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.seasonality.trade_events_data_reference",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.estimator_version.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.seasonality.estimator_version",
        },
    )?;

    validate_non_empty_artifact_value(
        payload.clock_model_schema_version.as_str(),
        CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
            stage,
            field: "stage_parameters.seasonality.clock_model_schema_version",
        },
    )?;

    validate_optimizer_state_absence(stage, optimizer_state)
}
