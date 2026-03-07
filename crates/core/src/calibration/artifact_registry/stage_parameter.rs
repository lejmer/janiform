use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::contracts::{
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunContract,
    CalibrationRunSpec, CalibrationStage, CalibrationStageHandoffTransport,
    CalibrationStageSeedSource,
};
use super::super::objective::CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION;
use super::super::staged_runner::CalibrationStagedRunStageReport;
use super::super::stages::hawkes::{HawkesStageExecutionReport, HawkesStageFittedParameters};
use super::super::stages::return_components::{
    ReturnComponentsStageExecutionReport, ReturnComponentsStageFittedParameters,
};
use super::super::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageExecutionReport,
    SeasonalityStageNormalizationMetadata,
};
use super::dataset_snapshot::CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY;
use super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind, calibration_stage_token,
    load_validated_json_artifact, persist_validated_json_artifact,
    stage_output_artifact_references_for_stage,
};

pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.v1";
pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY: &str =
    "calibration.artifact_registry.stage_parameter";
pub const CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION: &str = "v1";
pub const CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.output_schema_metadata.v1";
pub const CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.stage_parameter.optimizer_state.v1";

const CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS: [&str; 11] = [
    "schema_version",
    "run_id",
    "execution_mode",
    "stage",
    "artifact_reference",
    "schema_metadata",
    "output_schema_metadata",
    "stage_output_schema_version",
    "seed_metadata",
    "solver_settings",
    "stage_parameters",
];
const CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS: [&str; 4] = [
    "run_global_seed",
    "stage_seed",
    "stage_seed_source",
    "stage_seed_derivation_policy",
];
const CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS: [&str; 6] = [
    "schema_version",
    "optimizer_adapter_metadata",
    "input_candidate_count",
    "valid_candidate_count",
    "selected_candidate_raw_objective",
    "selected_candidate_penalized_objective",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterOutputSchemaMetadata {
    pub schema_version: String,
    pub required_artifact_fields: Vec<String>,
    pub required_seed_metadata_fields: Vec<String>,
    pub required_optimizer_state_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterSeedMetadata {
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterOptimizerState {
    pub schema_version: String,
    pub optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterOptimizerAdapterMetadata {
    pub schema_version: String,
    pub backend_id: String,
    pub random_seed: u64,
    pub selection_policy: String,
    pub penalty_policy: String,
    pub bounds_policy: String,
    pub solver_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonalityStageParameterPayload {
    pub dataset_id: String,
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub estimator_version: String,
    pub timestamp_event_input_schema_version: String,
    pub timestamp_event_preprocessed_schema_version: String,
    pub intraday_estimate_schema_version: String,
    pub day_of_week_estimate_schema_version: String,
    pub day_of_year_estimate_schema_version: String,
    pub composition_schema_version: String,
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub normalization_metadata: SeasonalityStageNormalizationMetadata,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnComponentsStageParameterPayload {
    pub selected_candidate_id: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "payload_kind", content = "payload", rename_all = "snake_case")]
pub enum CalibrationStageParameterPayload {
    Seasonality(SeasonalityStageParameterPayload),
    Hawkes(HawkesStageParameterPayload),
    ReturnComponents(ReturnComponentsStageParameterPayload),
}

impl CalibrationStageParameterPayload {
    const fn stage(&self) -> CalibrationStage {
        match self {
            Self::Seasonality(_) => CalibrationStage::Seasonality,
            Self::Hawkes(_) => CalibrationStage::Hawkes,
            Self::ReturnComponents(_) => CalibrationStage::ReturnComponents,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStageParameterArtifact {
    pub schema_version: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub stage: CalibrationStage,
    pub artifact_reference: String,
    pub schema_metadata: CalibrationStageParameterSchemaMetadata,
    pub output_schema_metadata: CalibrationStageParameterOutputSchemaMetadata,
    pub stage_output_schema_version: String,
    pub seed_metadata: CalibrationStageParameterSeedMetadata,
    pub solver_settings: CalibrationOptimizerSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimizer_state: Option<CalibrationStageParameterOptimizerState>,
    pub stage_parameters: CalibrationStageParameterPayload,
}

impl CalibrationStageParameterArtifact {
    pub fn validate(&self) -> Result<(), CalibrationStageParameterArtifactValidationError> {
        validate_stage_parameter_metadata_field(
            "schema_version",
            CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION,
            self.schema_version.as_str(),
        )?;

        if self.run_id.trim().is_empty() {
            return Err(CalibrationStageParameterArtifactValidationError::EmptyRunId);
        }
        if self.execution_mode != CalibrationExecutionMode::Disk {
            return Err(
                CalibrationStageParameterArtifactValidationError::UnsupportedExecutionMode {
                    expected: CalibrationExecutionMode::Disk,
                    found: self.execution_mode,
                },
            );
        }
        if self.artifact_reference.trim().is_empty() {
            return Err(
                CalibrationStageParameterArtifactValidationError::EmptyArtifactReference {
                    stage: self.stage,
                },
            );
        }
        if self.stage_output_schema_version.trim().is_empty() {
            return Err(
                CalibrationStageParameterArtifactValidationError::EmptyStageOutputSchemaVersion {
                    stage: self.stage,
                },
            );
        }

        validate_stage_parameter_metadata_field(
            "schema_metadata.schema_family",
            CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY,
            self.schema_metadata.schema_family.as_str(),
        )?;
        validate_stage_parameter_metadata_field(
            "schema_metadata.schema_generation",
            CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION,
            self.schema_metadata.schema_generation.as_str(),
        )?;
        validate_stage_parameter_metadata_field(
            "schema_metadata.forward_extension_policy",
            CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY,
            self.schema_metadata.forward_extension_policy.as_str(),
        )?;
        validate_stage_parameter_metadata_field(
            "output_schema_metadata.schema_version",
            CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION,
            self.output_schema_metadata.schema_version.as_str(),
        )?;
        validate_stage_parameter_required_string_list(
            "output_schema_metadata.required_artifact_fields",
            &CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS,
            self.output_schema_metadata
                .required_artifact_fields
                .as_slice(),
        )?;
        validate_stage_parameter_required_string_list(
            "output_schema_metadata.required_seed_metadata_fields",
            &CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS,
            self.output_schema_metadata
                .required_seed_metadata_fields
                .as_slice(),
        )?;
        validate_stage_parameter_required_string_list(
            "output_schema_metadata.required_optimizer_state_fields",
            &CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS,
            self.output_schema_metadata
                .required_optimizer_state_fields
                .as_slice(),
        )?;

        if self
            .seed_metadata
            .stage_seed_derivation_policy
            .trim()
            .is_empty()
        {
            return Err(
                CalibrationStageParameterArtifactValidationError::EmptySeedMetadataField {
                    field: "seed_metadata.stage_seed_derivation_policy",
                },
            );
        }

        self.solver_settings.validate().map_err(|source| {
            CalibrationStageParameterArtifactValidationError::InvalidSolverSettings {
                stage: self.stage,
                error_message: source.to_string(),
            }
        })?;

        if self.stage_parameters.stage() != self.stage {
            return Err(
                CalibrationStageParameterArtifactValidationError::StagePayloadMismatch {
                    stage: self.stage,
                    payload_stage: self.stage_parameters.stage(),
                },
            );
        }

        match &self.stage_parameters {
            CalibrationStageParameterPayload::Seasonality(payload) => {
                validate_non_empty_artifact_value(
                    payload.dataset_id.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.dataset_id",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.exchange_code.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.exchange_code",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.trade_events_data_reference.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.trade_events_data_reference",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.estimator_version.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.seasonality.estimator_version",
                    },
                )?;
                if payload.intraday_factor_by_bin.is_empty()
                    || payload.day_of_week_factor.is_empty()
                    || payload.day_of_year_factor.is_empty()
                {
                    return Err(
                        CalibrationStageParameterArtifactValidationError::MissingSeasonalityFactors,
                    );
                }
                if self.optimizer_state.is_some() {
                    return Err(
                        CalibrationStageParameterArtifactValidationError::UnexpectedOptimizerState {
                            stage: self.stage,
                        },
                    );
                }
            }
            CalibrationStageParameterPayload::Hawkes(payload) => {
                validate_non_empty_artifact_value(
                    payload.selected_candidate_id.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.hawkes.selected_candidate_id",
                    },
                )?;
                validate_hawkes_stage_parameter_boundary(payload)?;
                validate_optimizer_state_presence(self.stage, self.optimizer_state.as_ref())?;
            }
            CalibrationStageParameterPayload::ReturnComponents(payload) => {
                validate_non_empty_artifact_value(
                    payload.selected_candidate_id.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.selected_candidate_id",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.candidate_generation_policy.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.candidate_generation_policy",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.objective_weighting_policy.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.objective_weighting_policy",
                    },
                )?;
                validate_non_empty_artifact_value(
                    payload.objective_metric_bucket_policy.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
                        field: "stage_parameters.return_components.objective_metric_bucket_policy",
                    },
                )?;
                validate_return_components_stage_parameter_boundary(payload)?;
                validate_optimizer_state_presence(self.stage, self.optimizer_state.as_ref())?;
            }
        }

        if let Some(optimizer_state) = &self.optimizer_state {
            validate_stage_parameter_metadata_field(
                "optimizer_state.schema_version",
                CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION,
                optimizer_state.schema_version.as_str(),
            )?;
            validate_stage_parameter_metadata_field(
                "optimizer_state.optimizer_adapter_metadata.schema_version",
                CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
                optimizer_state
                    .optimizer_adapter_metadata
                    .schema_version
                    .as_str(),
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .backend_id
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata backend_id must be non-empty"
                        .to_string(),
                },
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .selection_policy
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata selection_policy must be non-empty"
                        .to_string(),
                },
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .penalty_policy
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata penalty_policy must be non-empty"
                        .to_string(),
                },
            )?;
            validate_non_empty_artifact_value(
                optimizer_state
                    .optimizer_adapter_metadata
                    .bounds_policy
                    .as_str(),
                CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                    stage: self.stage,
                    error_message: "optimizer adapter metadata bounds_policy must be non-empty"
                        .to_string(),
                },
            )?;
            optimizer_state
                .optimizer_adapter_metadata
                .solver_settings
                .validate()
                .map_err(|source| {
                    CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                        stage: self.stage,
                        error_message: source.to_string(),
                    }
                })?;
            if optimizer_state.valid_candidate_count > optimizer_state.input_candidate_count {
                return Err(
                    CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                        stage: self.stage,
                        error_message: "valid_candidate_count must be <= input_candidate_count"
                            .to_string(),
                    },
                );
            }
            if !optimizer_state.selected_candidate_raw_objective.is_finite()
                || !optimizer_state
                    .selected_candidate_penalized_objective
                    .is_finite()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::InvalidOptimizerState {
                        stage: self.stage,
                        error_message: "selected candidate objective values must be finite"
                            .to_string(),
                    },
                );
            }
        }

        Ok(())
    }
}

pub fn build_calibration_stage_parameter_artifact(
    run_contract: &CalibrationRunContract,
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            CalibrationStageParameterArtifactBuildError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: run_contract.run_spec.execution_mode,
            },
        );
    }

    let stage = match stage_report {
        CalibrationStagedRunStageReport::Seasonality(_) => CalibrationStage::Seasonality,
        CalibrationStagedRunStageReport::Hawkes(_) => CalibrationStage::Hawkes,
        CalibrationStagedRunStageReport::ReturnComponents(_) => CalibrationStage::ReturnComponents,
    };
    let artifact_reference =
        resolve_stage_output_artifact_reference(&run_contract.run_spec, stage)?;

    let artifact = match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            build_seasonality_stage_parameter_artifact(report, artifact_reference)
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            build_hawkes_stage_parameter_artifact(report, artifact_reference)
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            build_return_components_stage_parameter_artifact(report, artifact_reference)
        }
    }?;

    artifact.validate().map_err(|source| {
        CalibrationStageParameterArtifactBuildError::ValidationFailed {
            stage,
            error_message: source.to_string(),
        }
    })?;

    Ok(artifact)
}

pub fn calibration_stage_parameter_artifact_path(
    disk_checkpoint_directory: &Path,
    run_id: &str,
    stage: CalibrationStage,
) -> PathBuf {
    disk_checkpoint_directory.join(run_id).join(format!(
        "{}.stage_parameter.artifact.json",
        calibration_stage_token(stage)
    ))
}

pub fn persist_calibration_stage_parameter_artifact_to_disk(
    disk_checkpoint_directory: &Path,
    artifact: &CalibrationStageParameterArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::StageParameter;
    let artifact_path = calibration_stage_parameter_artifact_path(
        disk_checkpoint_directory,
        artifact.run_id.as_str(),
        artifact.stage,
    );
    persist_validated_json_artifact(
        artifact_kind,
        artifact_path.as_path(),
        artifact,
        |payload| payload.validate().map_err(|source| source.to_string()),
    )
}

pub fn load_calibration_stage_parameter_artifact_from_disk(
    artifact_path: &Path,
) -> Result<CalibrationStageParameterArtifact, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::StageParameter;
    load_validated_json_artifact(
        artifact_kind,
        artifact_path,
        |payload: &CalibrationStageParameterArtifact| {
            payload.validate().map_err(|source| source.to_string())
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageParameterArtifactBuildError {
    UnsupportedExecutionMode {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    StageReportWasNotSuccessful {
        stage: CalibrationStage,
    },
    MissingSuccessfulStagePayload {
        stage: CalibrationStage,
    },
    MissingSuccessfulStageMetadata {
        stage: CalibrationStage,
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
            Self::StageReportWasNotSuccessful { stage } => write!(
                formatter,
                "cannot build stage parameter artifact for unsuccessful stage `{stage:?}`"
            ),
            Self::MissingSuccessfulStagePayload { stage } => write!(
                formatter,
                "successful stage `{stage:?}` is missing its stage output payload"
            ),
            Self::MissingSuccessfulStageMetadata { stage } => write!(
                formatter,
                "successful stage `{stage:?}` is missing its execution metadata"
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
    MissingSeasonalityFactors,
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
    HawkesBoundaryMismatch {
        expected_transport: &'static str,
    },
    ReturnComponentsBoundaryMismatch {
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
            Self::MissingSeasonalityFactors => write!(
                formatter,
                "seasonality stage parameter artifact requires non-empty fitted factor vectors"
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
            Self::HawkesBoundaryMismatch { expected_transport } => write!(
                formatter,
                "hawkes stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
            Self::ReturnComponentsBoundaryMismatch { expected_transport } => write!(
                formatter,
                "return-components stage parameter boundary does not match expected `{expected_transport}` transport contract"
            ),
        }
    }
}

impl Error for CalibrationStageParameterArtifactValidationError {}

fn build_stage_parameter_output_schema_metadata() -> CalibrationStageParameterOutputSchemaMetadata {
    CalibrationStageParameterOutputSchemaMetadata {
        schema_version: CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION.to_string(),
        required_artifact_fields: CALIBRATION_STAGE_PARAMETER_REQUIRED_ARTIFACT_FIELDS
            .iter()
            .map(|field| (*field).to_string())
            .collect(),
        required_seed_metadata_fields: CALIBRATION_STAGE_PARAMETER_REQUIRED_SEED_METADATA_FIELDS
            .iter()
            .map(|field| (*field).to_string())
            .collect(),
        required_optimizer_state_fields:
            CALIBRATION_STAGE_PARAMETER_REQUIRED_OPTIMIZER_STATE_FIELDS
                .iter()
                .map(|field| (*field).to_string())
                .collect(),
    }
}

fn build_stage_parameter_seed_metadata(
    run_global_seed: u64,
    stage_seed: u64,
    stage_seed_source: CalibrationStageSeedSource,
    stage_seed_derivation_policy: &str,
) -> CalibrationStageParameterSeedMetadata {
    CalibrationStageParameterSeedMetadata {
        run_global_seed,
        stage_seed,
        stage_seed_source,
        stage_seed_derivation_policy: stage_seed_derivation_policy.to_string(),
    }
}

fn build_hawkes_optimizer_state(
    report: &HawkesStageExecutionReport,
) -> Result<CalibrationStageParameterOptimizerState, CalibrationStageParameterArtifactBuildError> {
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::Hawkes,
        },
    )?;

    Ok(CalibrationStageParameterOptimizerState {
        schema_version: CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION.to_string(),
        optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata {
            schema_version: metadata
                .optimizer_adapter_metadata
                .schema_version
                .to_string(),
            backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
            random_seed: metadata.optimizer_adapter_metadata.random_seed,
            selection_policy: metadata
                .optimizer_adapter_metadata
                .selection_policy
                .to_string(),
            penalty_policy: metadata
                .optimizer_adapter_metadata
                .penalty_policy
                .to_string(),
            bounds_policy: metadata
                .optimizer_adapter_metadata
                .bounds_policy
                .to_string(),
            solver_settings: metadata.optimizer_adapter_metadata.solver_settings.clone(),
        },
        input_candidate_count: metadata.input_candidate_count,
        valid_candidate_count: metadata.valid_candidate_count,
        selected_candidate_raw_objective: metadata.selected_candidate_raw_objective,
        selected_candidate_penalized_objective: metadata.selected_candidate_penalized_objective,
    })
}

fn build_return_components_optimizer_state(
    report: &ReturnComponentsStageExecutionReport,
) -> Result<CalibrationStageParameterOptimizerState, CalibrationStageParameterArtifactBuildError> {
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::ReturnComponents,
        },
    )?;

    Ok(CalibrationStageParameterOptimizerState {
        schema_version: CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION.to_string(),
        optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata {
            schema_version: metadata
                .optimizer_adapter_metadata
                .schema_version
                .to_string(),
            backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
            random_seed: metadata.optimizer_adapter_metadata.random_seed,
            selection_policy: metadata
                .optimizer_adapter_metadata
                .selection_policy
                .to_string(),
            penalty_policy: metadata
                .optimizer_adapter_metadata
                .penalty_policy
                .to_string(),
            bounds_policy: metadata
                .optimizer_adapter_metadata
                .bounds_policy
                .to_string(),
            solver_settings: metadata.optimizer_adapter_metadata.solver_settings.clone(),
        },
        input_candidate_count: metadata.input_candidate_count,
        valid_candidate_count: metadata.valid_candidate_count,
        selected_candidate_raw_objective: metadata.selected_candidate_raw_objective,
        selected_candidate_penalized_objective: metadata.selected_candidate_penalized_objective,
    })
}

fn build_seasonality_stage_parameter_artifact(
    report: &SeasonalityStageExecutionReport,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if report.status != CalibrationStageExecutionStatus::Success {
        return Err(
            CalibrationStageParameterArtifactBuildError::StageReportWasNotSuccessful {
                stage: CalibrationStage::Seasonality,
            },
        );
    }

    let payload = report.normalized_stage_payload.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStagePayload {
            stage: CalibrationStage::Seasonality,
        },
    )?;
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::Seasonality,
        },
    )?;

    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: report.run_id.clone(),
        execution_mode: report.execution_mode,
        stage: CalibrationStage::Seasonality,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy,
        ),
        solver_settings: metadata.solver_settings.clone(),
        optimizer_state: None,
        stage_parameters: CalibrationStageParameterPayload::Seasonality(
            SeasonalityStageParameterPayload {
                dataset_id: payload.dataset_id.clone(),
                exchange_code: payload.exchange_code.clone(),
                trade_events_data_reference: payload.trade_events_data_reference.clone(),
                estimator_version: metadata.estimator_version.clone(),
                timestamp_event_input_schema_version: payload
                    .timestamp_event_input_schema_version
                    .clone(),
                timestamp_event_preprocessed_schema_version: payload
                    .timestamp_event_preprocessed_schema_version
                    .clone(),
                intraday_estimate_schema_version: payload.intraday_estimate_schema_version.clone(),
                day_of_week_estimate_schema_version: payload
                    .day_of_week_estimate_schema_version
                    .clone(),
                day_of_year_estimate_schema_version: payload
                    .day_of_year_estimate_schema_version
                    .clone(),
                composition_schema_version: payload.composition_schema_version.clone(),
                intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
                intraday_factor_by_bin: payload.intraday_factor_by_bin.clone(),
                day_of_week_factor: payload.day_of_week_factor.clone(),
                day_of_year_factor: payload.day_of_year_factor.clone(),
                normalization_metadata: payload.normalization_metadata.clone(),
            },
        ),
    })
}

fn build_hawkes_stage_parameter_artifact(
    report: &HawkesStageExecutionReport,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if report.status != CalibrationStageExecutionStatus::Success {
        return Err(
            CalibrationStageParameterArtifactBuildError::StageReportWasNotSuccessful {
                stage: CalibrationStage::Hawkes,
            },
        );
    }

    let payload = report.fitted_stage_payload.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStagePayload {
            stage: CalibrationStage::Hawkes,
        },
    )?;
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::Hawkes,
        },
    )?;

    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: report.run_id.clone(),
        execution_mode: report.execution_mode,
        stage: CalibrationStage::Hawkes,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy,
        ),
        solver_settings: metadata.solver_settings.clone(),
        optimizer_state: Some(build_hawkes_optimizer_state(report)?),
        stage_parameters: CalibrationStageParameterPayload::Hawkes(HawkesStageParameterPayload {
            selected_candidate_id: payload.selected_candidate_id.clone(),
            fitted_parameters: payload.fitted_parameters.clone(),
            seasonality_input_transport: payload.seasonality_input_transport,
            seasonality_dataset_id: payload.seasonality_dataset_id.clone(),
            seasonality_artifact_reference: payload.seasonality_artifact_reference.clone(),
        }),
    })
}

fn build_return_components_stage_parameter_artifact(
    report: &ReturnComponentsStageExecutionReport,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if report.status != CalibrationStageExecutionStatus::Success {
        return Err(
            CalibrationStageParameterArtifactBuildError::StageReportWasNotSuccessful {
                stage: CalibrationStage::ReturnComponents,
            },
        );
    }

    let payload = report.fitted_stage_payload.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStagePayload {
            stage: CalibrationStage::ReturnComponents,
        },
    )?;
    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::MissingSuccessfulStageMetadata {
            stage: CalibrationStage::ReturnComponents,
        },
    )?;

    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: report.run_id.clone(),
        execution_mode: report.execution_mode,
        stage: CalibrationStage::ReturnComponents,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy,
        ),
        solver_settings: metadata.solver_settings.clone(),
        optimizer_state: Some(build_return_components_optimizer_state(report)?),
        stage_parameters: CalibrationStageParameterPayload::ReturnComponents(
            ReturnComponentsStageParameterPayload {
                selected_candidate_id: payload.selected_candidate_id.clone(),
                fitted_parameters: payload.fitted_parameters.clone(),
                candidate_generation_policy: payload.candidate_generation_policy.to_string(),
                objective_weighting_policy: payload.objective_weighting_policy.to_string(),
                objective_metric_bucket_policy: payload.objective_metric_bucket_policy.to_string(),
                activity_input_transport: payload.activity_input_transport,
                activity_selected_hawkes_candidate_id: payload
                    .activity_selected_hawkes_candidate_id
                    .clone(),
                activity_artifact_reference: payload.activity_artifact_reference.clone(),
            },
        ),
    })
}

fn resolve_stage_output_artifact_reference(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Result<String, CalibrationStageParameterArtifactBuildError> {
    let artifact_references = stage_output_artifact_references_for_stage(run_spec, stage);

    match artifact_references.as_slice() {
        [] => Ok(format!(
            "artifact://{}/{}",
            calibration_stage_token(stage),
            run_spec.run_id
        )),
        [artifact_reference] => Ok(artifact_reference.clone()),
        _ => Err(
            CalibrationStageParameterArtifactBuildError::ConflictingArtifactReferences {
                stage,
                artifact_references,
            },
        ),
    }
}

fn validate_stage_parameter_metadata_field(
    field: &'static str,
    expected: &'static str,
    found: &str,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    if found != expected {
        return Err(
            CalibrationStageParameterArtifactValidationError::MetadataFieldMismatch {
                field,
                expected,
                found: found.to_string(),
            },
        );
    }

    Ok(())
}

fn validate_stage_parameter_required_string_list(
    field: &'static str,
    expected: &[&str],
    found: &[String],
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    let expected_fields: Vec<String> = expected.iter().map(|value| (*value).to_string()).collect();
    if found != expected_fields.as_slice() {
        return Err(
            CalibrationStageParameterArtifactValidationError::RequiredFieldListMismatch {
                field,
                expected: expected_fields,
                found: found.to_vec(),
            },
        );
    }

    Ok(())
}

fn validate_non_empty_artifact_value<E>(value: &str, error: E) -> Result<(), E> {
    if value.trim().is_empty() {
        return Err(error);
    }

    Ok(())
}

fn validate_optimizer_state_presence(
    stage: CalibrationStage,
    optimizer_state: Option<&CalibrationStageParameterOptimizerState>,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    if optimizer_state.is_none() {
        return Err(
            CalibrationStageParameterArtifactValidationError::MissingOptimizerState { stage },
        );
    }

    Ok(())
}

fn validate_hawkes_stage_parameter_boundary(
    payload: &HawkesStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match payload.seasonality_input_transport {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            if payload
                .seasonality_dataset_id
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
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
                .is_none_or(|value| value.trim().is_empty())
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

fn validate_return_components_stage_parameter_boundary(
    payload: &ReturnComponentsStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match payload.activity_input_transport {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            if payload
                .activity_selected_hawkes_candidate_id
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
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
                .is_none_or(|value| value.trim().is_empty())
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
