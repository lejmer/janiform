use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use janiform_foundations::strings::is_non_empty_string;
use serde::{Deserialize, Serialize};

use super::super::contracts::{
    CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunContract,
    CalibrationRunSpec, CalibrationStage, CalibrationStageHandoffTransport,
    CalibrationStageSeedSource,
};
use super::super::{
    HawkesStageFittedParameters, JointRefinementStageFittedParameters,
    JointRefinementStageObjectiveBreakdown, JointRefinementStageOptimizerDiagnostics,
    MeanReversionStageDiagnostics, ReturnComponentsStageFittedParameters,
    SeasonalityStageNormalizationMetadata,
};
use super::dataset_snapshot::CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY;
use super::shared::{
    CalibrationArtifactIoError, CalibrationArtifactPersistenceKind,
    calibration_committed_artifact_run_directory, load_validated_json_artifact,
    persist_validated_json_artifact,
};
use super::stage_references::{
    calibration_stage_artifact_reference, calibration_stage_token,
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
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION: &str =
    "calibration.optimizer.adapter_run.v1";

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
    pub hour_of_week_factor: Vec<f64>,
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
#[serde(deny_unknown_fields)]
pub struct MeanReversionStageParameterPayload {
    pub fitted_parameters: janiform_model_kernels::mean_reversion::MeanReversionFittedParameters,
    pub diagnostics: MeanReversionStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "payload_kind", content = "payload", rename_all = "snake_case")]
pub enum CalibrationStageParameterPayload {
    Seasonality(Box<SeasonalityStageParameterPayload>),
    Hawkes(Box<HawkesStageParameterPayload>),
    ReturnComponents(Box<ReturnComponentsStageParameterPayload>),
    MeanReversion(Box<MeanReversionStageParameterPayload>),
    JointRefinement(Box<JointRefinementStageParameterPayload>),
    OptionRefinement(Box<JointRefinementStageParameterPayload>),
    Finalization(Box<JointRefinementStageParameterPayload>),
}

impl CalibrationStageParameterPayload {
    const fn stage(&self) -> CalibrationStage {
        match self {
            Self::Seasonality(_) => CalibrationStage::Seasonality,
            Self::Hawkes(_) => CalibrationStage::Hawkes,
            Self::ReturnComponents(_) => CalibrationStage::ReturnComponents,
            Self::MeanReversion(_) => CalibrationStage::MeanReversion,
            Self::JointRefinement(_) => CalibrationStage::JointRefinement,
            Self::OptionRefinement(_) => CalibrationStage::OptionRefinement,
            Self::Finalization(_) => CalibrationStage::Finalization,
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
                    || payload.hour_of_week_factor.is_empty()
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

                payload
                    .fitted_parameters
                    .mean_reversion_fitted_parameters
                    .validate()
                    .map_err(|source| {
                        CalibrationStageParameterArtifactValidationError::InvalidMeanReversionParameters {
                            stage: self.stage,
                            error_message: source.to_string(),
                        }
                    })?;

                validate_optimizer_state_presence(self.stage, self.optimizer_state.as_ref())?;
            }
            CalibrationStageParameterPayload::JointRefinement(payload)
            | CalibrationStageParameterPayload::OptionRefinement(payload)
            | CalibrationStageParameterPayload::Finalization(payload) => {
                validate_non_empty_artifact_value(
                    payload.selected_candidate_id.as_str(),
                    CalibrationStageParameterArtifactValidationError::EmptyStageParameterField {
                        stage: self.stage,
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
                            stage: self.stage,
                            error_message: source.to_string(),
                        }
                    })?;

                payload
                    .baseline_return_components_fitted_parameters
                    .mean_reversion_fitted_parameters
                    .validate()
                    .map_err(|source| {
                        CalibrationStageParameterArtifactValidationError::InvalidMeanReversionParameters {
                            stage: self.stage,
                            error_message: source.to_string(),
                        }
                    })?;

                validate_optimizer_state_presence(self.stage, self.optimizer_state.as_ref())?;
            }
            CalibrationStageParameterPayload::MeanReversion(payload) => {
                payload.fitted_parameters.validate().map_err(|source| {
                    CalibrationStageParameterArtifactValidationError::InvalidMeanReversionParameters {
                        stage: self.stage,
                        error_message: source.to_string(),
                    }
                })?;

                payload
                    .diagnostics
                    .anchor_parameter_estimates
                    .validate()
                    .map_err(|source| {
                        CalibrationStageParameterArtifactValidationError::InvalidMeanReversionParameters {
                            stage: self.stage,
                            error_message: source.to_string(),
                        }
                    })?;

                validate_mean_reversion_stage_parameter_boundary(payload)?;

                if self.optimizer_state.is_some() {
                    return Err(
                        CalibrationStageParameterArtifactValidationError::UnexpectedOptimizerState {
                            stage: self.stage,
                        },
                    );
                }
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

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationStageParameterBuildMetadata {
    pub stage_output_schema_version: String,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: String,
    pub solver_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationStageParameterOptimizerStateInput {
    pub backend_id: String,
    pub random_seed: u64,
    pub selection_policy: String,
    pub penalty_policy: String,
    pub bounds_policy: String,
    pub solver_settings: CalibrationOptimizerSettings,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
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
    pub intraday_estimate_schema_version: String,
    pub day_of_week_estimate_schema_version: String,
    pub day_of_year_estimate_schema_version: String,
    pub composition_schema_version: String,
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub hour_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub normalization_metadata: SeasonalityStageNormalizationMetadata,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub optimizer_state: CalibrationStageParameterOptimizerStateInput,
    pub selected_candidate_id: String,
    pub fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_generation_policy: String,
    pub objective_weighting_policy: String,
    pub objective_metric_bucket_policy: String,
    pub activity_input_transport: CalibrationStageHandoffTransport,
    pub activity_selected_hawkes_candidate_id: Option<String>,
    pub activity_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JointRefinementStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub optimizer_state: CalibrationStageParameterOptimizerStateInput,
    pub selected_candidate_id: String,
    pub fitted_parameters: JointRefinementStageFittedParameters,
    pub baseline_hawkes_fitted_parameters: HawkesStageFittedParameters,
    pub baseline_return_components_fitted_parameters: ReturnComponentsStageFittedParameters,
    pub objective_breakdown: JointRefinementStageObjectiveBreakdown,
    pub optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeanReversionStageParameterArtifactInput {
    pub metadata: CalibrationStageParameterBuildMetadata,
    pub fitted_parameters: janiform_model_kernels::mean_reversion::MeanReversionFittedParameters,
    pub diagnostics: MeanReversionStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    pub return_components_selected_candidate_id: Option<String>,
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationStageParameterArtifactInput {
    Seasonality(Box<SeasonalityStageParameterArtifactInput>),
    Hawkes(Box<HawkesStageParameterArtifactInput>),
    ReturnComponents(Box<ReturnComponentsStageParameterArtifactInput>),
    MeanReversion(Box<MeanReversionStageParameterArtifactInput>),
    JointRefinement(Box<JointRefinementStageParameterArtifactInput>),
    OptionRefinement(Box<JointRefinementStageParameterArtifactInput>),
    Finalization(Box<JointRefinementStageParameterArtifactInput>),
}

pub fn build_calibration_stage_parameter_artifact(
    run_contract: &CalibrationRunContract,
    stage_input: CalibrationStageParameterArtifactInput,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    if run_contract.run_spec.execution_mode != CalibrationExecutionMode::Disk {
        return Err(
            CalibrationStageParameterArtifactBuildError::UnsupportedExecutionMode {
                expected: CalibrationExecutionMode::Disk,
                found: run_contract.run_spec.execution_mode,
            },
        );
    }

    let stage = match &stage_input {
        CalibrationStageParameterArtifactInput::Seasonality(_) => CalibrationStage::Seasonality,
        CalibrationStageParameterArtifactInput::Hawkes(_) => CalibrationStage::Hawkes,
        CalibrationStageParameterArtifactInput::ReturnComponents(_) => {
            CalibrationStage::ReturnComponents
        }
        CalibrationStageParameterArtifactInput::MeanReversion(_) => CalibrationStage::MeanReversion,
        CalibrationStageParameterArtifactInput::JointRefinement(_) => {
            CalibrationStage::JointRefinement
        }
        CalibrationStageParameterArtifactInput::OptionRefinement(_) => {
            CalibrationStage::OptionRefinement
        }
        CalibrationStageParameterArtifactInput::Finalization(_) => CalibrationStage::Finalization,
    };
    let artifact_reference =
        resolve_stage_output_artifact_reference(&run_contract.run_spec, stage)?;

    let artifact = match stage_input {
        CalibrationStageParameterArtifactInput::Seasonality(input) => {
            build_seasonality_stage_parameter_artifact(
                run_contract.run_spec.run_id.as_str(),
                run_contract.run_spec.execution_mode,
                *input,
                artifact_reference,
            )
        }
        CalibrationStageParameterArtifactInput::Hawkes(input) => {
            build_hawkes_stage_parameter_artifact(
                run_contract.run_spec.run_id.as_str(),
                run_contract.run_spec.execution_mode,
                *input,
                artifact_reference,
            )
        }
        CalibrationStageParameterArtifactInput::ReturnComponents(input) => {
            build_return_components_stage_parameter_artifact(
                run_contract.run_spec.run_id.as_str(),
                run_contract.run_spec.execution_mode,
                *input,
                artifact_reference,
            )
        }
        CalibrationStageParameterArtifactInput::MeanReversion(input) => {
            build_mean_reversion_stage_parameter_artifact(
                run_contract.run_spec.run_id.as_str(),
                run_contract.run_spec.execution_mode,
                *input,
                artifact_reference,
            )
        }
        CalibrationStageParameterArtifactInput::JointRefinement(input) => {
            build_joint_refinement_stage_parameter_artifact(
                run_contract.run_spec.run_id.as_str(),
                run_contract.run_spec.execution_mode,
                CalibrationStage::JointRefinement,
                *input,
                artifact_reference,
            )
        }
        CalibrationStageParameterArtifactInput::OptionRefinement(input) => {
            build_joint_refinement_stage_parameter_artifact(
                run_contract.run_spec.run_id.as_str(),
                run_contract.run_spec.execution_mode,
                CalibrationStage::OptionRefinement,
                *input,
                artifact_reference,
            )
        }
        CalibrationStageParameterArtifactInput::Finalization(input) => {
            build_joint_refinement_stage_parameter_artifact(
                run_contract.run_spec.run_id.as_str(),
                run_contract.run_spec.execution_mode,
                CalibrationStage::Finalization,
                *input,
                artifact_reference,
            )
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
    persistence_root_directory: &Path,
    run_id: &str,
    stage: CalibrationStage,
) -> PathBuf {
    let run_directory =
        calibration_committed_artifact_run_directory(persistence_root_directory, run_id);

    calibration_stage_parameter_artifact_path_in_run_directory(run_directory.as_path(), stage)
}

pub fn calibration_stage_parameter_artifact_path_in_run_directory(
    run_directory: &Path,
    stage: CalibrationStage,
) -> PathBuf {
    run_directory.join(format!(
        "{}.stage_parameter.artifact.json",
        calibration_stage_token(stage)
    ))
}

pub fn persist_calibration_stage_parameter_artifact_to_disk(
    persistence_root_directory: &Path,
    artifact: &CalibrationStageParameterArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::StageParameter;
    let artifact_path = calibration_stage_parameter_artifact_path(
        persistence_root_directory,
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

pub fn persist_calibration_stage_parameter_artifact_to_run_directory(
    run_directory: &Path,
    artifact: &CalibrationStageParameterArtifact,
) -> Result<PathBuf, CalibrationArtifactIoError> {
    let artifact_kind = CalibrationArtifactPersistenceKind::StageParameter;
    let artifact_path =
        calibration_stage_parameter_artifact_path_in_run_directory(run_directory, artifact.stage);

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
    InvalidMeanReversionParameters {
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
            Self::InvalidMeanReversionParameters {
                stage,
                error_message,
            } => write!(
                formatter,
                "stage parameter artifact for `{stage:?}` uses invalid mean_reversion_fitted_parameters: {error_message}"
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

fn build_optimizer_state(
    optimizer_state: CalibrationStageParameterOptimizerStateInput,
) -> CalibrationStageParameterOptimizerState {
    CalibrationStageParameterOptimizerState {
        schema_version: CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION.to_string(),
        optimizer_adapter_metadata: CalibrationStageParameterOptimizerAdapterMetadata {
            schema_version: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION
                .to_string(),
            backend_id: optimizer_state.backend_id,
            random_seed: optimizer_state.random_seed,
            selection_policy: optimizer_state.selection_policy,
            penalty_policy: optimizer_state.penalty_policy,
            bounds_policy: optimizer_state.bounds_policy,
            solver_settings: optimizer_state.solver_settings,
        },
        input_candidate_count: optimizer_state.input_candidate_count,
        valid_candidate_count: optimizer_state.valid_candidate_count,
        selected_candidate_raw_objective: optimizer_state.selected_candidate_raw_objective,
        selected_candidate_penalized_objective: optimizer_state
            .selected_candidate_penalized_objective,
    }
}

fn build_seasonality_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: SeasonalityStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: run_id.to_string(),
        execution_mode,
        stage: CalibrationStage::Seasonality,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: input.metadata.stage_output_schema_version,
        seed_metadata: build_stage_parameter_seed_metadata(
            input.metadata.run_global_seed,
            input.metadata.stage_seed,
            input.metadata.stage_seed_source,
            input.metadata.stage_seed_derivation_policy.as_str(),
        ),
        solver_settings: input.metadata.solver_settings,
        optimizer_state: None,
        stage_parameters: CalibrationStageParameterPayload::Seasonality(Box::new(
            SeasonalityStageParameterPayload {
                dataset_id: input.dataset_id,
                exchange_code: input.exchange_code,
                trade_events_data_reference: input.trade_events_data_reference,
                estimator_version: input.estimator_version,
                timestamp_event_input_schema_version: input.timestamp_event_input_schema_version,
                timestamp_event_preprocessed_schema_version: input
                    .timestamp_event_preprocessed_schema_version,
                intraday_estimate_schema_version: input.intraday_estimate_schema_version,
                day_of_week_estimate_schema_version: input.day_of_week_estimate_schema_version,
                day_of_year_estimate_schema_version: input.day_of_year_estimate_schema_version,
                composition_schema_version: input.composition_schema_version,
                intraday_minutes_per_bin: input.intraday_minutes_per_bin,
                intraday_factor_by_bin: input.intraday_factor_by_bin,
                day_of_week_factor: input.day_of_week_factor,
                hour_of_week_factor: input.hour_of_week_factor,
                day_of_year_factor: input.day_of_year_factor,
                normalization_metadata: input.normalization_metadata,
            },
        )),
    })
}

fn build_hawkes_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: HawkesStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: run_id.to_string(),
        execution_mode,
        stage: CalibrationStage::Hawkes,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: input.metadata.stage_output_schema_version,
        seed_metadata: build_stage_parameter_seed_metadata(
            input.metadata.run_global_seed,
            input.metadata.stage_seed,
            input.metadata.stage_seed_source,
            input.metadata.stage_seed_derivation_policy.as_str(),
        ),
        solver_settings: input.metadata.solver_settings,
        optimizer_state: Some(build_optimizer_state(input.optimizer_state)),
        stage_parameters: CalibrationStageParameterPayload::Hawkes(Box::new(
            HawkesStageParameterPayload {
                selected_candidate_id: input.selected_candidate_id,
                fitted_parameters: input.fitted_parameters,
                seasonality_input_transport: input.seasonality_input_transport,
                seasonality_dataset_id: input.seasonality_dataset_id,
                seasonality_artifact_reference: input.seasonality_artifact_reference,
            },
        )),
    })
}

fn build_return_components_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: ReturnComponentsStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: run_id.to_string(),
        execution_mode,
        stage: CalibrationStage::ReturnComponents,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: input.metadata.stage_output_schema_version,
        seed_metadata: build_stage_parameter_seed_metadata(
            input.metadata.run_global_seed,
            input.metadata.stage_seed,
            input.metadata.stage_seed_source,
            input.metadata.stage_seed_derivation_policy.as_str(),
        ),
        solver_settings: input.metadata.solver_settings,
        optimizer_state: Some(build_optimizer_state(input.optimizer_state)),
        stage_parameters: CalibrationStageParameterPayload::ReturnComponents(Box::new(
            ReturnComponentsStageParameterPayload {
                selected_candidate_id: input.selected_candidate_id,
                fitted_parameters: input.fitted_parameters,
                candidate_generation_policy: input.candidate_generation_policy,
                objective_weighting_policy: input.objective_weighting_policy,
                objective_metric_bucket_policy: input.objective_metric_bucket_policy,
                activity_input_transport: input.activity_input_transport,
                activity_selected_hawkes_candidate_id: input.activity_selected_hawkes_candidate_id,
                activity_artifact_reference: input.activity_artifact_reference,
            },
        )),
    })
}

fn build_mean_reversion_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    input: MeanReversionStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: run_id.to_string(),
        execution_mode,
        stage: CalibrationStage::MeanReversion,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: input.metadata.stage_output_schema_version,
        seed_metadata: build_stage_parameter_seed_metadata(
            input.metadata.run_global_seed,
            input.metadata.stage_seed,
            input.metadata.stage_seed_source,
            input.metadata.stage_seed_derivation_policy.as_str(),
        ),
        solver_settings: input.metadata.solver_settings,
        optimizer_state: None,
        stage_parameters: CalibrationStageParameterPayload::MeanReversion(Box::new(
            MeanReversionStageParameterPayload {
                fitted_parameters: input.fitted_parameters,
                diagnostics: input.diagnostics,
                return_components_input_transport: input.return_components_input_transport,
                return_components_selected_candidate_id: input
                    .return_components_selected_candidate_id,
                return_components_artifact_reference: input.return_components_artifact_reference,
            },
        )),
    })
}

fn build_joint_refinement_stage_parameter_artifact(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    stage: CalibrationStage,
    input: JointRefinementStageParameterArtifactInput,
    artifact_reference: String,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let JointRefinementStageParameterArtifactInput {
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

    Ok(CalibrationStageParameterArtifact {
        schema_version: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION.to_string(),
        run_id: run_id.to_string(),
        execution_mode,
        stage,
        artifact_reference,
        schema_metadata: CalibrationStageParameterSchemaMetadata {
            schema_family: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY.to_string(),
            schema_generation: CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION.to_string(),
            forward_extension_policy: CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY
                .to_string(),
        },
        output_schema_metadata: build_stage_parameter_output_schema_metadata(),
        stage_output_schema_version: metadata.stage_output_schema_version,
        seed_metadata: build_stage_parameter_seed_metadata(
            metadata.run_global_seed,
            metadata.stage_seed,
            metadata.stage_seed_source,
            metadata.stage_seed_derivation_policy.as_str(),
        ),
        solver_settings: metadata.solver_settings,
        optimizer_state: Some(build_optimizer_state(optimizer_state)),
        stage_parameters,
    })
}

fn resolve_stage_output_artifact_reference(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Result<String, CalibrationStageParameterArtifactBuildError> {
    let artifact_references = stage_output_artifact_references_for_stage(run_spec, stage);

    match artifact_references.as_slice() {
        [] => Ok(calibration_stage_artifact_reference(
            run_spec.run_id.as_str(),
            stage,
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
    if !is_non_empty_string(value) {
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

fn validate_mean_reversion_stage_parameter_boundary(
    payload: &MeanReversionStageParameterPayload,
) -> Result<(), CalibrationStageParameterArtifactValidationError> {
    match payload.return_components_input_transport {
        CalibrationStageHandoffTransport::InMemoryOutput => {
            if payload
                .return_components_selected_candidate_id
                .as_ref()
                .is_none_or(|value| !is_non_empty_string(value))
                || payload.return_components_artifact_reference.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::MeanReversionBoundaryMismatch {
                        expected_transport: "in_memory_output",
                    },
                );
            }
        }
        CalibrationStageHandoffTransport::PersistedArtifactReference => {
            if payload
                .return_components_artifact_reference
                .as_ref()
                .is_none_or(|value| !is_non_empty_string(value))
                || payload.return_components_selected_candidate_id.is_some()
            {
                return Err(
                    CalibrationStageParameterArtifactValidationError::MeanReversionBoundaryMismatch {
                        expected_transport: "persisted_artifact_reference",
                    },
                );
            }
        }
    }

    Ok(())
}
