use std::error::Error;
use std::fmt::{Display, Formatter};

use super::super::super::super::contracts::{CalibrationExecutionMode, CalibrationStage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageParameterArtifactBuildError {
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    ConflictingArtifactReferences {
        stage: CalibrationStage,
        artifact_references: Vec<String>,
    },
    ValidationFailed {
        stage: CalibrationStage,
        error_message: String,
    },
}

impl Display for CalibrationStageParameterArtifactBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "stage parameter artifacts require execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::ConflictingArtifactReferences {
                stage,
                artifact_references,
            } => write!(
                formatter,
                "stage `{stage:?}` has conflicting persisted artifact references {:?}",
                artifact_references
            ),
            Self::ValidationFailed {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` failed validation: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationStageParameterArtifactBuildError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationStageParameterArtifactValidationError {
    MetadataFieldMismatch {
        field: &'static str,
        expected: &'static str,
        found: String,
    },
    RequiredFieldListMismatch {
        field: &'static str,
        expected: Vec<String>,
        found: Vec<String>,
    },
    EmptyRunId,
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    EmptyArtifactReference {
        stage: CalibrationStage,
    },
    EmptyStageOutputSchemaVersion {
        stage: CalibrationStage,
    },
    EmptySeedMetadataField {
        field: &'static str,
    },
    InvalidSolverSettings {
        stage: CalibrationStage,
        error_message: String,
    },
    StagePayloadMismatch {
        stage: CalibrationStage,
        payload_stage: CalibrationStage,
    },
    EmptyStageParameterField {
        stage: CalibrationStage,
        field: &'static str,
    },
    MissingSeasonalityClockModel,
    MissingOptimizerState {
        stage: CalibrationStage,
    },
    UnexpectedOptimizerState {
        stage: CalibrationStage,
    },
    InvalidOptimizerState {
        stage: CalibrationStage,
        error_message: String,
    },
    InvalidNoMoveEventModel {
        stage: CalibrationStage,
        error_message: String,
    },
    InvalidTimestampBatchingObservationModel {
        stage: CalibrationStage,
        error_message: String,
    },
    InvalidMeanReversionParameters {
        stage: CalibrationStage,
        error_message: String,
    },
    InvalidLongVarianceParameters {
        stage: CalibrationStage,
        error_message: String,
    },
    InvalidNewsProcessConfiguration {
        stage: CalibrationStage,
        error_message: String,
    },
    HawkesBoundaryMismatch {
        expected_transport: &'static str,
    },
    ReturnComponentsBoundaryMismatch {
        expected_transport: &'static str,
    },
    MeanReversionBoundaryMismatch {
        expected_transport: &'static str,
    },
    LongVarianceBoundaryMismatch {
        expected_transport: &'static str,
    },
    NewsShocksBoundaryMismatch {
        expected_transport: &'static str,
    },
    ReturnBackfitBoundaryMismatch {
        expected_transport: &'static str,
    },
}

impl Display for CalibrationStageParameterArtifactValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MetadataFieldMismatch {
                field,
                expected,
                found,
            } => write!(formatter, "{field} must be `{expected}`, found `{found}`"),
            Self::RequiredFieldListMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must equal {:?}, found {:?}",
                expected, found
            ),
            Self::EmptyRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::UnsupportedExecutionMode { expected, found } => write!(
                formatter,
                "stage parameter artifact requires execution_mode={expected:?}, found `{found:?}`"
            ),
            Self::EmptyArtifactReference { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` must use a non-empty artifact_reference"
            ),
            Self::EmptyStageOutputSchemaVersion { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` must use a non-empty stage_output_schema_version"
            ),
            Self::EmptySeedMetadataField { field } => {
                write!(formatter, "{field} must be a non-empty string")
            }
            Self::InvalidSolverSettings {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid solver_settings: {error_message}"
            ),
            Self::StagePayloadMismatch {
                stage,
                payload_stage,
            } => write!(
                formatter,
                "stage parameter artifact stage `{stage:?}` does not match payload stage `{payload_stage:?}`"
            ),
            Self::EmptyStageParameterField { stage, field } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` requires non-empty field `{field}`"
            ),
            Self::MissingSeasonalityClockModel => write!(
                formatter,
                "seasonality stage parameter artifact requires a fitted clock model"
            ),
            Self::MissingOptimizerState { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` requires optimizer_state metadata"
            ),
            Self::UnexpectedOptimizerState { stage } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` must not include optimizer_state metadata"
            ),
            Self::InvalidOptimizerState {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid optimizer_state: {error_message}"
            ),
            Self::InvalidNoMoveEventModel {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid no_move_event_model: {error_message}"
            ),
            Self::InvalidTimestampBatchingObservationModel {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid timestamp_batching_observation_model: {error_message}"
            ),
            Self::InvalidMeanReversionParameters {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid mean_reversion_fitted_parameters: {error_message}"
            ),
            Self::InvalidLongVarianceParameters {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid long_variance_parameters: {error_message}"
            ),
            Self::InvalidNewsProcessConfiguration {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid news_process_configuration: {error_message}"
            ),
            Self::HawkesBoundaryMismatch { expected_transport } => write!(
                formatter,
                "hawkes stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
            Self::ReturnComponentsBoundaryMismatch { expected_transport } => write!(
                formatter,
                "return-components stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
            Self::MeanReversionBoundaryMismatch { expected_transport } => write!(
                formatter,
                "mean-reversion stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
            Self::LongVarianceBoundaryMismatch { expected_transport } => write!(
                formatter,
                "long-variance stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
            Self::NewsShocksBoundaryMismatch { expected_transport } => write!(
                formatter,
                "news-shocks stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
            Self::ReturnBackfitBoundaryMismatch { expected_transport } => write!(
                formatter,
                "return-backfit stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
        }
    }
}

impl Error for CalibrationStageParameterArtifactValidationError {}
